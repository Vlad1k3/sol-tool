use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::Confirm;
use indicatif::{ProgressBar, ProgressStyle};
use solana_account_decoder::UiAccountEncoding;
use solana_client::{
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_filter::{Memcmp, RpcFilterType},
};
use solana_sdk::{
    commitment_config::CommitmentConfig, compute_budget::ComputeBudgetInstruction, pubkey::Pubkey,
    signature::Keypair, signer::Signer, transaction::Transaction,
};
use spl_token::instruction::close_account;

use crate::solanapay;
use crate::utils;

#[derive(serde::Serialize)]
struct CloseableAccount {
    address: String,
    mint: String,
    token_balance: f64,
    rent_lamports: u64,
}

pub async fn run(
    rpc_url: &str,
    wallet_str: Option<&str>,
    keypair_path: Option<&str>,
    file_path: Option<&str>,
    dry_run: bool,
    batch_size: usize,
    dust_threshold: Option<f64>,
    connect: bool,
    json: bool,
) -> Result<()> {
    //  Batch mode: process CSV file
    if let Some(path) = file_path {
        return run_batch(rpc_url, path, dry_run, batch_size, dust_threshold, json).await;
    }

    //  Connect Flow
    let wallet = if connect && wallet_str.is_none() {
        // use shared logic
        crate::solanapay::connect_wallet().await?
    } else {
        // parse arg
        let s = wallet_str.ok_or_else(|| {
            anyhow::anyhow!("Wallet address needed. Use --connect or pass address.")
        })?;
        utils::parse_pubkey(s)?
    };

    let client = crate::rpc::client(rpc_url);

    if !json {
        println!(
            "\n{} Scanning accounts for {}...",
            "🧹".bold(),
            utils::short_key(&wallet).cyan()
        );
    }

    let closeable = fetch_and_analyze(rpc_url, &wallet, dust_threshold).await?;

    if !json {
        println!(
            "  Found {} accounts total",
            closeable.len().to_string().white().bold()
        );
    }

    if closeable.is_empty() {
        if json {
            println!(
                "{}",
                serde_json::json!({ "status": "clean", "closeable": 0 })
            );
        } else {
            println!("\n  {}", "✅ Wallet is clean!".green());
        }
        return Ok(());
    }

    let total_rent: u64 = closeable.iter().map(|c| c.1.rent_lamports).sum();
    let total_sol = utils::lamports_to_sol(total_rent);

    // get price
    let sol_usd = crate::price::sol_price().await.unwrap_or(0.0);
    let total_usd = total_sol * sol_usd;

    if json {
        let accs: Vec<&CloseableAccount> = closeable.iter().map(|c| &c.1).collect();
        println!(
            "{}",
            serde_json::json!({
                "status": "found",
                "closeable": closeable.len(),
                "reclaimable_sol": total_sol,
                "reclaimable_usd": total_usd,
                "dry_run": dry_run,
                "accounts": accs,
            })
        );
        if dry_run {
            return Ok(());
        }
    } else {
        println!(
            "\n  {} empty/dust accounts found",
            closeable.len().to_string().yellow().bold()
        );

        let usd_str = if sol_usd > 0.0 {
            format!("(≈ {})", utils::format_usd(total_usd))
                .dimmed()
                .to_string()
        } else {
            "".to_string()
        };
        println!(
            "  {} reclaimable {}",
            utils::format_sol(total_sol).green().bold(),
            usd_str
        );

        println!();
        let show = closeable.len().min(12);
        for (_, acc) in &closeable[..show] {
            let dust = if acc.token_balance > 0.0 {
                format!(" dust:{:.6}", acc.token_balance)
                    .dimmed()
                    .to_string()
            } else {
                "".to_string()
            };
            println!(
                "    {} → {}{}",
                acc.address[..16].dimmed(),
                utils::format_sol(utils::lamports_to_sol(acc.rent_lamports)).white(),
                dust
            );
        }
        if closeable.len() > show {
            println!("    … and {} more", closeable.len() - show);
        }

        if dry_run {
            println!("\n  {} Dry run — remove flag to execute.\n", "🔍".yellow());
            return Ok(());
        }
    }

    //  Execution

    if connect {
        // SOLANA PAY MODE
        if !json {
            if !Confirm::new()
                .with_prompt(format!(
                    "Close {} accounts via mobile wallet?",
                    closeable.len()
                ))
                .default(true)
                .interact()?
            {
                println!("{}", "  Cancelled.".dimmed());
                return Ok(());
            }
        }

        let recent_hash = client.get_latest_blockhash()?;
        let mut all_transactions = Vec::new();

        // batch ixs
        for batch in closeable.chunks(batch_size) {
            let mut ixs = vec![
                ComputeBudgetInstruction::set_compute_unit_limit(batch.len() as u32 * 3000 + 5000),
                ComputeBudgetInstruction::set_compute_unit_price(1000),
            ];
            for (addr, _) in batch.iter() {
                ixs.push(close_account(
                    &spl_token::id(),
                    addr,
                    &wallet,
                    &wallet,
                    &[],
                )?);
            }
            let mut tx = Transaction::new_with_payer(&ixs, Some(&wallet));
            tx.message.recent_blockhash = recent_hash;
            all_transactions.push(tx);
        }

        println!("\n{}", "📱 Preparing transaction...".cyan().bold());

        // upload to relay
        let solana_pay_url = solanapay::upload_transactions(
            solanapay::DEFAULT_RELAY_URL,
            &all_transactions,
            &wallet,
            "sol-tool: Close Empty Accounts",
        )
        .await?;

        println!("{}", "✓ Uploaded successfully".green());
        solanapay::display_qr(&solana_pay_url)?;

        println!(
            "\n{}",
            "Scan QR with your wallet to sign and send.".dimmed()
        );
        println!(
            "{}",
            format!(
                "Reclaiming ~{} from {} accounts",
                utils::format_sol(total_sol),
                closeable.len()
            )
            .green()
        );

        return Ok(());
    }

    // KEYPAIR MODE
    let keypair = if let Some(path_or_key) = keypair_path {
        if std::path::Path::new(path_or_key).exists() {
            solana_sdk::signature::read_keypair_file(path_or_key)
                .map_err(|e| anyhow::anyhow!("Failed keypair file: {}", e))?
        } else {
            let bytes = bs58::decode(path_or_key)
                .into_vec()
                .map_err(|_| anyhow::anyhow!("Invalid keypair"))?;
            Keypair::try_from(bytes.as_slice())
                .map_err(|e| anyhow::anyhow!("Invalid bytes: {}", e))?
        }
    } else {
        utils::load_keypair(None)?
    };

    utils::verify_keypair(&keypair, &wallet)?;

    if !json {
        if !Confirm::new()
            .with_prompt(format!(
                "Close {} accounts and reclaim {}?",
                closeable.len(),
                utils::format_sol(total_sol)
            ))
            .default(false)
            .interact()?
        {
            println!("{}", "  Cancelled.".dimmed());
            return Ok(());
        }
    }

    // execute batches
    let batches: Vec<Vec<&(Pubkey, CloseableAccount)>> = closeable
        .chunks(batch_size)
        .map(|c| c.iter().collect())
        .collect();

    let pb: Option<ProgressBar> = if !json {
        let p = ProgressBar::new(batches.len() as u64);
        p.set_style(
            ProgressStyle::default_bar()
                .template("  {spinner:.green} [{bar:30}] {pos}/{len}")
                .unwrap()
                .progress_chars("█▓░"),
        );
        Some(p)
    } else {
        None
    };

    let mut closed = 0usize;
    let mut reclaimed = 0u64;
    let mut sigs = Vec::new();

    for (i, batch) in batches.iter().enumerate() {
        if let Some(ref p) = pb {
            p.set_message(format!("batch {}", i + 1));
        }

        let mut ixs = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(batch.len() as u32 * 3000 + 5000),
            ComputeBudgetInstruction::set_compute_unit_price(1000),
        ];

        for (addr, _) in batch {
            ixs.push(close_account(
                &spl_token::id(),
                addr,
                &wallet,
                &wallet,
                &[],
            )?);
        }

        let lh = client.get_latest_blockhash()?;
        let tx = Transaction::new_signed_with_payer(&ixs, Some(&wallet), &[&keypair], lh);

        match client.send_and_confirm_transaction(&tx) {
            Ok(sig) => {
                let br: u64 = batch.iter().map(|b| b.1.rent_lamports).sum();
                closed += batch.len();
                reclaimed += br;
                sigs.push(sig.to_string());
                if let Some(ref p) = pb {
                    p.inc(1);
                }
            }
            Err(e) => {
                if !json {
                    if let Some(ref p) = pb {
                        p.println(format!("  {} batch {} failed: {}", "⚠".yellow(), i + 1, e));
                    }
                }
            }
        }
    }

    if let Some(p) = pb {
        p.finish_and_clear();
    }

    if json {
        println!(
            "{}",
            serde_json::json!({
                "status": "done",
                "closed": closed,
                "reclaimed_sol": utils::lamports_to_sol(reclaimed),
                "signatures": sigs
            })
        );
    } else {
        println!(
            "\n  {} {} accounts closed",
            "✅".green(),
            closed.to_string().green().bold()
        );
        let usd_str = if sol_usd > 0.0 {
            format!(
                "(≈ {})",
                utils::format_usd(utils::lamports_to_sol(reclaimed) * sol_usd)
            )
            .dimmed()
            .to_string()
        } else {
            "".to_string()
        };
        println!(
            "  {} {} reclaimed {}",
            "💰",
            utils::format_sol(utils::lamports_to_sol(reclaimed))
                .green()
                .bold(),
            usd_str
        );

        for sig in &sigs {
            println!("     https://solscan.io/tx/{}", sig.dimmed());
        }
        println!();
    }

    Ok(())
}

async fn fetch_and_analyze(
    rpc_url: &str,
    wallet: &Pubkey,
    dust_threshold: Option<f64>,
) -> Result<Vec<(Pubkey, CloseableAccount)>> {
    let rpc_url = rpc_url.to_string();
    let wallet = *wallet;

    let accounts = tokio::task::spawn_blocking(move || {
        let client = crate::rpc::client(&rpc_url);
        let config = RpcProgramAccountsConfig {
            filters: Some(vec![
                RpcFilterType::DataSize(165),
                RpcFilterType::Memcmp(Memcmp::new_raw_bytes(32, wallet.to_bytes().to_vec())),
            ]),
            account_config: RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                commitment: Some(CommitmentConfig::confirmed()),
                ..Default::default()
            },
            ..Default::default()
        };
        client.get_program_accounts_with_config(&spl_token::id(), config)
    })
    .await?
    .context("Failed to fetch accounts")?;

    let dust_lamports = dust_threshold.map(|d| (d * 1e9) as u64).unwrap_or(0);
    // Use the pure logic filter
    // Transform Account to (Pubkey, Account) for the filter
    let accounts_with_pubkey: Vec<(Pubkey, solana_sdk::account::Account)> = accounts;
    Ok(filter_closeable_accounts(
        accounts_with_pubkey,
        dust_lamports,
    ))
}

/// Pure logic: Filter accounts that should be closed
fn filter_closeable_accounts(
    accounts: Vec<(Pubkey, solana_sdk::account::Account)>,
    dust_lamports: u64,
) -> Vec<(Pubkey, CloseableAccount)> {
    let mut closeable = Vec::new();

    for (addr, acc) in accounts {
        let data = &acc.data;
        if data.len() < 72 {
            continue;
        }

        let amount = u64::from_le_bytes(data[64..72].try_into().unwrap_or([0u8; 8]));
        let mint = Pubkey::try_from(&data[0..32]).unwrap_or_default();

        let is_empty = amount == 0;
        let is_dust = dust_lamports > 0 && amount > 0 && amount <= dust_lamports;

        if is_empty || is_dust {
            // Check delegate (u32 at offset 72)
            let has_delegate = data.len() > 76
                && u32::from_le_bytes(data[72..76].try_into().unwrap_or([0u8; 4])) == 1;

            // Check frozen (u8 at offset 108)
            let is_frozen = data.len() > 108 && data[108] == 2;

            if !has_delegate && !is_frozen {
                closeable.push((
                    addr,
                    CloseableAccount {
                        address: addr.to_string(),
                        mint: mint.to_string(),
                        token_balance: utils::token_amount(amount, 9),
                        rent_lamports: acc.lamports,
                    },
                ));
            }
        }
    }
    closeable
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::account::Account;

    fn create_token_account(amount: u64, delegate: Option<Pubkey>, state: u8) -> Account {
        let mut data = vec![0u8; 165];
        // Mint (0..32) - zeros
        // Owner (32..64) - zeros
        // Amount (64..72)
        data[64..72].copy_from_slice(&amount.to_le_bytes());

        // Delegate (72..108)
        if let Some(d) = delegate {
            data[72..76].copy_from_slice(&1u32.to_le_bytes()); // Option::Some
            data[76..108].copy_from_slice(d.as_ref());
        }

        // State (108) - 1=Initialized, 2=Frozen
        data[108] = state;

        Account {
            lamports: 2_039_280, // rent exempt
            data,
            owner: spl_token::id(),
            executable: false,
            rent_epoch: 0,
        }
    }

    #[test]
    fn test_filter_empty_accounts() {
        let addr = Pubkey::new_unique();
        let acc = create_token_account(0, None, 1); // Empty, No delegate, Initialized

        let candidates = filter_closeable_accounts(vec![(addr, acc)], 0);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].0, addr);
    }

    #[test]
    fn test_filter_non_empty_accounts() {
        let addr = Pubkey::new_unique();
        let acc = create_token_account(100, None, 1); // Balance 100

        let candidates = filter_closeable_accounts(vec![(addr, acc)], 0);
        assert!(candidates.is_empty(), "Should not close non-empty account");
    }

    #[test]
    fn test_filter_dust_accounts() {
        let addr = Pubkey::new_unique();
        let acc = create_token_account(100, None, 1); // Balance 100

        // Dust threshold 200 > 100 -> Should close
        let candidates = filter_closeable_accounts(vec![(addr, acc)], 200);
        assert_eq!(candidates.len(), 1, "Should close dust account");

        // Dust threshold 50 < 100 -> Keep
        let acc2 = create_token_account(100, None, 1);
        let candidates2 = filter_closeable_accounts(vec![(addr, acc2)], 50);
        assert!(
            candidates2.is_empty(),
            "Should keep account above dust threshold"
        );
    }

    #[test]
    fn test_filter_frozen_accounts() {
        let addr = Pubkey::new_unique();
        let acc = create_token_account(0, None, 2); // Empty but Frozen (state=2)

        let candidates = filter_closeable_accounts(vec![(addr, acc)], 0);
        assert!(candidates.is_empty(), "Must NOT close frozen accounts");
    }

    #[test]
    fn test_filter_delegated_accounts() {
        let addr = Pubkey::new_unique();
        let delegate = Pubkey::new_unique();
        let acc = create_token_account(0, Some(delegate), 1); // Delegated

        let candidates = filter_closeable_accounts(vec![(addr, acc)], 0);
        assert!(candidates.is_empty()); // Should be filtered out
    }

    /// Test CSV line parsing logic (simulates batch mode parsing)
    #[test]
    fn test_csv_line_parsing() {
        // Valid line from wallets.csv
        let line = "9sRRkYzseywA5zjLd2tqZLAgNgK6X4MVbagrNTmM8jAw,5Matzsut1HNJqtbPT4FWjAaZZbqw6UEDhSb7KSqTtsXqk3nDt8XXr4wRHZo8c4q73XwqKpv8D7roS3v1NWN1AAwy";
        let parts: Vec<&str> = line.split(',').collect();

        assert_eq!(parts.len(), 2);

        let pubkey_str = parts[0].trim();
        let privkey_str = parts[1].trim();

        // Parse pubkey
        let pubkey: Result<Pubkey, _> = pubkey_str.parse();
        assert!(pubkey.is_ok());

        // Parse private key (base58)
        let bytes = bs58::decode(privkey_str).into_vec();
        assert!(bytes.is_ok());

        let keypair = Keypair::try_from(bytes.unwrap().as_slice());
        assert!(keypair.is_ok());

        // Verify keypair matches pubkey
        assert_eq!(keypair.unwrap().pubkey(), pubkey.unwrap());
    }

    #[test]
    fn test_csv_skip_header_and_comments() {
        let lines = vec![
            "WALLET,PRIVATE KEY",  // Header - should skip
            "# This is a comment", // Comment - should skip
            "",                    // Empty - should skip
        ];

        for line in lines {
            let line = line.trim();
            let should_skip = line.is_empty()
                || line.starts_with('#')
                || line.to_uppercase().starts_with("WALLET");
            assert!(should_skip, "Line should be skipped: {}", line);
        }
    }

    #[test]
    fn test_csv_invalid_format() {
        let invalid_lines = vec![
            "only_one_field", // Missing comma
            "a,b,c",          // Too many fields (we only expect 2)
        ];

        for line in invalid_lines {
            let parts: Vec<&str> = line.split(',').collect();
            assert_ne!(parts.len(), 2, "Line should be invalid: {}", line);
        }
    }
}

/// Batch mode: process multiple wallets from CSV file
/// Format: public_key,private_key (one per line)
async fn run_batch(
    rpc_url: &str,
    file_path: &str,
    dry_run: bool,
    batch_size: usize,
    dust_threshold: Option<f64>,
    _json: bool,
) -> Result<()> {
    use std::io::BufRead;

    let file =
        std::fs::File::open(file_path).context(format!("Failed to open file: {}", file_path))?;
    let reader = std::io::BufReader::new(file);

    let mut wallets: Vec<(Pubkey, Keypair)> = Vec::new();

    for (line_num, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() != 2 {
            eprintln!(
                "{}",
                format!(
                    "⚠ Line {}: invalid format (expected: pubkey,privatekey)",
                    line_num + 1
                )
                .yellow()
            );
            continue;
        }

        let pubkey_str = parts[0].trim();
        let privkey_str = parts[1].trim();

        // Parse pubkey
        let pubkey: Pubkey = match pubkey_str.parse() {
            Ok(pk) => pk,
            Err(_) => {
                eprintln!(
                    "{}",
                    format!("⚠ Line {}: invalid public key", line_num + 1).yellow()
                );
                continue;
            }
        };

        // Parse private key (base58)
        let keypair = match bs58::decode(privkey_str).into_vec() {
            Ok(bytes) => match Keypair::try_from(bytes.as_slice()) {
                Ok(kp) => kp,
                Err(_) => {
                    eprintln!(
                        "{}",
                        format!("⚠ Line {}: invalid keypair bytes", line_num + 1).yellow()
                    );
                    continue;
                }
            },
            Err(_) => {
                eprintln!(
                    "{}",
                    format!("⚠ Line {}: invalid base58 private key", line_num + 1).yellow()
                );
                continue;
            }
        };

        // Verify keypair matches pubkey
        if keypair.pubkey() != pubkey {
            eprintln!(
                "{}",
                format!("⚠ Line {}: keypair doesn't match pubkey", line_num + 1).yellow()
            );
            continue;
        }

        wallets.push((pubkey, keypair));
    }

    if wallets.is_empty() {
        anyhow::bail!("No valid wallets found in CSV file");
    }

    let wallets_count = wallets.len();
    println!(
        "\n{} Processing {} wallets from {} (parallel)\n",
        "📁".bold(),
        wallets.len().to_string().cyan(),
        file_path.dimmed()
    );

    let rpc_url_arc = std::sync::Arc::new(rpc_url.to_string());
    let sol_usd = crate::price::sol_price().await.unwrap_or(0.0);

    // Process wallets in parallel with semaphore for rate limiting
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(10)); // 10 concurrent

    let mut handles = Vec::new();

    for (idx, (wallet, keypair)) in wallets.into_iter().enumerate() {
        let sem = semaphore.clone();
        let rpc = rpc_url_arc.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();

            let client = crate::rpc::client(&rpc);

            // Fetch token accounts
            let config = RpcProgramAccountsConfig {
                filters: Some(vec![
                    RpcFilterType::DataSize(165),
                    RpcFilterType::Memcmp(Memcmp::new_raw_bytes(32, wallet.to_bytes().to_vec())),
                ]),
                account_config: RpcAccountInfoConfig {
                    encoding: Some(UiAccountEncoding::Base64),
                    commitment: Some(CommitmentConfig::confirmed()),
                    ..Default::default()
                },
                ..Default::default()
            };

            let accounts = match client.get_program_accounts_with_config(&spl_token::id(), config) {
                Ok(acc) => acc,
                Err(_) => {
                    return (idx, wallet, 0usize, 0u64, false);
                }
            };

            // Find closeable accounts (using shared logic)
            let dust_lamports = dust_threshold.map(|d| (d * 1e9) as u64).unwrap_or(0);

            // filter_closeable_accounts expects Vec<(Pubkey, Account)>
            // get_program_accounts returns Vec<(Pubkey, Account)>
            let candidates = filter_closeable_accounts(accounts, dust_lamports);

            if candidates.is_empty() {
                return (idx, wallet, 0, 0, true);
            }

            let rent_total: u64 = candidates.iter().map(|(_, c)| c.rent_lamports).sum();

            if dry_run {
                return (idx, wallet, candidates.len(), rent_total, true);
            }

            // Close accounts
            let closeable = candidates; // alias for clarity

            // Fetch blockhash or skip if fails
            let recent_hash = match client.get_latest_blockhash() {
                Ok(h) => h,
                Err(_) => return (idx, wallet, closeable.len(), 0, false),
            };

            let mut closed = 0usize;
            let mut reclaimed = 0u64;

            for chunk in closeable.chunks(batch_size) {
                let mut ixs = vec![
                    ComputeBudgetInstruction::set_compute_unit_limit(
                        chunk.len() as u32 * 3000 + 5000,
                    ),
                    ComputeBudgetInstruction::set_compute_unit_price(1000),
                ];

                for (addr, _) in chunk {
                    if let Ok(ix) = close_account(&spl_token::id(), addr, &wallet, &wallet, &[]) {
                        ixs.push(ix);
                    }
                }

                let tx = Transaction::new_signed_with_payer(
                    &ixs,
                    Some(&wallet),
                    &[&keypair],
                    recent_hash,
                );

                if client.send_and_confirm_transaction(&tx).is_ok() {
                    closed += chunk.len();
                    reclaimed += chunk.iter().map(|(_, acc)| acc.rent_lamports).sum::<u64>();
                }
            }

            (idx, wallet, closed, reclaimed, true)
        });

        handles.push(handle);
    }

    // Collect results
    let mut total_closed = 0u64;
    let mut total_reclaimed = 0u64;
    let mut wallet_results: Vec<(usize, Pubkey, usize, u64, bool)> = Vec::new();

    for handle in handles {
        if let Ok(result) = handle.await {
            wallet_results.push(result);
        }
    }

    // Sort by index and print
    wallet_results.sort_by_key(|(idx, _, _, _, _)| *idx);

    for (idx, wallet, closed, reclaimed, success) in wallet_results {
        if !success {
            println!(
                "{} {} Failed",
                format!("[{}/{}]", idx + 1, wallets_count).dimmed(),
                utils::short_key(&wallet)
            );
        } else if closed == 0 && reclaimed == 0 {
            println!(
                "{} {} ✓ Clean",
                format!("[{}/{}]", idx + 1, wallets_count).dimmed(),
                utils::short_key(&wallet)
            );
        } else if dry_run {
            println!(
                "{} {} 📋 {} accounts, {} reclaimable",
                format!("[{}/{}]", idx + 1, wallets_count).dimmed(),
                utils::short_key(&wallet),
                closed.to_string().yellow(),
                utils::format_sol(utils::lamports_to_sol(reclaimed)).green()
            );
        } else {
            println!(
                "{} {} ✓ Closed {} → {}",
                format!("[{}/{}]", idx + 1, wallets_count).dimmed(),
                utils::short_key(&wallet),
                closed.to_string().green(),
                utils::format_sol(utils::lamports_to_sol(reclaimed)).green()
            );
        }
        total_closed += closed as u64;
        total_reclaimed += reclaimed;
    }

    // Summary
    println!("\n{}", "═══ Summary ═══".bold());
    println!(
        "  Total closed: {}",
        total_closed.to_string().green().bold()
    );
    println!(
        "  Total reclaimed: {} (≈ {})",
        utils::format_sol(utils::lamports_to_sol(total_reclaimed))
            .green()
            .bold(),
        utils::format_usd(utils::lamports_to_sol(total_reclaimed) * sol_usd).dimmed()
    );

    Ok(())
}
