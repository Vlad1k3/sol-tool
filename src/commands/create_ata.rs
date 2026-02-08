//! Create empty ATA for testing clean command

use anyhow::{Context, Result};
use colored::Colorize;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    transaction::Transaction,
};

#[allow(deprecated)]
use solana_sdk::system_program;

use crate::solanapay;
use crate::utils;

/// Associated Token Program ID
const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

/// Well-known token mints for testing (devnet/mainnet)
const TEST_MINTS: &[(&str, &str)] = &[
    ("USDC", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
    ("USDT", "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"),
    ("RAY", "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R"),
];

/// Get ATA address for wallet and mint
fn get_associated_token_address(wallet: &Pubkey, mint: &Pubkey) -> Pubkey {
    let ata_program: Pubkey = ASSOCIATED_TOKEN_PROGRAM_ID.parse().unwrap();
    let token_program = spl_token::id();
    let seeds = &[wallet.as_ref(), token_program.as_ref(), mint.as_ref()];
    Pubkey::find_program_address(seeds, &ata_program).0
}

/// Create ATA instruction
fn create_associated_token_account_instruction(
    payer: &Pubkey,
    wallet: &Pubkey,
    mint: &Pubkey,
) -> Instruction {
    let ata_program: Pubkey = ASSOCIATED_TOKEN_PROGRAM_ID.parse().unwrap();
    let ata = get_associated_token_address(wallet, mint);

    Instruction {
        program_id: ata_program,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(*wallet, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data: vec![], // Create instruction has no data
    }
}

pub async fn run(
    rpc_url: &str,
    wallet_str: Option<&str>,
    keypair_path: Option<&str>,
    mint_str: Option<&str>,
    connect: bool,
) -> Result<()> {
    // ── Connect Flow ────────────────────────────────────────────────────────
    let wallet = if connect && wallet_str.is_none() {
        // use shared logic
        crate::solanapay::connect_wallet().await?
    } else {
        let w = wallet_str.ok_or_else(|| anyhow::anyhow!("Wallet address required"))?;
        utils::parse_pubkey(w)?
    };

    let client = crate::rpc::client(rpc_url);

    // mint setup
    let mint: Pubkey = if let Some(m) = mint_str {
        utils::parse_pubkey(m)?
    } else {
        // USDC default
        TEST_MINTS[0].1.parse().unwrap()
    };

    let mint_name = TEST_MINTS
        .iter()
        .find(|(_, addr)| addr.parse::<Pubkey>().ok() == Some(mint))
        .map(|(name, _)| *name)
        .unwrap_or("Unknown");

    // derive ata
    let ata = get_associated_token_address(&wallet, &mint);

    // check exist
    if client.get_account(&ata).is_ok() {
        println!(
            "{}",
            format!("✓ ATA already exists: {}", utils::short_key(&ata)).yellow()
        );
        return Ok(());
    }

    println!(
        "📝 Creating ATA for {} ({})...",
        mint_name.cyan(),
        utils::short_key(&mint)
    );

    // ── Execution ───────────────────────────────────────────────────────────

    if connect {
        // SOLANA PAY MODE
        let ix = create_associated_token_account_instruction(&wallet, &wallet, &mint);

        let recent_hash = client.get_latest_blockhash()?;
        let mut tx = Transaction::new_with_payer(&[ix], Some(&wallet));
        tx.message.recent_blockhash = recent_hash;

        println!("\n{}", "📱 Preparing transaction...".cyan().bold());

        // upload to relay
        let solana_pay_url = solanapay::upload_transactions(
            solanapay::DEFAULT_RELAY_URL,
            &[tx],
            &wallet,
            &format!("Create {} ATA", mint_name),
        )
        .await?;

        println!("{}", "✓ Uploaded successfully".green());
        solanapay::display_qr(&solana_pay_url)?;

        println!(
            "\n{}",
            "Scan QR with your wallet to sign and send.".dimmed()
        );
        println!(
            "   ATA will be created at: {}",
            utils::short_key(&ata).cyan()
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

    let ix = create_associated_token_account_instruction(&keypair.pubkey(), &wallet, &mint);
    let lh = client.get_latest_blockhash()?;

    let tx = Transaction::new_signed_with_payer(&[ix], Some(&keypair.pubkey()), &[&keypair], lh);

    let sig = client
        .send_and_confirm_transaction(&tx)
        .context("Failed to create ATA")?;

    println!("{}", "✅ ATA created successfully!".green());
    println!("   Address: {}", utils::short_key(&ata).cyan());
    println!("   Signature: {}", sig.to_string().dimmed());
    println!();
    println!(
        "{}",
        "Now you can test: sol-tool clean <WALLET> --connect".dimmed()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;
    use std::str::FromStr;

    const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

    #[test]
    fn test_get_associated_token_address() {
        // Known wallet from wallets.csv
        let wallet = Pubkey::from_str("9sRRkYzseywA5zjLd2tqZLAgNgK6X4MVbagrNTmM8jAw").unwrap();
        let mint = Pubkey::from_str(USDC_MINT).unwrap();

        let ata = get_associated_token_address(&wallet, &mint);

        // ATA should be a valid pubkey (not the wallet or mint)
        assert_ne!(ata, wallet);
        assert_ne!(ata, mint);

        // ATA derivation should be deterministic
        let ata2 = get_associated_token_address(&wallet, &mint);
        assert_eq!(ata, ata2);
    }

    #[test]
    fn test_ata_differs_by_wallet() {
        let wallet1 = Pubkey::from_str("9sRRkYzseywA5zjLd2tqZLAgNgK6X4MVbagrNTmM8jAw").unwrap();
        let wallet2 = Pubkey::from_str("CiK1qipeLb4PuTbSUHLAocYqiSwR5TXPgWmBurFwzQFG").unwrap();
        let mint = Pubkey::from_str(USDC_MINT).unwrap();

        let ata1 = get_associated_token_address(&wallet1, &mint);
        let ata2 = get_associated_token_address(&wallet2, &mint);

        assert_ne!(ata1, ata2);
    }

    #[test]
    fn test_create_ata_instruction_structure() {
        let payer = Pubkey::new_unique();
        let wallet = Pubkey::new_unique();
        let mint = Pubkey::from_str(USDC_MINT).unwrap();

        let ix = create_associated_token_account_instruction(&payer, &wallet, &mint);

        // Verify program ID
        assert_eq!(ix.program_id.to_string(), ASSOCIATED_TOKEN_PROGRAM_ID);

        // Verify account count (6 accounts)
        assert_eq!(ix.accounts.len(), 6);

        // First account is payer (signer)
        assert!(ix.accounts[0].is_signer);
        assert!(ix.accounts[0].is_writable);
        assert_eq!(ix.accounts[0].pubkey, payer);

        // Third account is wallet (not signer)
        assert!(!ix.accounts[2].is_signer);
        assert_eq!(ix.accounts[2].pubkey, wallet);

        // Fourth account is mint
        assert_eq!(ix.accounts[3].pubkey, mint);

        // Data should be empty for create instruction
        assert!(ix.data.is_empty());
    }

    #[test]
    fn test_test_mints_are_valid() {
        for (name, addr) in TEST_MINTS {
            let pk = Pubkey::from_str(addr);
            assert!(pk.is_ok(), "Invalid mint address for {}: {}", name, addr);
        }
    }
}
