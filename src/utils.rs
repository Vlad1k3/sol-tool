use anyhow::{Context, Result};
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
};
use std::{path::PathBuf, str::FromStr};

pub fn parse_pubkey(s: &str) -> Result<Pubkey> {
    Pubkey::from_str(s).context(format!("Invalid pubkey: {s}"))
}

pub fn parse_signature(s: &str) -> Result<Signature> {
    Signature::from_str(s).context(format!("Invalid signature: {s}"))
}

// simple keypair loader
pub fn load_keypair(path: Option<&str>) -> Result<Keypair> {
    let p = match path {
        Some(p) => PathBuf::from(p),
        None => {
            let home = std::env::var("HOME").context("HOME not set")?;
            PathBuf::from(home).join(".config/solana/id.json")
        }
    };

    let data =
        std::fs::read_to_string(&p).context(format!("Can't read keypair: {}", p.display()))?;

    // try to parse json
    if let Ok(bytes) = serde_json::from_str::<Vec<u8>>(&data) {
        return Keypair::try_from(&bytes[..])
            .map_err(|e| anyhow::anyhow!("Invalid keypair bytes: {}", e));
    }

    // maybe it's raw bytes?
    anyhow::bail!("Invalid keypair file format");
}

pub fn verify_keypair(keypair: &Keypair, wallet: &Pubkey) -> Result<()> {
    if keypair.pubkey() != *wallet {
        anyhow::bail!(
            "Keypair matches {} but expected {}",
            short_key(&keypair.pubkey()),
            short_key(wallet)
        );
    }
    Ok(())
}

pub fn lamports_to_sol(l: u64) -> f64 {
    l as f64 / 1_000_000_000.0
}

pub fn format_sol(sol: f64) -> String {
    if sol == 0.0 {
        return "0 SOL".into();
    }
    if sol < 0.001 {
        return format!("{sol:.9} SOL");
    }
    if sol < 1.0 {
        return format!("{sol:.6} SOL");
    }
    format!("{sol:.4} SOL")
}

pub fn format_usd(usd: f64) -> String {
    if usd == 0.0 {
        return "$0.00".to_string();
    }
    if usd < 0.01 {
        return format!("${usd:.6}");
    }
    if usd < 1.0 {
        return format!("${usd:.4}");
    }
    if usd < 1000.0 {
        return format!("${usd:.2}");
    }

    // add commas
    let s = format!("{:.2}", usd);
    let parts: Vec<&str> = s.split('.').collect();
    let int_part = parts[0];
    let frac = parts[1];

    let mut result = String::new();
    let mut count = 0;
    for c in int_part.chars().rev() {
        if count > 0 && count % 3 == 0 {
            result.push(',');
        }
        result.push(c);
        count += 1;
    }
    format!("${}.{}", result.chars().rev().collect::<String>(), frac)
}

pub fn short_key(pk: &Pubkey) -> String {
    let s = pk.to_string();
    format!("{}…{}", &s[..4], &s[s.len() - 4..])
}

pub fn short_sig(sig: &Signature) -> String {
    let s = sig.to_string();
    if s.len() > 16 {
        format!("{}…{}", &s[..8], &s[s.len() - 8..])
    } else {
        s
    }
}

pub fn token_amount(raw: u64, decimals: u8) -> f64 {
    raw as f64 / 10f64.powi(decimals as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;
    use std::str::FromStr;

    #[test]
    fn test_format_sol() {
        assert_eq!(format_sol(0.0), "0 SOL");
        assert_eq!(format_sol(1.5), "1.5000 SOL");
        assert_eq!(format_sol(0.000000001), "0.000000001 SOL");
    }

    #[test]
    fn test_format_usd() {
        assert_eq!(format_usd(0.0), "$0.00");
        assert_eq!(format_usd(0.005), "$0.005000");
        assert_eq!(format_usd(10.50), "$10.50");
        assert_eq!(format_usd(1234.56), "$1,234.56");
    }

    #[test]
    fn test_short_key() {
        let pk = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap();
        assert_eq!(short_key(&pk), "Toke…Q5DA");
    }

    #[test]
    fn test_lamports_to_sol() {
        assert_eq!(lamports_to_sol(1_000_000_000), 1.0);
        assert_eq!(lamports_to_sol(500_000_000), 0.5);
    }

    #[test]
    fn test_token_amount() {
        assert_eq!(token_amount(1000, 2), 10.0); // 10.00
        assert_eq!(token_amount(123456, 6), 0.123456); // USDC
    }

    #[test]
    fn test_verify_keypair_success() {
        let kp = Keypair::new();
        let pk = kp.pubkey();
        assert!(verify_keypair(&kp, &pk).is_ok());
    }

    #[test]
    fn test_verify_keypair_mismatch() {
        let kp = Keypair::new();
        let other_pk = Pubkey::new_unique();
        assert!(verify_keypair(&kp, &other_pk).is_err());
    }

    #[test]
    fn test_load_keypair_from_file() {
        use std::io::Write;

        // Create temp keypair file
        let kp = Keypair::new();
        let bytes: Vec<u8> = kp.to_bytes().to_vec();
        let json = serde_json::to_string(&bytes).unwrap();

        let tmp = std::env::temp_dir().join("test_keypair.json");
        let mut file = std::fs::File::create(&tmp).unwrap();
        file.write_all(json.as_bytes()).unwrap();

        let loaded = load_keypair(Some(tmp.to_str().unwrap())).unwrap();
        assert_eq!(loaded.pubkey(), kp.pubkey());

        // Cleanup
        std::fs::remove_file(tmp).ok();
    }

    #[test]
    fn test_load_keypair_invalid_file() {
        let result = load_keypair(Some("/nonexistent/path.json"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_pubkey() {
        let valid = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
        assert!(parse_pubkey(valid).is_ok());

        let invalid = "not-a-pubkey";
        assert!(parse_pubkey(invalid).is_err());
    }

    #[test]
    fn test_parse_signature() {
        // Valid signature (88 chars base58)
        let valid = "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW";
        assert!(parse_signature(valid).is_ok());

        let invalid = "not-a-signature";
        assert!(parse_signature(invalid).is_err());
    }
}
