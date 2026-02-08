use crate::{price, utils};
use anyhow::Result;
use colored::Colorize;

struct RentEntry {
    name: &'static str,
    size: usize,
}

const COMMON_ACCOUNTS: &[RentEntry] = &[
    RentEntry {
        name: "System Account",
        size: 0,
    },
    RentEntry {
        name: "Token Account (SPL)",
        size: 165,
    },
    RentEntry {
        name: "Mint Account",
        size: 82,
    },
    RentEntry {
        name: "Token-2022 Account",
        size: 170,
    },
    RentEntry {
        name: "Token-2022 Mint",
        size: 82,
    },
    RentEntry {
        name: "Metadata (Metaplex)",
        size: 679,
    },
    RentEntry {
        name: "Nonce Account",
        size: 80,
    },
    RentEntry {
        name: "Stake Account",
        size: 200,
    },
    RentEntry {
        name: "Vote Account",
        size: 3762,
    },
    RentEntry {
        name: "AMM Pool (typical)",
        size: 752,
    },
    RentEntry {
        name: "OpenBook Market",
        size: 388,
    },
    RentEntry {
        name: "1 KB data",
        size: 1024,
    },
    RentEntry {
        name: "10 KB data",
        size: 10240,
    },
];

pub async fn run(rpc_url: &str, size: Option<usize>, json: bool) -> Result<()> {
    // 1. fetch reference rent (cost/byte)
    let rent_per_byte = tokio::task::spawn_blocking({
        let c = crate::rpc::client(rpc_url);
        move || -> anyhow::Result<f64> {
            let r1 = c.get_minimum_balance_for_rent_exemption(0)?;
            let r2 = c.get_minimum_balance_for_rent_exemption(1000)?;
            Ok(calculate_rent_per_byte(r1, r2, 1000))
        }
    })
    .await??;

    let sol_usd = price::sol_price().await.unwrap_or(0.0);

    // 2. if specific size requested
    if let Some(s) = size {
        let lamports = tokio::task::spawn_blocking({
            let c = crate::rpc::client(rpc_url);
            move || c.get_minimum_balance_for_rent_exemption(s)
        })
        .await??;

        let sol = utils::lamports_to_sol(lamports);

        if json {
            println!(
                "{}",
                serde_json::json!({
                    "size_bytes": s,
                    "lamports": lamports,
                    "sol": sol,
                    "usd": sol * sol_usd,
                })
            );
        } else {
            println!(
                "\n  Rent-exempt minimum for {} bytes: {} {}\n",
                s.to_string().cyan(),
                utils::format_sol(sol).green().bold(),
                if sol_usd > 0.0 {
                    format!("(≈ {})", utils::format_usd(sol * sol_usd))
                        .dimmed()
                        .to_string()
                } else {
                    "".into()
                }
            );
        }
        return Ok(());
    }

    // 3. show table
    if json {
        let mut entries = Vec::new();
        for e in COMMON_ACCOUNTS {
            let lamports = tokio::task::spawn_blocking({
                let c = crate::rpc::client(rpc_url);
                let s = e.size;
                move || c.get_minimum_balance_for_rent_exemption(s)
            })
            .await??;

            let sol = utils::lamports_to_sol(lamports);
            entries.push(serde_json::json!({
                "name": e.name,
                "size": e.size,
                "lamports": lamports,
                "sol": sol,
                "usd": sol * sol_usd,
            }));
        }
        println!(
            "{}",
            serde_json::json!({
                "rent_per_byte": rent_per_byte,
                "sol_price": sol_usd,
                "accounts": entries,
            })
        );
        return Ok(());
    }

    println!(
        "\n{} Solana Rent-Exempt Minimums {}\n",
        "🏦".bold(),
        if sol_usd > 0.0 {
            format!("(SOL = {})", utils::format_usd(sol_usd))
                .dimmed()
                .to_string()
        } else {
            "".into()
        }
    );

    println!(
        "  {:<24} {:>8} {:>14} {:>10}",
        "Account Type".white().bold(),
        "Bytes".white().bold(),
        "Rent (SOL)".white().bold(),
        "USD".white().bold()
    );
    println!("  {}", "─".repeat(60).dimmed());

    for e in COMMON_ACCOUNTS {
        let lamports = tokio::task::spawn_blocking({
            let c = crate::rpc::client(rpc_url);
            let s = e.size;
            move || c.get_minimum_balance_for_rent_exemption(s)
        })
        .await??;

        let sol = utils::lamports_to_sol(lamports);

        println!(
            "  {:<24} {:>8} {:>14} {:>10}",
            e.name.white(),
            e.size.to_string().dimmed(),
            format!("{sol:.6}").green(),
            if sol_usd > 0.0 {
                utils::format_usd(sol * sol_usd)
            } else {
                "—".into()
            }
            .dimmed(),
        );
    }

    println!(
        "\n  {} Cost per byte: {:.6} lamports\n",
        "ℹ".dimmed(),
        rent_per_byte
    );

    Ok(())
}

fn calculate_rent_per_byte(base_rent: u64, rent_for_bytes: u64, bytes: u64) -> f64 {
    if bytes == 0 {
        return 0.0;
    }
    (rent_for_bytes - base_rent) as f64 / bytes as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_rent_per_byte() {
        // Mock values: base rent = 890880, rent for 1000 bytes = 7850880
        // diff = 6960000. / 1000 = 6960.0
        let r = calculate_rent_per_byte(890_880, 7_850_880, 1000);
        assert_eq!(r, 6960.0);
    }

    #[test]
    fn test_calculate_rent_zero_bytes() {
        let r = calculate_rent_per_byte(100, 100, 0);
        assert_eq!(r, 0.0);
    }
}
