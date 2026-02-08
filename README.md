# ⚡ sol-tool — Swiss Army Knife for Solana

Blazing-fast CLI utility for **every Solana user** — traders, degens, hodlers, and developers.

> Close empty accounts. Track portfolio. Scan wallet health. Benchmark RPCs. Sign with mobile wallet.

---

## 🚀 Features

| Command | Description |
|---------|-------------|
| `clean` | Close empty token accounts, reclaim rent SOL |
| `portfolio` | Token balances with live USD prices (Jupiter API) |
| `scan` | Wallet health report: security, waste, delegate approvals |
| `rpc-bench` | Benchmark RPC endpoints, show latency/reliability |
| `monitor` | Real-time transaction feed for any wallet |
| `rent` | Rent-exempt minimums for all account types |
| `create-ata` | Create test ATA accounts (developer utility) |

### � Mobile Wallet Support

Sign transactions with your mobile wallet (Phantom, Solflare) via QR code:

```bash
sol-tool clean --connect       # No keypair needed!
sol-tool create-ata --connect  # Sign with phone
```

### 📁 Batch Processing

Process multiple wallets from CSV file:

```bash
sol-tool clean -f wallets.csv --dry-run
```

---

## 📦 Installation

```bash
# Install Rust (if not installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone and build
git clone https://github.com/YOUR_USERNAME/sol-tool.git
cd sol-tool
cargo build --release

# Binary at ./target/release/sol-tool
```

---

## 💡 Usage

### 🧹 Clean — Reclaim Rent SOL

```bash
# Preview (safe — no transactions)
sol-tool clean <WALLET_ADDRESS> --dry-run

# Execute with keypair
sol-tool clean <WALLET_ADDRESS> --keypair /path/to/keypair.json

# Sign with mobile wallet (QR code)
sol-tool clean <WALLET_ADDRESS> --connect
sol-tool clean --connect  # Wallet address from QR scan

# Batch mode (multiple wallets)
sol-tool clean -f wallets.csv

# Include dust accounts (<0.001 SOL value)
sol-tool clean <WALLET_ADDRESS> --dust 0.001

# Custom batch size (max 20)
sol-tool clean <WALLET_ADDRESS> --batch 15
```

**Safety:**
- `--dry-run` is read-only, no transactions sent
- Skips frozen accounts and accounts with active delegate approvals
- Shows USD value of reclaimable SOL
- Links to Solscan for every transaction

---

### 💰 Portfolio — Token Balances + Prices

```bash
sol-tool portfolio <WALLET_ADDRESS>
sol-tool portfolio <WALLET_ADDRESS> --min-usd 1.0   # Hide tiny positions
sol-tool portfolio <WALLET_ADDRESS> --sort balance  # Sort by balance
sol-tool portfolio <WALLET_ADDRESS> --json          # JSON output
```

Uses **Jupiter Price API v2** — free, no API key needed.

---

### 🔍 Scan — Wallet Health Report

```bash
sol-tool scan <WALLET_ADDRESS>
```

Checks: empty accounts, delegate approvals, frozen accounts, health score (0–100).

---

### 🏎️ RPC Bench — Find the Fastest Endpoint

```bash
sol-tool rpc-bench
sol-tool rpc-bench --extra "https://your-rpc.com"
sol-tool rpc-bench --count 50
```

---

### 📡 Monitor — Real-Time Transaction Feed

```bash
sol-tool monitor <WALLET_ADDRESS>
sol-tool monitor <WALLET_ADDRESS> --interval 1  # Faster polling
```

---

### 🏦 Rent — Reference Table

```bash
sol-tool rent
sol-tool rent --size 500
```

---

### 🧪 Create ATA — Test Utility

```bash
sol-tool create-ata <WALLET_ADDRESS>
sol-tool create-ata <WALLET_ADDRESS> --mint <MINT_ADDRESS>
sol-tool create-ata --connect  # Sign with mobile wallet
```

---

## ⚙️ Configuration

```bash
# Custom RPC (all commands)
sol-tool --rpc https://your-rpc.com <command>

# Environment variable
export SOLANA_RPC_NODE=https://mainnet.helius-rpc.com/?api-key=KEY

# Or use .env file
echo "SOLANA_RPC_NODE=https://your-rpc.com" > .env
```

---

## 🏗️ Architecture

```
src/
├── main.rs              CLI entry point (clap)
├── utils.rs             Pubkey parsing, formatting, keypair loading
├── rpc.rs               RPC client factory
├── price.rs             Jupiter Price API integration
├── solanapay/
│   ├── mod.rs           Solana Pay module exports
│   └── relay.rs         Netlify relay for mobile wallet signing
└── commands/
    ├── clean.rs         Close empty accounts, reclaim rent
    ├── portfolio.rs     Token balances + USD prices
    ├── scan.rs          Wallet health analysis
    ├── rpc_bench.rs     RPC endpoint benchmarking
    ├── monitor.rs       Real-time transaction feed
    ├── rent.rs          Rent-exempt reference table
    └── create_ata.rs    Create ATA test utility
```

---

## 🧪 Testing

```bash
cargo test
```

46 tests covering:
- Safety checks (frozen/delegated accounts)
- Keypair loading/verification
- ATA derivation
- Jupiter API integration
- Netlify Relay sessions
- CSV batch parsing

---

## 📄 License

MIT
