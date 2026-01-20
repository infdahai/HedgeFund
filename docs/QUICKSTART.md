# Opinion HFT - Quick Start Guide

## Prerequisites

- Rust 1.90+
- Valid Opinion.trade API key
- BNB Chain RPC URL (or use public)
- Optional: Python 3.12 + uv (for Parquet verification)

## Installation

```bash
cargo build --release
```

## Configuration

Create `.env` file in project root:

```bash
# API Configuration
OPINION_API_KEY=your_api_key_here
OPINION_REST_URL=https://proxy.opinion.trade:8443/openapi/
OPINION_WS_URL=wss://ws.opinion.trade
OPINION_MASTER_MARKET_URL=https://...  # optional master list URL (comma-separated supported)

# Environment
OPINION_ENV=local  # or aws
OPINION_SCAN_ONLY=true  # optional safety switch

# Proxy (local only)
OPINION_PROXY_URL=http://127.0.0.1:7890

# Data collection
OPINION_DATA_DB_PATH=data/opinion-hft.sqlite

# Logging
OPINION_LOG_FILE_ENABLED=true

# Blockchain
BSC_RPC_URL=https://bsc-dataseed.binance.org
PRIVATE_KEY=0x...
OPINION_EOA_ADDRESS=0x...
```

## Running Tests

### Unit Tests (No API Required)
```bash
cargo test --lib
```

### Integration Tests (Requires .env)
```bash
# All tests
cargo test --tests

# Specific test file
cargo test --test live_market_data
cargo test --test live_websocket_tests
cargo test --test live_trade_integration
```

## Running the Bot

```bash
cargo run --release
```

## Parquet Verification (Optional)

If `OPINION_PARQUET_ENABLED=true` is set and data has been recorded, validate one
Parquet file with the project venv to avoid system Python conflicts.

```bash
uv pip install polars
".venv/bin/python" - <<'PY'
import glob
import polars as pl

files = glob.glob("data/parquet/orderbook_snapshots_v1/date=*/hour=*/*.parquet")
if not files:
    raise SystemExit("No parquet files found under data/parquet/orderbook_snapshots_v1")

target = files[0]
print(f"Checking file: {target}")

df = pl.read_parquet(target, hive_partitioning=True)
print("\n--- Schema ---")
print(df.schema)

print("\n--- Data Preview ---")
print(df.select(["local_recv_ts_us", "market_id", "bids", "date", "hour"]).head(5))

bids_list = df["bids"].to_list()
first_bids = bids_list[0] if bids_list else []
print(f"\nFirst row bids count: {len(first_bids or [])}")
if first_bids:
    print(f"Top bid: {first_bids[0]}")
PY
```

DuckDB alternative: see `docs/verify_parquet.sql`.

## First Steps

1. **Test Configuration**
   ```bash
   cargo test --test live_account_tests
   ```

2. **Market Data**
   ```bash
   cargo test --test live_market_data
   ```

3. **Trade Integration**
   ```bash
   cargo test --test live_trade_integration
   ```

## Common Issues

### TLS Fingerprinting Errors
- **Local dev**: Ensure `OPINION_PROXY_URL` is set
- **AWS**: Use `OPINION_ENV=aws`

### Rate Limiting
- Bot automatically respects 15 TPS limit
- Burst capacity: 3 requests

### Order Confirmation
- Average latency: 3-5 seconds
- Confirmed via on-chain `balanceOf()` query
- WebSocket quantity data is unreliable

## Project Structure

```
├── src/
│   ├── auth/           # EIP-712 signing
│   ├── data/          # REST & WebSocket clients
│   ├── execution/     # Order gateway, rate limiter
│   ├── strategy/      # Arbitrage, market scanner
│   └── chain/         # BNB Chain interactions
├── tests/             # Integration tests
└── docs/              # Documentation
```

## Next Steps

- Read [architecture.md](architecture.md) for architecture details
- See [AWS_DEPLOYMENT.md](AWS_DEPLOYMENT.md) for systemd service setup
- Check [agents.md](../agents.md) for development guidelines
- Review existing tests for implementation patterns
