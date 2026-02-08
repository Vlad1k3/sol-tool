use anyhow::{Context, Result};
use colored::Colorize;
use solana_account_decoder::UiAccountEncoding;
use solana_client::{
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_filter::{Memcmp, RpcFilterType},
};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};

use crate::{price, utils};

pub async fn run(rpc_url: &str, wallet_str: &str, json: bool) -> Result<()> {
    let wallet = utils::parse_pubkey(wallet_str)?;

    if !json {
        println!(
            "\n{} Full wallet scan: {}…\n",
            "🔍".bold(),
            utils::short_key(&wallet).cyan()
        );
    }

    // 1. fetch balance
    let sol_bal = tokio::task::spawn_blocking({
        let c = crate::rpc::client(rpc_url);
        let w = wallet;
        move || c.get_balance(&w)
    })
    .await?
    .context("Failed to get SOL balance")?;

    let sol = utils::lamports_to_sol(sol_bal);
    let sol_price = price::sol_price().await.unwrap_or(0.0);

    // 2. fetch token accounts
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

    let accounts = tokio::task::spawn_blocking({
        let c = crate::rpc::client(rpc_url);
        move || c.get_program_accounts_with_config(&spl_token::id(), config)
    })
    .await?
    .context("Failed to get token accounts")?;

    // 3. analyze
    let mut total_accs = 0;
    let mut empty_accs = 0;
    let mut balance_accs = 0;
    let mut delegate_accs = 0;
    let mut frozen_accs = 0;
    let mut rent_locked = 0;
    let mut rent_reclaim = 0;
    let mut mints = std::collections::HashSet::new();

    for (_, acc) in &accounts {
        if acc.data.len() < 108 {
            continue;
        } // skip invalid

        total_accs += 1;
        rent_locked += acc.lamports;

        let amount = u64::from_le_bytes(acc.data[64..72].try_into().unwrap());
        let mint = Pubkey::try_from(&acc.data[0..32]).unwrap_or_default();
        mints.insert(mint);

        let has_delegate = u32::from_le_bytes(acc.data[72..76].try_into().unwrap()) == 1;
        let is_frozen = acc.data[108] == 2;

        if amount == 0 {
            empty_accs += 1;
            if !has_delegate && !is_frozen {
                rent_reclaim += acc.lamports;
            }
        } else {
            balance_accs += 1;
        }

        if has_delegate {
            delegate_accs += 1;
        }
        if is_frozen {
            frozen_accs += 1;
        }
    }

    let reclaim_sol = utils::lamports_to_sol(rent_reclaim);
    let reclaim_usd = reclaim_sol * sol_price;
    let locked_sol = utils::lamports_to_sol(rent_locked);

    // 4. Output
    if json {
        println!(
            "{}",
            serde_json::json!({
                "wallet": wallet_str,
                "balance": { "sol": sol, "usd": sol * sol_price },
                "stats": {
                    "total_accounts": total_accs,
                    "empty": empty_accs,
                    "with_balance": balance_accs,
                    "delegated": delegate_accs,
                    "frozen": frozen_accs,
                    "unique_mints": mints.len(),
                },
                "rent": {
                    "locked_sol": locked_sol,
                    "reclaimable_sol": reclaim_sol,
                    "reclaimable_usd": reclaim_usd,
                }
            })
        );
        return Ok(());
    }

    // --- Overview ---
    let header = |t: &str| println!("  {} {}", "▸".cyan(), t.white().bold());

    header("Balance");
    println!(
        "    SOL: {} {}",
        format!("{sol:.4}").green().bold(),
        if sol_price > 0.0 {
            format!("(≈ {})", utils::format_usd(sol * sol_price))
                .dimmed()
                .to_string()
        } else {
            "".into()
        }
    );
    println!();

    header("Token Accounts");
    println!("    Total:        {}", total_accs.to_string().white());
    println!("    With balance: {}", balance_accs.to_string().green());
    println!(
        "    Empty:        {}",
        if empty_accs > 0 {
            empty_accs.to_string().yellow().bold()
        } else {
            "0".green()
        }
    );
    println!("    Unique mints: {}", mints.len().to_string().white());
    println!();

    header("Security");
    if delegate_accs > 0 {
        println!(
            "    {} {} accounts have active delegate approvals",
            "⚠".yellow(),
            delegate_accs.to_string().yellow().bold()
        );
        println!("    {}", "Consider revoking unused approvals".dimmed());
    } else {
        println!("    {} No active delegate approvals", "✅".green());
    }
    if frozen_accs > 0 {
        println!("    {} {} frozen accounts", "❄️".blue(), frozen_accs);
    }
    println!();

    header("Rent Analysis");
    println!(
        "    Total locked: {}",
        utils::format_sol(locked_sol).white()
    );
    if empty_accs > 0 {
        println!(
            "    {} Reclaimable: {} {}",
            "💰".green(),
            utils::format_sol(reclaim_sol).green().bold(),
            if reclaim_usd > 0.0 {
                format!("(≈ {})", utils::format_usd(reclaim_usd))
                    .dimmed()
                    .to_string()
            } else {
                "".into()
            }
        );
        println!(
            "    {}",
            format!("Run `sol-tool clean {wallet_str}` to reclaim").dimmed()
        );
    } else {
        println!("    {} No rent to reclaim", "✅".green());
    }
    println!();

    // --- Health Score ---
    let score = calc_score(empty_accs, delegate_accs, frozen_accs, total_accs);
    header("Wallet Health");

    let (col, label) = match score {
        90..=100 => (score.to_string().green().bold(), "Excellent"),
        70..=89 => (score.to_string().yellow().bold(), "Good"),
        50..=69 => (score.to_string().yellow(), "Fair"),
        _ => (score.to_string().red().bold(), "Needs attention"),
    };
    println!("    Score: {}/100 — {}", col, label);

    if score < 90 {
        println!("    {}", "Recommendations:".dimmed());
        if empty_accs > 0 {
            println!("     • Close {} empty accounts", empty_accs);
        }
        if delegate_accs > 0 {
            println!("     • Revoke {} delegations", delegate_accs);
        }
    }
    println!();

    Ok(())
}

fn calc_score(empty: usize, delegates: usize, frozen: usize, total: usize) -> u32 {
    if total == 0 {
        return 100;
    }
    let mut score = 100i32;

    // penalties
    let empty_pct = (empty as f64 / total as f64) * 100.0;
    score -= match empty_pct as u32 {
        0..=5 => 0,
        6..=20 => 10,
        21..=50 => 20,
        _ => 35,
    };

    score -= (delegates as i32 * 5).min(25);
    score -= (frozen as i32 * 2).min(10);

    score.clamp(0, 100) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calc_score_perfect() {
        assert_eq!(calc_score(0, 0, 0, 100), 100);
    }

    #[test]
    fn test_calc_score_empty_accounts() {
        // 5% empty (5/100) -> 0 penalty
        assert_eq!(calc_score(5, 0, 0, 100), 100);

        // 10% empty (10/100) -> 10 penalty
        assert_eq!(calc_score(10, 0, 0, 100), 90);

        // 30% empty (30/100) -> 20 penalty
        assert_eq!(calc_score(30, 0, 0, 100), 80);

        // 80% empty (80/100) -> 35 penalty
        assert_eq!(calc_score(80, 0, 0, 100), 65);
    }

    #[test]
    fn test_calc_score_delegates() {
        // 1 delegate -> 5 penalty
        assert_eq!(calc_score(0, 1, 0, 100), 95);

        // 5 delegates -> 25 penalty (capped at 25?)
        // Code says: (delegates * 5).min(25)
        assert_eq!(calc_score(0, 5, 0, 100), 75);

        // 10 delegates -> 25 penalty (capped)
        assert_eq!(calc_score(0, 10, 0, 100), 75);
    }

    #[test]
    fn test_calc_score_frozen() {
        // 1 frozen -> 2 penalty
        assert_eq!(calc_score(0, 0, 1, 100), 98);

        // 5 frozen -> 10 penalty (capped)
        assert_eq!(calc_score(0, 0, 5, 100), 90);
    }

    #[test]
    fn test_calc_score_mixed() {
        // 10% empty (-10), 1 delegate (-5), 1 frozen (-2) = 100 - 17 = 83
        assert_eq!(calc_score(10, 1, 1, 100), 83);
    }

    #[test]
    fn test_calc_score_empty_wallet() {
        assert_eq!(calc_score(0, 0, 0, 0), 100);
    }
}
