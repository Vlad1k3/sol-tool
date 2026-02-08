//! Netlify relay client for Solana Pay transactions

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use solana_sdk::{pubkey::Pubkey, transaction::Transaction};

/// Default relay URL
pub const DEFAULT_RELAY_URL: &str = "https://unrivaled-torte-81e36b.netlify.app";

#[derive(Serialize)]
struct UploadRequest {
    transactions: Vec<String>,
    wallet: String,
    label: String,
}

#[derive(Serialize)]
struct ConnectRequest {
    mode: String,
    label: String,
}

#[derive(Deserialize)]
struct SessionResponse {
    id: String,
}

#[derive(Deserialize, Debug)]
pub struct PollResponse {
    pub connected: bool,
    pub wallet: Option<String>,
}

/// Create a connect session (no transactions yet)
/// Returns session ID
pub async fn create_connect_session(relay_url: &str, label: &str) -> Result<String> {
    let request = ConnectRequest {
        mode: "connect".to_string(),
        label: label.to_string(),
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/.netlify/functions/tx", relay_url))
        .json(&request)
        .send()
        .await
        .context("Failed to create session")?;

    if !resp.status().is_success() {
        let error = resp.text().await.unwrap_or_default();
        anyhow::bail!("Relay error: {}", error);
    }

    let session: SessionResponse = resp
        .json()
        .await
        .context("Failed to parse session response")?;

    Ok(session.id)
}

/// Poll session for wallet connection
pub async fn poll_session(relay_url: &str, session_id: &str) -> Result<PollResponse> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{}/.netlify/functions/tx?id={}&poll=true",
            relay_url, session_id
        ))
        .send()
        .await
        .context("Failed to poll session")?;

    if !resp.status().is_success() {
        let error = resp.text().await.unwrap_or_default();
        anyhow::bail!("Poll error: {}", error);
    }

    resp.json().await.context("Failed to parse poll response")
}

/// Upload transactions to relay and return the Solana Pay URL
pub async fn upload_transactions(
    relay_url: &str,
    transactions: &[Transaction],
    wallet: &Pubkey,
    label: &str,
) -> Result<String> {
    let tx_base64: Vec<String> = transactions
        .iter()
        .map(|tx| {
            let bytes = bincode::serialize(tx).expect("serialize tx");
            STANDARD.encode(&bytes)
        })
        .collect();

    let request = UploadRequest {
        transactions: tx_base64,
        wallet: wallet.to_string(),
        label: label.to_string(),
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/.netlify/functions/tx", relay_url))
        .json(&request)
        .send()
        .await
        .context("Failed to upload to relay")?;

    if !resp.status().is_success() {
        let error = resp.text().await.unwrap_or_default();
        anyhow::bail!("Relay error: {}", error);
    }

    let upload_resp: SessionResponse = resp
        .json()
        .await
        .context("Failed to parse relay response")?;

    let function_url = format!("{}/.netlify/functions/tx?id={}", relay_url, upload_resp.id);
    let solana_pay_url = format!("solana:{}", urlencoding::encode(&function_url));

    Ok(solana_pay_url)
}

/// Build Solana Pay URL from session ID
pub fn session_to_solana_pay_url(relay_url: &str, session_id: &str) -> String {
    let function_url = format!("{}/.netlify/functions/tx?id={}", relay_url, session_id);
    format!("solana:{}", urlencoding::encode(&function_url))
}

/// Display QR code for Solana Pay URL
pub fn display_qr(solana_pay_url: &str) -> Result<()> {
    println!("\n{}", "📱 Scan this QR code with your wallet:".cyan());
    println!("{}", "(Phantom, Solflare, or Trust Wallet)".dimmed());
    println!();

    qr2term::print_qr(solana_pay_url).context("Failed to generate QR code")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_to_solana_pay_url() {
        let url = session_to_solana_pay_url("https://example.com", "session-123");
        assert!(url.starts_with("solana:"));
        assert!(url.contains("session-123"));
        assert!(url.contains("example.com"));
    }

    /// Integration test: Create a real connect session on the relay
    #[tokio::test]
    async fn test_create_connect_session_integration() {
        let result = create_connect_session(DEFAULT_RELAY_URL, "Test Session").await;

        assert!(result.is_ok(), "Failed to create session: {:?}", result);

        let session_id = result.unwrap();
        assert!(!session_id.is_empty(), "Session ID should not be empty");
    }

    /// Integration test: Poll a session (should not panic)
    #[tokio::test]
    async fn test_poll_session_integration() {
        // First create a session
        let session_id = create_connect_session(DEFAULT_RELAY_URL, "Poll Test")
            .await
            .unwrap();

        // Then poll it
        let result = poll_session(DEFAULT_RELAY_URL, &session_id).await;

        assert!(result.is_ok(), "Failed to poll session: {:?}", result);

        let poll_resp = result.unwrap();
        // Fresh session should not be connected yet
        assert!(!poll_resp.connected);
    }

    #[test]
    fn test_upload_request_serialization() {
        let tx = Transaction::default();
        let bytes = bincode::serialize(&tx).unwrap();
        let b64 = STANDARD.encode(&bytes);

        // Should be valid base64
        let decoded = STANDARD.decode(&b64);
        assert!(decoded.is_ok());
    }
}
