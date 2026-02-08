mod commands;
mod price;
mod rpc;
mod solanapay;
mod utils;

use clap::{Parser, Subcommand};
use colored::Colorize;
use std::env;

#[derive(Parser)]
#[command(
    name = "sol-tool",
    version,
    about = format!("{}\n{}", "⚡ sol-tool".bold().cyan(), "Swiss Army Knife for Solana".dimmed()),
    after_help = "
Examples:
  sol-tool clean <WALLET>              Close empty accounts
  sol-tool portfolio <WALLET>          Show token values
  sol-tool scan <WALLET>               Health check
  sol-tool rpc-bench                   Test RPC speed
"
)]
struct App {
    #[command(subcommand)]
    cmd: Commands,

    /// Custom RPC URL
    #[arg(long, global = true, env = "SOLANA_RPC_NODE")]
    rpc: Option<String>,

    /// JSON output
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// 🧹 Close empty accounts & reclaim rent
    Clean {
        wallet: Option<String>,
        #[arg(short, long)]
        keypair: Option<String>,
        #[arg(short, long)]
        file: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value_t = 10)]
        batch: usize,
        #[arg(long)]
        dust: Option<f64>,
        #[arg(long)]
        connect: bool,
    },

    /// 💰 Token portfolio & prices
    Portfolio {
        wallet: String,
        #[arg(long, default_value_t = 0.01)]
        min_usd: f64,
        #[arg(long, default_value = "value")]
        sort: String,
    },

    /// 🔍 Wallet health check
    Scan { wallet: String },

    /// 🏎️ RPC benchmark
    RpcBench {
        #[arg(long)]
        extra: Option<String>,
        #[arg(long, default_value_t = 10)]
        count: usize,
    },

    /// 📡 Live tx monitor
    Monitor {
        wallet: String,
        #[arg(long, default_value_t = 3)]
        interval: u64,
    },

    /// 🏦 Rent exemption table
    Rent {
        #[arg(long)]
        size: Option<usize>,
    },

    /// 🧪 Create ATA (test util)
    CreateAta {
        #[arg(required_unless_present = "connect")]
        wallet: Option<String>,
        #[arg(short, long)]
        keypair: Option<String>,
        #[arg(long, short)]
        mint: Option<String>,
        #[arg(long)]
        connect: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // try load .env
    let _ = dotenvy::dotenv();

    let app = App::parse();

    let rpc_url = app.rpc.unwrap_or_else(|| {
        env::var("SOLANA_RPC_NODE").unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".into())
    });

    match app.cmd {
        Commands::Clean {
            wallet,
            keypair,
            file,
            dry_run,
            batch,
            dust,
            connect,
        } => {
            commands::clean::run(
                &rpc_url,
                wallet.as_deref(),
                keypair.as_deref(),
                file.as_deref(),
                dry_run,
                batch.clamp(1, 20),
                dust,
                connect,
                app.json,
            )
            .await
        }
        Commands::Portfolio {
            wallet,
            min_usd,
            sort,
        } => commands::portfolio::run(&rpc_url, &wallet, min_usd, &sort, app.json).await,
        Commands::Scan { wallet } => commands::scan::run(&rpc_url, &wallet, app.json).await,
        Commands::RpcBench { extra, count } => {
            commands::rpc_bench::run(&rpc_url, extra.as_deref(), count, app.json).await
        }
        Commands::Monitor { wallet, interval } => {
            commands::monitor::run(&rpc_url, &wallet, interval).await
        }
        Commands::Rent { size } => commands::rent::run(&rpc_url, size, app.json).await,
        Commands::CreateAta {
            wallet,
            keypair,
            mint,
            connect,
        } => {
            commands::create_ata::run(
                &rpc_url,
                wallet.as_deref(),
                keypair.as_deref(),
                mint.as_deref(),
                connect,
            )
            .await
        }
    }
}
