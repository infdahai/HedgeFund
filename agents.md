# Opinion HFT - LLM Development Context

## Project Overview
High-frequency trading bot for Opinion.trade prediction markets on BNB Chain (56).

## Quick Commands
```bash
cargo check              # Type check
cargo clippy             # Lint (0 warnings required)
cargo test --lib         # Run 69 unit tests (no API required)
cargo test --tests       # Run ALL live integration tests (requires .env)
cargo test --test <name> # Run specific test file (e.g., live_account_tests)
cargo run                # Start bot
cargo run --bin market_sweep  # One-off market metadata sweep (logs all markets)
cargo run --bin ws_probe      # One-off WS probe (subscribe + dump raw events)
```

**Formatting Standard**: See [docs/FORMAT.md](./docs/FORMAT.md) for single formatting standard (rustfmt + clippy rules).

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
| `live_rpc_failover` | 2 | RPC primary/fallback switching |

**Note**: All integration tests require valid `.env` configuration with API credentials.

## API Quirks (CRITICAL)
| Issue | Correct | Wrong |
|-------|---------|-------|
| Auth Header | `apikey: <key>` | `Authorization: Bearer` |
| Quote Token Endpoint | `/quoteToken?chainId=56` | `/quoteToken/56` |
| Order Endpoint | `/order` (singular) | `/orders` (plural) |
| Cancel Order | `POST /order/cancel` + body | `DELETE /order/{id}` |
| Side Values | `0` (Buy) / `1` (Sell) as strings | "BUY"/"SELL" |
| Price Precision | **0.1 - 0.9**, 1 decimal (frontend standard) | More than 1 decimal |
| Market Status Filter | `"activated"` or `"resolved"` (string) | Numeric values like "2" |
| Minimum Order | 1.30 USDT | - |
| Market Detail | `/market/{id}` path param, `data` wrapper | `/market/detail?marketId={id}` |
| Categorical Market Detail | `/market/categorical/{id}` | `/market/{id}` |

**WebSocket Notes**
- Subscribe payload uses `{"action":"subscribe","channel":"..."}` (not `type`).
- Server acks with `{"code":200,"message":"Message processed"}`.
- Orderbook channel uses `{"action":"SUBSCRIBE","channel":"market.depth.diff","marketId":<int>}` (diff stream).
- Diff payload fields: `msgType`, `marketId`, `outcome` or `outcomeSide`, `side`, `price`, `size` (single level update).
- `marketId` is the integer `topicId` from the master market list endpoint.
- Always route updates by outcome ("YES"/"NO"); do not derive NO from YES.

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

See [Architecture Design](./docs/architecture.md) for detailed system diagrams.

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

## Trading Fees (CRITICAL)

### Maker vs Taker Fees
- **Maker (限价单)**: **0% 手续费** ✅ - 提供流动性完全免费
- **Taker (市价单)**: **动态手续费 0% - 2%** - 成交时支付

### 费率计算公式
```
基础费率 = topic_rate × price × (1 - price)
有效费率 = 基础费率 × (1 - vip_discount) × (1 - referral_discount)
实际手续费 = max(交易金额 × 有效费率, $0.50)
```

### 费率示例 (topic_rate = 0.02)
| 价格 | 基础费率 | 说明 |
|------|---------|------|
| 0.1 | 0.18% | 低不确定性 |
| 0.5 | **0.50%** | 最高费率（最大不确定性） |
| 0.9 | 0.18% | 低不确定性 |

### 关键限制
- **最小订单金额**: $5.00 USDT
- **最小手续费**: $0.50 USDT (即使计算值更低)
- **最小提现金额**: $5.00 USDT

### 折扣系统
- VIP Program: 根据交易量级别折扣
- Referral Program: 邀请者获得被邀请者手续费的 5% 返还
- Promotions: 限时活动折扣
- **折扣可叠加使用**（乘法累计）

### 实际成本案例
**市价单 Taker 买入 $100 @ 0.5**:
- 计算费率: 0.02 × 0.5 × 0.5 = 0.005 (0.5%)
- 计算手续费: $100 × 0.5% = $0.50
- 实际收费: $0.50 (达到最低限制)

**小额订单 $10 @ 0.5**:
- 计算手续费: $10 × 0.5% = $0.05
- 实际收费: $0.50 (最低手续费底限)

**关键结论**：市价单成交至少损失 $0.50，小额订单更不划算。

### 代码实现
```rust
// src/strategy/arbitrage.rs:106-114
pub fn calculate_fee_rate(&self, price: f64) -> f64 {
    let base_fee = self.topic_fee_rate * price * (1.0 - price);
    base_fee * (1.0 - self.referral_discount)
}

// 默认配置
topic_fee_rate: 0.02,   // 2% 最大费率
referral_discount: 0.10, // 10% 邀请折扣
```

**官方文档**: https://docs.opinion.trade/trade-on-opinion.trade/fees

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
| 2026-01-20 | Added market pagination + cached refresh, categorical market detail support, arbitrage fee/notional fixes with scan diagnostics, and risk sizing uses total spend. |
| 2026-01-20 | Added master market list ingestion (topicId), WS `market.depth.diff` routing with outcome/outcomeSide, scan-only mode, local orderbook store, and sweep/probe tooling |
| 2026-01-20 | Implemented RPC failover: ChainClient now supports primary/fallback RPC endpoints with automatic health check and recovery. ConfirmationTracker batch optimization: 50ms buffer window for balanceOfBatch, reducing RPC calls by ~80%. New env vars: OPINION_RPC_URL_PRIMARY, OPINION_RPC_URL_FALLBACK, OPINION_BATCH_BUFFER_MS, OPINION_HEALTH_CHECK_INTERVAL |
| 2026-01-19 | Implemented daily DB rotation with date-based file naming (data/opinion-{YYYY-MM-DD}.sqlite) to prevent single DB from exceeding 5GB and avoid VACUUM issues |
| 2026-01-19 | Fixed market status filter: use `"activated"` (string) instead of `"2"` (numeric); Updated price precision: support 0.1-0.9 range with 1 decimal; Throttled RiskManager health check from 1s to 30s interval |
| 2026-01-19 | Implemented file logging system with daily rotation to logs/opinion-hft.log.YYYY-MM-DD, non-blocking I/O with WorkerGuard lifecycle management, environment-aware formatting (stdout vs file), test infrastructure updates with std::sync::Once pattern. Retained file+line metadata for production loss debugging (AWS environment priority over micro-optimization) |
| 2026-01-18 | Completed API test coverage (latest_price, price_history, order_by_id) and enhanced WebSocket tests (reconnection, lifecycle) |
| 2026-01-18 | Added small-scale arbitrage detection integration tests with real market data |
| 2026-01-18 | Comprehensive documentation update: created QUICKSTART.md, updated architecture.md with Strategy, Telemetry, and Address Config sections |
| 2026-01-18 | Added comprehensive API test coverage: User Data (Positions, Balances, Trades) and Batch Operations (Place/Cancel) |
| 2026-01-18 | Fixed API parameter issues: `chain_id` snake_case fix, handled null lists in responses |
| 2026-01-18 | Updated test documentation and fixed agents.md commands (formerly CLAUDE.md) |
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
OPINION_MASTER_MARKET_URL=https://...  # Comma-separated URLs allowed (frontend DevTools)
OPINION_SCAN_ONLY=true                 # Disable order submission (scan-only mode)

# Environment Selection
OPINION_ENV=local  # local or aws (defaults to aws)
OPINION_CHAIN_ID=56  # Optional override for market sweep (defaults to 56)
OPINION_WS_CHANNEL=market.data.<pair>  # WS probe single channel
OPINION_WS_CHANNELS=market.data.<pair1>,market.data.<pair2>  # WS probe multi-channel
OPINION_WS_PROBE_SECS=20  # WS probe duration (seconds)
WS probe raw payload: cargo run --bin ws_probe -- '{"action":"subscribe","channel":"..."}'

# Proxy Configuration (local only)
OPINION_PROXY_URL=http://127.0.0.1:7890  # FlClash proxy port

# Blockchain
BSC_RPC_URL=https://bsc-dataseed.binance.org
OPINION_PRIVATE_KEY=0x...
PRIVATE_KEY=0x...  # Fallback for legacy setups
OPINION_EOA_ADDRESS=0x...

# Data Collection (Phase 0 - SQLite Recorder)
OPINION_DATA_ENABLED=true          # Enable/disable recorder (default: true)
OPINION_DATA_DB_PATH=data/opinion-hft.sqlite  # Base DB path (date suffix added automatically)
OPINION_DATA_WAL=true               # Enable WAL mode for concurrent reads/writes (default: true)
OPINION_DATA_BATCH_SIZE=200         # Batch size before auto-flush (default: 200)
OPINION_DATA_FLUSH_MS=1000         # Flush interval in milliseconds (default: 1000)
OPINION_DATA_OB_DEPTH=10           # Orderbook depth to record (default: 10)
OPINION_DATA_QUEUE_CAP=2000        # Event queue capacity (default: 2000)
```

## Data Collection (Phase 0 - SQLite Recorder)

The bot includes a SQLite-based data collector for recording market data and confirmed trades.

**Key Features:**
- **Daily DB rotation**: Creates separate SQLite files per day (`data/opinion-{YYYY-MM-DD}.sqlite`)
- **WAL mode**: Configurable Write-Ahead Logging for concurrent reads/writes
- **Batched writes**: Async batch writer with configurable size/flush interval
- **Source of truth**: Only records confirmed orders from `ConfirmationTracker` (on-chain verified), not unreliable WebSocket data

**Configuration:**
- Config struct: `src/config.rs::DataCollectionConfig`
- Module: `src/data/recorder.rs`
- Integration: `src/main.rs` - wired into confirmation loop

**Schema:**
- `orderbook_snapshots` - Market depth snapshots with timestamps
- `orderbook_levels` - Individual bid/ask levels linked to snapshots
- `confirmed_orders` - Verified order executions with position tracking

**Usage:**
```rust
use opinion_hft::data::recorder::spawn_recorder;

let data_config = DataCollectionConfig::from_env();
let recorder_handle = spawn_recorder(data_config)
    .await
    .map(|(handle, _join)| handle);

// In confirmation loop:
if let Some(recorder) = &recorder_handle {
    recorder.record_confirmed_order(&confirmed).await;
}
```

## Testing Analysis (2026-01-18)

### Live Test Results

**Summary**: All three live test runs resulted in losses.

**Tests Analyzed**:
1. `live_order_tests.rs` - Full order lifecycle
2. `trading_scenarios.rs` - Market scenario testing  
3. `live_round_trip.rs` - Complete trading workflow

---

### Root Causes of Losses

#### 1. Test Amounts Below Minimum (1.30 USDT)

**Current State**:
- `live_order_tests.rs:119` - Uses **2.0 USDT** (below 1.30 minimum)
  - Code: `let price = 0.001; ... Minimum amount is 1.30 USDT. We use 2.0 USDT to be safe.`
  - Actual value: 2.0 * 0.001 = 0.002 USDT (far below minimum!)
- `live_batch_operations.rs:59` - Calculates price as **0.01 + i * 0.001**
  - For `i=1`: price = 0.02, value = 2.0 USDT (barely above minimum)
  - For `i=2000`: price = 2.01, value = 2000 * 0.001 = 2.0 USDT
  - **Problem**: Floating point precision and incorrect formula

**Impact**:
- Orders below 1.30 USDT are **rejected by API**
- Even when accepted, fills occur at terrible prices (huge slippage)

#### 2. Extreme Price Placement (0.001 USDT)

**Current State**:
- `live_order_tests.rs:137-150` - Uses price **0.001** to create "ghost orders"
  - Code: `let price = 0.01; // Very low price (won't fill)`
  - Intent: Creates order that won't execute but prevents orderbook interference

**Impact**:
- Wastes API quota
- Prevents testing real order execution behavior
- If it fills, slippage would be >99%

#### 3. Market Depth Insufficient

**Current State**:
- `trading_scenarios.rs:218` - Places 2000 shares at 0.01 price = 2 USDT value
  - Code: `let price = 0.01; let quantity = 200; // Value = 2 USDT`
  - Attempting arbitrage with 2 USDT orders on shallow orderbook
  - Result: Orders pushed prices significantly (high slippage)
  - Test prices: 0.1, 0.3, 0.5, 0.7, 0.9 (unrealistic spread)
  - Fee peaks at 0.5: `base_fee = 0.02 * 0.5 * 0.5 = 0.005 (~0.45%)`

**Impact**:
- Large orders (even 2 USDT) cannot be filled without moving price
- Real arbitrage opportunities are rare and short-lived
- Fees + spread = guaranteed loss

#### 4. Actual Arbitrage Conditions

**Market Reality**:
- Opinion Trade markets are prediction markets with limited orderbook depth
- Liquidity is often concentrated around current market price
- Large trades (even small USDT amounts) can move prices significantly
- Spreads are typically 1-3% at best, not 0-100% in tests

---

### Recommended Fixes

#### Immediate Actions Required

1. **Fix Test Amounts (P0 - Critical)**
    ```rust
    // ❌ WRONG - Below minimum
    let price = 0.001;
    let quantity = 200;  // Value = 2 USDT (below 1.30!)
    
    // ✅ CORRECT - Meet minimum
    let price = 0.5;  // Mid-range realistic price
    let quantity = 130;  // 130 * 0.5 = 65 USDT (above 1.30 minimum)
    ```

2. **Remove Extreme Price Orders**
    - Delete or comment out ghost order placement (price = 0.001)
    - Use prices that can actually execute (0.3 - 0.7 range)

3. **Add Market Depth Validation**
    ```rust
    // Before placing order, check available liquidity
    let orderbook = client.get_orderbook(token_id).await?;
    let total_liquidity = sum_bid_liquidity(orderbook) + sum_ask_liquidity(orderbook);
    if total_liquidity < 3.0 {  // Below test size
        warn!("Insufficient liquidity: {:.2} USDT", total_liquidity);
        return Ok(());
    }
    ```

4. **Use Conservative Test Strategy**
    - Test order flow with 1.5 USDT (not 65 USDT)
    - Measure actual slippage in production conditions
    - Validate minimum fill amount and execution quality

---

### Testing Guidelines Going Forward

**When modifying integration tests, follow these rules:**

1. **Minimum Order Amount**: All live tests MUST use **1.30 USDT minimum**
    - Formula: `price * quantity >= 1.30`
    - Recommended: Use 1.5 - 65 USDT range with prices 0.3 - 0.7

2. **No Ghost Orders**: Never place orders that won't execute
    - Minimum price: 0.3 for YES tokens
    - Never use 0.001 or similar extreme values

3. **Orderbook Awareness**: Check liquidity before placing orders
    - Ensure total available liquidity is at least 10x the order size
    - Fail fast if market is shallow

4. **Realistic Test Prices**: Use actual market conditions
    - Spread in prediction markets is typically 1-3%
    - Test prices should be 0.3, 0.5, 0.7 (not 0.1, 0.9)

5. **Measure Don't Just Assume**: Always log actual fill amounts, prices, and fees
    - Calculate and report real P&L for each trade
    - Use this data to improve strategy, not just pass/fail tests
