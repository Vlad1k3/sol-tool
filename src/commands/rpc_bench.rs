use anyhow::Result;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::{Duration, Instant};

use crate::rpc;

struct BenchResult {
    name: String,
    url: String,
    latencies: Vec<u128>,
    errors: usize,
}

impl BenchResult {
    fn avg(&self) -> f64 {
        if self.latencies.is_empty() {
            return f64::MAX;
        }
        self.latencies.iter().sum::<u128>() as f64 / self.latencies.len() as f64
    }

    fn min(&self) -> u128 {
        self.latencies.iter().copied().min().unwrap_or(0)
    }
    fn max(&self) -> u128 {
        self.latencies.iter().copied().max().unwrap_or(0)
    }
    fn p50(&self) -> u128 {
        percentile(&self.latencies, 50)
    }
    fn p99(&self) -> u128 {
        percentile(&self.latencies, 99)
    }

    fn success_rate(&self) -> f64 {
        let total = self.latencies.len() + self.errors;
        if total == 0 {
            return 0.0;
        }
        self.latencies.len() as f64 / total as f64 * 100.0
    }
}

fn percentile(sorted: &[u128], pct: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = (pct as f64 / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

pub async fn run(rpc_url: &str, extra_rpcs: Option<&str>, count: usize, json: bool) -> Result<()> {
    let count = count.clamp(3, 100);

    // Build endpoint list - start with configured RPC
    let mut endpoints: Vec<(&str, String)> = vec![("Configured RPC", rpc_url.to_string())];

    // Add extra RPCs if provided
    if let Some(extra) = extra_rpcs {
        for (i, url) in extra.split(',').enumerate() {
            let url = url.trim();
            if !url.is_empty() {
                endpoints.push((
                    Box::leak(format!("Extra #{}", i + 1).into_boxed_str()),
                    url.to_string(),
                ));
            }
        }
    }

    if !json {
        println!(
            "\n{} Benchmarking {} endpoint(s) × {} requests…\n",
            "🏎️".bold(),
            endpoints.len().to_string().cyan(),
            count.to_string().cyan()
        );
    }

    let pb = if !json {
        let pb = ProgressBar::new((endpoints.len() * count) as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("  {spinner:.green} [{bar:30.cyan/blue}] {pos}/{len} ({msg})")
                .unwrap()
                .progress_chars("█▓░"),
        );
        Some(pb)
    } else {
        None
    };

    let mut results: Vec<BenchResult> = Vec::new();

    for (name, url) in &endpoints {
        if let Some(ref pb) = pb {
            pb.set_message(name.to_string());
        }

        let mut latencies = Vec::with_capacity(count);
        let mut errors = 0usize;

        for _ in 0..count {
            let client = rpc::client_with_timeout(url, Duration::from_secs(10));
            let start = Instant::now();
            let res = tokio::task::spawn_blocking(move || client.get_slot()).await?;
            match res {
                Ok(_) => latencies.push(start.elapsed().as_millis()),
                Err(_) => errors += 1,
            }
            if let Some(ref pb) = pb {
                pb.inc(1);
            }
        }

        latencies.sort();
        results.push(BenchResult {
            name: name.to_string(),
            url: url.clone(),
            latencies,
            errors,
        });
    }

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    // sort best to worst
    results.sort_by(|a, b| a.avg().partial_cmp(&b.avg()).unwrap());

    if json {
        let data: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "url": r.url,
                    "avg_ms": r.avg().round(),
                    "min_ms": r.min(),
                    "max_ms": r.max(),
                    "p50_ms": r.p50(),
                    "p99_ms": r.p99(),
                    "success_rate": r.success_rate(),
                    "errors": r.errors,
                })
            })
            .collect();

        println!("{}", serde_json::json!({ "results": data, "count": count }));
        return Ok(());
    }

    // output table
    println!(
        "  {:<16} {:>7} {:>7} {:>7} {:>7} {:>7} {:>8}",
        "Endpoint".white().bold(),
        "Avg".white().bold(),
        "Min".white().bold(),
        "P50".white().bold(),
        "P99".white().bold(),
        "Max".white().bold(),
        "Success".white().bold()
    );
    println!("  {}", "─".repeat(72).dimmed());

    for (i, r) in results.iter().enumerate() {
        let rank = match i {
            0 => "🥇".to_string(),
            1 => "🥈".to_string(),
            2 => "🥉".to_string(),
            _ => format!("#{}", i + 1),
        };

        let avg = r.avg();
        let avg_str = format!("{:.0}ms", avg);
        let avg_col = if avg < 200.0 {
            avg_str.green()
        } else if avg < 500.0 {
            avg_str.yellow()
        } else {
            avg_str.red()
        };

        let succ = r.success_rate();
        let succ_str = format!("{:.0}%", succ);
        let succ_col = if succ >= 99.0 {
            succ_str.green()
        } else if succ >= 90.0 {
            succ_str.yellow()
        } else {
            succ_str.red()
        };

        println!(
            "  {:<16} {:>7} {:>7} {:>7} {:>7} {:>7} {:>8}",
            format!("{rank} {}", r.name).white(),
            avg_col,
            format!("{}ms", r.min()).dimmed(),
            format!("{}ms", r.p50()).dimmed(),
            format!("{}ms", r.p99()).dimmed(),
            format!("{}ms", r.max()).dimmed(),
            succ_col,
        );
    }

    println!();
    if let Some(best) = results.first() {
        println!(
            "  {} Fastest: {} ({:.0}ms avg)\n",
            "⚡".green(),
            best.name.green().bold(),
            best.avg()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bench_result_stats() {
        let res = BenchResult {
            name: "Test".to_string(),
            url: "http://localhost".to_string(),
            latencies: vec![10, 20, 30, 40, 50],
            errors: 0,
        };

        assert_eq!(res.min(), 10);
        assert_eq!(res.max(), 50);
        assert_eq!(res.avg(), 30.0);
        assert_eq!(res.p50(), 30); // 5 elements, index 2 (0,1,2,3,4) = 30

        // p99: 99% of 4 = 3.96 -> round to 4 -> index 4 = 50
        assert_eq!(res.p99(), 50);
    }

    #[test]
    fn test_bench_result_empty() {
        let res = BenchResult {
            name: "Empty".to_string(),
            url: "http://localhost".to_string(),
            latencies: vec![],
            errors: 0,
        };

        assert_eq!(res.min(), 0);
        assert_eq!(res.max(), 0);
        assert_eq!(res.avg(), f64::MAX);
    }

    #[test]
    fn test_bench_result_success_rate() {
        let res = BenchResult {
            name: "Partial".to_string(),
            url: "http://localhost".to_string(),
            latencies: vec![10, 20],
            errors: 2,
        };
        // 2 success, 2 errors = 4 total. 50% success
        assert_eq!(res.success_rate(), 50.0);
    }

    #[test]
    fn test_percentile_calculation() {
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        // p50 of 10 items: 50% of 9 = 4.5 -> round to 5 -> index 5 = 6
        // Wait, percentile logic: (pct * (len-1)).round()
        // 0.5 * 9 = 4.5 -> 5. index 5 is 6.
        assert_eq!(percentile(&data, 50), 6);

        // p90: 0.9 * 9 = 8.1 -> 8. index 8 is 9.
        assert_eq!(percentile(&data, 90), 9);
    }
}
