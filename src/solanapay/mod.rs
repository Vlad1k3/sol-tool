use anyhow::{Context, Result};
use colored::Colorize;
use solana_sdk::pubkey::Pubkey;

mod relay;

pub use relay::{
    create_connect_session, display_qr, poll_session, session_to_solana_pay_url,
    upload_transactions, DEFAULT_RELAY_URL,
};

/// connect wallet flow (qr code)
/// returns wallet pubkey
pub async fn connect_wallet() -> Result<Pubkey> {
    use std::io::Write;

    // nice UI
    println!("\n{}", "📱 Connect your wallet via QR".cyan().bold());
    println!(
        "{}",
        "Scan with Phantom, Solflare, or Trust Wallet".dimmed()
    );

    // create session
    let session_id = create_connect_session(DEFAULT_RELAY_URL, "sol-tool connect").await?;

    // show qr
    let url = session_to_solana_pay_url(DEFAULT_RELAY_URL, &session_id);
    display_qr(&url)?;

    println!("\n{}", "⏳ Waiting for wallet connection...".yellow());

    // poll loop
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let poll = poll_session(DEFAULT_RELAY_URL, &session_id).await?;

        if poll.connected {
            if let Some(w) = poll.wallet {
                // helper for short string
                let short = if w.len() > 8 {
                    format!("{}…{}", &w[..4], &w[w.len() - 4..])
                } else {
                    w.clone()
                };

                println!("{} Wallet connected: {}", "✓".green(), short);

                return w.parse().context("Invalid wallet address from relay");
            }
        }

        // dot progress
        print!(".");
        std::io::stdout().flush().ok();
    }
}
