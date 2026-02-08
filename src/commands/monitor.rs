use crate::utils;
use anyhow::{Context, Result};
use colored::Colorize;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::{commitment_config::CommitmentConfig, signature::Signature};
use solana_transaction_status::UiTransactionEncoding;
use std::collections::HashSet;

pub async fn run(rpc_url: &str, wallet_str: &str, interval: u64) -> Result<()> {
    let wallet = utils::parse_pubkey(wallet_str)?;

    println!(
        "\n{} Monitoring transactions for {}",
        "📡".bold(),
        utils::short_key(&wallet).cyan()
    );
    println!("  {} Press Ctrl+C to stop\n", "ℹ".dimmed());

    let mut seen: HashSet<Signature> = HashSet::new();
    let delay = std::time::Duration::from_secs(interval);

    // load initial state so we don't spam old txs
    {
        let client = crate::rpc::client(rpc_url);
        let w = wallet;
        let initial = tokio::task::spawn_blocking(move || client.get_signatures_for_address(&w))
            .await?
            .context("Failed initial fetch")?;

        for info in &initial {
            if let Ok(sig) = utils::parse_signature(&info.signature) {
                seen.insert(sig);
            }
        }
        println!(
            "  {} Loaded {} existing transactions, watching for new…\n",
            "✓".green(),
            seen.len()
        );
    }

    loop {
        let client = crate::rpc::client(rpc_url);
        let w = wallet;

        // fetch signatures
        let sigs = tokio::task::spawn_blocking(move || client.get_signatures_for_address(&w))
            .await?
            .context("RPC error")?;

        // process new ones (reverse to show oldest first)
        for info in sigs.iter().rev() {
            let sig = match utils::parse_signature(&info.signature) {
                Ok(s) => s,
                Err(_) => continue,
            };

            if seen.contains(&sig) {
                continue;
            }
            seen.insert(sig);

            // fetch details for new tx
            let client = crate::rpc::client(rpc_url);
            let config = RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::JsonParsed),
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(0),
            };

            let s_clone = sig;
            let tx_result = tokio::task::spawn_blocking(move || {
                client.get_transaction_with_config(&s_clone, config)
            })
            .await?;

            let time = if let Some(bt) = info.block_time {
                chrono::DateTime::from_timestamp(bt, 0)
                    .map(|dt| dt.format("%H:%M:%S").to_string())
                    .unwrap_or_else(|| "?".into())
            } else {
                "?".into()
            };

            let status = if info.err.is_some() {
                "FAIL".red().bold()
            } else {
                "OK".green().bold()
            };

            // try to guess balance change
            let change = tx_result.ok().and_then(|tx| {
                if let Some(meta) = tx.transaction.meta {
                    estimate_balance_change(&meta.pre_balances, &meta.post_balances)
                } else {
                    None
                }
            });

            let change_str = match change {
                Some(d) if d > 0.0 => format!("+{d:.6} SOL").green().to_string(),
                Some(d) if d < 0.0 => format!("{d:.6} SOL").red().to_string(),
                _ => String::new(),
            };

            let memo = info
                .memo
                .as_ref()
                .map(|m| format!(" memo:{}", m.dimmed()))
                .unwrap_or_default();

            println!(
                "  {} [{}] {} {} {} {}",
                time.dimmed(),
                status,
                utils::short_sig(&sig).white(),
                change_str,
                memo,
                format!("https://solscan.io/tx/{sig}").dimmed(),
            );
        }

        tokio::time::sleep(delay).await;
    }
}

fn estimate_balance_change(pre: &[u64], post: &[u64]) -> Option<f64> {
    if !pre.is_empty() && !post.is_empty() {
        let diff = post[0] as i64 - pre[0] as i64;
        if diff != 0 {
            return Some(diff as f64 / 1e9);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_balance_change() {
        // Increase
        assert_eq!(
            estimate_balance_change(&[1_000_000_000], &[2_500_000_000]),
            Some(1.5)
        );

        // Decrease
        assert_eq!(
            estimate_balance_change(&[2_000_000_000], &[1_000_000_000]),
            Some(-1.0)
        );

        // No change
        assert_eq!(
            estimate_balance_change(&[1_000_000_000], &[1_000_000_000]),
            None
        );

        // Empty
        assert_eq!(estimate_balance_change(&[], &[]), None);
    }
}
