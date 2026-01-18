# Opinion HFT - LLM Development Context

## Project Overview
High-frequency trading bot for Opinion.trade prediction markets on BNB Chain (56).

## Quick Commands
```bash
cd opinion-hft
cargo check              # Type check
cargo clippy             # Lint (0 warnings required)
cargo test --lib         # Run 69 unit tests (no API required)
cargo test --tests       # Run ALL live integration tests (requires .env)
cargo test --test <name> # Run specific test file (e.g., live_account_tests)
cargo run                # Start bot
```

## Test Structure
| Test File | Tests | Description |
|-----------|-------|-------------|
| `live_extended_api_tests` | 3 | Historical data, latest price, order details |
| `small_scale_arbitrage_test` | 3 | Real-data arbitrage detection validation |
| `live_user_data_tests` | 3 | User positions, balances, and trades |
| `live_batch_operations` | 2 | Batch place/cancel order operations |
| `live_account_tests` | 6 | User auth, proxy address, balance checks |
| `live_market_data` | 6 | Markets, orderbook, quote tokens |
| `live_order_tests` | 1 | Full order lifecycle (place→query→cancel) |
| `live_trade_integration` | 2 | Complete trading workflow |
| `live_websocket_tests` | 4 | WebSocket connection, subscriptions |
| `trading_scenarios` | 8 | EIP-712 signing, market scenarios |
| `dual_mode_client` | 3 | Environment switching (local/aws) |

**Note**: All integration tests require valid `.env` configuration with API credentials.

## API Quirks (CRITICAL)
| Issue | Correct | Wrong |
|-------|---------|-------|
| Auth Header | `apikey: <key>` | `Authorization: Bearer` |
| Quote Token Endpoint | `/quoteToken?chainId=56` | `/quoteToken/56` |
| Order Endpoint | `/order` (singular) | `/orders` (plural) |
| Cancel Order | `POST /order/cancel` + body | `DELETE /order/{id}` |
| Side Values | `0` (Buy) / `1` (Sell) as strings | "BUY"/"SELL" |
| Price Precision | 3 decimals (0.001-0.999) | 2 decimals |
| Minimum Order | 1.30 USDT | - |
| Market Detail | `/market/{id}` path param, `data` wrapper | `/market/detail?marketId={id}` |

## TLS Fingerprinting & HTTP Client
Kong API Gateway uses JA3 TLS fingerprinting. The bot uses **pure rquest** with Chrome120 TLS emulation.

**Environments:**
- **Local dev**: `OPINION_ENV=local` + `OPINION_PROXY_URL=http://127.0.0.1:7890` (rquest with proxy)
- **AWS Tokyo**: `OPINION_ENV=aws` or unset (rquest direct connection, no proxy)
- **Default**: AWS (production-safe)

**Configuration:**
- `src/config.rs::Environment` enum defines environments
- `src/data/rest/unified_client.rs` implements pure rquest client
- Local: Routes through proxy to bypass Kong's TLS fingerprinting
- AWS: Direct connection (no TLS blocking in cloud environment)
- Chrome120 TLS emulation applied in both environments

**Performance:**
- No subprocess overhead (unlike curl)
- Native async performance with tokio
- Connection pooling and HTTP/2 support

See [Architecture Design](./opinion-hft/docs/DESIGN.md) for detailed system diagrams.

## ChainClient RPC Retry
`ChainClient` automatically retries failed RPC calls (up to 3 attempts) with exponential backoff.
**Endpoints with retry**: `balance_of()`, `balance_of_batch()`
**Handled errors**: timeout, connection errors, HTTP 503, HTTP 429

## Key Addresses
| Item | Address |
|------|---------|
| Chain ID | 56 (BNB Chain) |
| Quote Token (USDT) | `0x55d398326f99059fF775485246999027B3197955` |
| ConditionalTokens | `0xAD1a38cEc043e70E83a3eC30443dB285ED10D774` |
| CTF Exchange | Fetch from `/quoteToken` endpoint |
| Proxy/Safe Address | Fetch from `/user/auth` → `walletUsers["56"]` |

## Order Signing (EIP-712)
- **Maker**: Proxy/Safe address (not EOA)
- **Signer**: EOA address
- **SignatureType**: `2` (PolyGnosisSafe)
- **Domain Name**: `"OPINION CTF Exchange"` (note the space!)
- **Amounts**: Wei strings (18 decimals)

## Opinion Clob SDK Source Code
- **Location**: .venv/lib/python3.12/site-packages/opinion_clob_sdk

## Confirmation Flow
1. Submit order → REST API
2. Wait for WebSocket `status="Finish"` (DO NOT trust quantity data)
3. Query on-chain `balanceOf()` for actual fill
4. Latency: 3-5s (chain confirmation unavoidable)

## Risk Management
- Pre-trade: Position limits, exposure limits, profit threshold
- **Post-trade validation**: Slippage check, P&L tracking, execution time validation
- Circuit breaker: Halts after 3 consecutive losses or daily loss limit
- Drawdown protection: Stops if drawdown > 10%

## Development Log
| Date | Change |
|------|--------|
| 2026-01-18 | Completed API test coverage (latest_price, price_history, order_by_id) and enhanced WebSocket tests (reconnection, lifecycle) |
| 2026-01-18 | Added small-scale arbitrage detection integration tests with real market data |
| 2026-01-18 | Comprehensive documentation update: created QUICKSTART.md, updated DESIGN.md with Strategy, Telemetry, and Address Config sections |
| 2026-01-18 | Added comprehensive API test coverage: User Data (Positions, Balances, Trades) and Batch Operations (Place/Cancel) |
| 2026-01-18 | Fixed API parameter issues: `chain_id` snake_case fix, handled null lists in responses |
| 2026-01-18 | Updated test documentation and fixed CLAUDE.md commands |
| 2026-01-18 | API Compatibility Fixes: tradingMethod type (i32), added sign/safeRate/orderExpTime fields, fixed EIP-712 VerifyingContract to use CTF Exchange address, Fraction-based amount calculation, Serde camelCase verified, cancel order orderId confirmed |
| 2026-01-18 | Implemented post-trade validation in RiskManager (slippage, execution time, P&L tracking, circuit breaker) |
| 2026-01-18 | Fixed market detail API: use path param `/market/{id}` instead of query param, response has `data` wrapper |
| 2026-01-18 | Removed all curl subprocess, implemented pure rrequest client with environment detection (local=proxy, aws=direct), ChainClient retry mechanism |
| 2026-01-18 | Extended curl to ALL endpoints (temporary workaround for 403 Forbidden errors) |
| 2026-01-17 | OpinionClient caching, RiskManager enhanced |
| 2026-01-17 | Arbitrage rollback logic implemented |
| 2026-01-17 | 0 warnings achieved |

## Environment Variables
```bash
# API Configuration
OPINION_API_KEY=xxx
OPINION_REST_URL=https://proxy.opinion.trade:8443/openapi/
OPINION_WS_URL=wss://ws.opinion.trade

# Environment Selection
OPINION_ENV=local  # local or aws (defaults to aws)

# Proxy Configuration (local only)
OPINION_PROXY_URL=http://127.0.0.1:7890  # FlClash proxy port

# Blockchain
BSC_RPC_URL=https://bsc-dataseed.binance.org
PRIVATE_KEY=0x...
OPINION_EOA_ADDRESS=0x...
```
