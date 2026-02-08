use crate::{price, utils};
use anyhow::{Context, Result};
use colored::Colorize;
use solana_account_decoder::UiAccountEncoding;
use solana_client::{
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_filter::{Memcmp, RpcFilterType},
};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};

#[derive(serde::Serialize, Clone)]
struct Token {
    mint: String,
    account: String,
    raw: u64,
    decimals: u8,
    balance: f64,
    price: f64,
    value: f64,
}

pub async fn run(
    rpc_url: &str,
    wallet_str: &str,
    min_usd: f64,
    sort: &str,
    json: bool,
) -> Result<()> {
    let wallet = utils::parse_pubkey(wallet_str)?;

    if !json {
        println!(
            "\n{} Loading portfolio for {}…\n",
            "💰".bold(),
            utils::short_key(&wallet).cyan()
        );
    }

    // 1. fetch sol balance
    // 1. fetch sol balance
    let sol_bal = tokio::task::spawn_blocking({
        let c = crate::rpc::client(rpc_url);
        let w = wallet;
        move || c.get_balance(&w)
    })
    .await?
    .context("Failed to get SOL balance")?;

    let sol = utils::lamports_to_sol(sol_bal);

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

    // 3. parse tokens
    let mut tokens: Vec<Token> = Vec::new();
    let mut mints: Vec<String> = vec![price::SOL_MINT.to_string()];

    for (addr, acc) in &accounts {
        if acc.data.len() < 72 {
            continue;
        } // ignore malformed

        let mint = Pubkey::try_from(&acc.data[0..32]).unwrap_or_default();
        let amount = u64::from_le_bytes(acc.data[64..72].try_into().unwrap());

        if amount == 0 {
            continue;
        }

        let mint_str = mint.to_string();
        mints.push(mint_str.clone());

        tokens.push(Token {
            mint: mint_str,
            account: addr.to_string(),
            raw: amount,
            decimals: 0, // fetch later
            balance: 0.0,
            price: 0.0,
            value: 0.0,
        });
    }

    // 4. fetch decimals for mints
    {
        let pks: Vec<Pubkey> = tokens
            .iter()
            .map(|t| utils::parse_pubkey(&t.mint).unwrap())
            .collect();
        if !pks.is_empty() {
            let mint_accs = tokio::task::spawn_blocking({
                let c = crate::rpc::client(rpc_url);
                let p = pks.clone();
                move || c.get_multiple_accounts(&p)
            })
            .await?
            .context("Failed to get mint info")?;

            for (i, m_acc) in mint_accs.iter().enumerate() {
                if let Some(acc) = m_acc {
                    if acc.data.len() > 44 {
                        tokens[i].decimals = acc.data[44];
                        tokens[i].balance = utils::token_amount(tokens[i].raw, tokens[i].decimals);
                    }
                }
            }
        }
    }

    // 5. prices
    if !json {
        println!("  Fetching prices…");
    }
    let prices = price::fetch_prices(&mints).await.unwrap_or_default();

    let sol_price = prices.get(price::SOL_MINT).copied().unwrap_or(0.0);
    let sol_val = sol * sol_price;

    for t in &mut tokens {
        t.price = prices.get(&t.mint).copied().unwrap_or(0.0);
        t.value = t.balance * t.price;
    }

    // 6. sort
    sort_tokens(&mut tokens, sort);

    // 7. filter & sum
    let visible: Vec<&Token> = filter_tokens(&tokens, min_usd);

    let total_token_usd: f64 = tokens.iter().map(|t| t.value).sum();
    let total = sol_val + total_token_usd;

    // 8. output
    if json {
        println!(
            "{}",
            serde_json::json!({
                "wallet": wallet_str,
                "sol": { "balance": sol, "price": sol_price, "value": sol_val },
                "tokens": tokens,
                "total_usd": total,
            })
        );
        return Ok(());
    }

    println!();
    // SOL
    println!(
        "  {} {} {}",
        "SOL".white().bold(),
        format!("{sol:.4}").green(),
        if sol_price > 0.0 {
            format!(
                "× {} = {}",
                utils::format_usd(sol_price),
                utils::format_usd(sol_val)
            )
            .dimmed()
            .to_string()
        } else {
            "".into()
        }
    );

    println!("  {}", "─".repeat(60).dimmed());

    if visible.is_empty() {
        println!("  {}", "No tokens found".dimmed());
    } else {
        for t in &visible {
            let short = format!("{}…{}", &t.mint[..6], &t.mint[t.mint.len() - 4..]);
            let p_str = if t.price > 0.0 {
                format!(
                    "× {} = {}",
                    utils::format_usd(t.price),
                    utils::format_usd(t.value)
                )
                .dimmed()
                .to_string()
            } else {
                "(no price)".dimmed().to_string()
            };

            let b_str = if t.balance < 0.001 {
                format!("{:.9}", t.balance)
            } else if t.balance < 1.0 {
                format!("{:.6}", t.balance)
            } else if t.balance < 100_000.0 {
                format!("{:.2}", t.balance)
            } else {
                format!("{:.0}", t.balance)
            };

            println!("  {} {} {}", short.white(), b_str.green(), p_str);
        }

        let hidden = tokens.len() - visible.len();
        if hidden > 0 {
            println!(
                "  {} {hidden} small tokens hidden (< {})",
                "…".dimmed(),
                utils::format_usd(min_usd)
            );
        }
    }

    println!("  {}", "─".repeat(60).dimmed());
    if total > 0.0 {
        println!(
            "  {} {}",
            "Total:".white().bold(),
            utils::format_usd(total).green().bold()
        );
    }
    println!("  {} {} accounts\n", "📊", accounts.len());

    Ok(())
}

fn sort_tokens(tokens: &mut [Token], sort_by: &str) {
    match sort_by {
        "name" => tokens.sort_by(|a, b| a.mint.cmp(&b.mint)),
        "balance" => tokens.sort_by(|a, b| {
            b.balance
                .partial_cmp(&a.balance)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        _ => tokens.sort_by(|a, b| {
            b.value
                .partial_cmp(&a.value)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
    }
}

fn filter_tokens(tokens: &[Token], min_usd: f64) -> Vec<&Token> {
    tokens
        .iter()
        .filter(|t| t.value >= min_usd || (t.price == 0.0 && t.balance > 0.0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_token(mint: &str, balance: f64, value: f64, price: f64) -> Token {
        Token {
            mint: mint.to_string(),
            account: "acc".to_string(),
            raw: 0,
            decimals: 9,
            balance,
            price,
            value,
        }
    }

    #[test]
    fn test_sort_tokens() {
        let mut tokens = vec![
            mock_token("A", 10.0, 10.0, 1.0),
            mock_token("B", 5.0, 50.0, 10.0),
            mock_token("C", 20.0, 5.0, 0.25),
        ];

        // Default (Value desc)
        sort_tokens(&mut tokens, "default");
        assert_eq!(tokens[0].mint, "B"); // 50
        assert_eq!(tokens[1].mint, "A"); // 10
        assert_eq!(tokens[2].mint, "C"); // 5

        // Balance desc
        sort_tokens(&mut tokens, "balance");
        assert_eq!(tokens[0].mint, "C"); // 20
        assert_eq!(tokens[1].mint, "A"); // 10
        assert_eq!(tokens[2].mint, "B"); // 5

        // Name asc
        sort_tokens(&mut tokens, "name");
        assert_eq!(tokens[0].mint, "A");
        assert_eq!(tokens[1].mint, "B");
        assert_eq!(tokens[2].mint, "C");
    }

    #[test]
    fn test_filter_tokens() {
        let tokens = vec![
            mock_token("HighVal", 10.0, 100.0, 10.0),
            mock_token("LowVal", 10.0, 0.5, 0.05),
            mock_token("NoPrice", 10.0, 0.0, 0.0),
            mock_token("Dust", 0.0, 0.0, 0.0),
        ];

        // Min USD = 1.0
        // HighVal (100 > 1) -> Keep
        // LowVal (0.5 < 1) -> Drop
        // NoPrice (0 val, but bal > 0) -> Keep
        // Dust (0 val, 0 bal) -> Drop
        let visible = filter_tokens(&tokens, 1.0);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].mint, "HighVal");
        assert_eq!(visible[1].mint, "NoPrice");
    }
}
