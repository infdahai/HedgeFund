# Opinion HFT - Quick Start Guide

## Prerequisites

- Rust 1.75+
- Valid Opinion.trade API key
- BNB Chain RPC URL (or use public)

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

# Environment
OPINION_ENV=local  # or aws

# Proxy (local only)
OPINION_PROXY_URL=http://127.0.0.1:7890

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

- Read [DESIGN.md](DESIGN.md) for architecture details
- Check [CLAUDE.md](../CLAUDE.md) for development guidelines
- Review existing tests for implementation patterns
