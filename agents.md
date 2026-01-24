# Opinion HFT - LLM Development Context

## Project Overview
High-frequency trading bot for Opinion.trade prediction markets on BNB Chain (56).

## Quick Commands
```bash
cargo fmt --all          # Format code (required)
cargo fmt --all --check  # Verify formatting (CI-style)
cargo clippy --all -- -D warnings # Strict linting (0 warnings required)
cargo test --lib         # Run 69 unit tests (no API required)
cargo test --tests       # Run ALL live integration tests (requires .env)
cargo test --test <name> # Run specific test file (e.g., live_account_tests)
cargo run                # Start bot
cargo run --bin market_sweep  # One-off market metadata sweep (logs all markets)
cargo run --bin ws_probe      # One-off WS probe (subscribe + dump raw events)
```

## Formatting Standard

**Version**: 1.1.0
**Effective Date**: 2026-01-20
**Status**: Active

### Goal

Establish a single, definitive formatting standard to eliminate confusion between rustfmt and clippy rules.

### Core Principles

1. **Single Source of Truth**: Only one formatting standard in the project
2. **Automation**: Use rustfmt for automatic formatting, don't rely on manual memory
3. **Enforceable**: Rules can be checked and enforced via tools
4. **Minimalism**: Rules are simple and clear, reducing cognitive burden

### 1. rustfmt Configuration (Mechanical Formatting)

**Config File**: `rustfmt.toml`

```toml
edition = "2021"
hard_tabs = false
tab_spaces = 4
max_width = 100
```

**Coverage**

- ✅ **Indentation**: 4 spaces
- ✅ **Line width**: 100 characters (default)
- ⚠️ **Not covered**: Chained calls, string literals, etc. (keep defaults)

**Rationale**

- Rust official standard
- Maximum readability
- Consistent with existing codebase style

### 2. clippy Configuration (Linting and Style)

#### 2.1. Error Handling

- ✅ **Prefer structured error types**: `crate::error::Error`
- ✅ **Use `.into()` to convert error messages**: `.into()` instead of `format!()` (preserves type information)
- ❌ **Forbidden**: String-formatted errors (loses type information)

**Examples**:
```rust
// ✅ Correct
return Err(Error::ApiError {
    status: status.as_u16(),
    message: format!("HTTP error {}", status),
});

// ❌ Wrong
return Err(Error::ApiError(
    format!("HTTP error {}", status).into()
));
```

#### 2.2. Naming Conventions

- ✅ **Types**: `PascalCase` (uppercase first letter)
- ✅ **Functions/Methods**: `snake_case` (underscore-separated)
- ✅ **Constants**: `SCREAMING_SNAKE_CASE` (all caps with underscores)

**Exception**: Naming from external dependencies follows their style

#### 2.3. Structs

- ✅ **Field ordering**: Public API fields first, private fields last
- ✅ **derive macros**: Use `#[derive(Debug, Clone)]` (small types can add `#[derive(Copy)]`)

**Exception**: Test helper structs can be arranged by logical order

#### 2.4. Comments

- ✅ **Public APIs**: Use `///` triple-slash documentation comments
- ⚠️ **Internal implementation**: Prefer `//` double-slash, keep concise
- ❌ **Forbidden**: Block comments `/* */` (unless in temporary code)

**Comment Content Guidelines**:
- Explain "what" (what it is)
- Avoid "how to implement"

**Example**:
```rust
/// Represents a detected arbitrage opportunity.
///
/// This struct contains all information needed to execute
/// an arbitrage trade, including cost estimates and profit margins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    /// Unique identifier for tracking this opportunity
    pub id: String,

    /// Type of arbitrage strategy detected
    pub arb_type: ArbitrageType,

    // Internal implementation details
    // Timestamp when opportunity was detected
    pub detected_at: u64,
}
```

#### 2.5. Async/Await

- ✅ **spawn and await**: Use `.await` (preserve JoinHandle or cancellation mechanism)
- ✅ **select!**: Multi-way event selection
- ❌ **Forbidden**: Blocking calls (`std::thread::sleep`)

**Examples**:
```rust
// ✅ Correct
let handle = tokio::spawn(async move { /* ... */ });

// ✅ Correct
tokio::select! {
    Some(event) = rx.recv() => { /* ... */ },
    _ = tick.tick() => { /* ... */ },
}
```

#### 2.6. Test Code

- ✅ **Test functions**: Use `#[tokio::test]`
- ✅ **Test naming**: `test_<scenario>_<behavior>` (e.g., `test_fetch_active_markets`)
- ⚠️ **Integration tests**: Require real `.env` configuration, prefixed with `live_`

**Test Data Constants**:
- Minimum order amount: `1.30` USDT
- Minimum fee: `0.50` USDT
- Reasonable price range: `0.3 - 0.7`

### 3. Handling Differences from rustfmt/clippy

#### 3.1. rustfmt Override: Acceptable Differences

Deviations from rustfmt default format are allowed in these cases:

- **Long URL/path strings**: May exceed 100 character limit
- **Complex error messages**: Long lines caused by `format!()` macros

#### 3.2. clippy Warning Handling Strategy

- **Fix immediately**: Warnings that block compilation must be fixed first
- **Technical debt**: Non-blocking warnings can be recorded as technical debt, allow follow-up handling
- **False positives**: If confirmed as false positives, add `#[allow(...)]`

### 4. Mandatory Execution Workflow

To maintain the high standards of the Opinion HFT project, all contributors must follow this workflow before committing code or submitting tasks.

#### 4.1. Formatting (Required - Mandatory)
Code must be formatted using the project's `rustfmt` configuration. This ensures a consistent style across the entire codebase. Run this after any code changes.
```bash
cargo fmt --all
```

**Verification step (required before commit/PR):**
```bash
cargo fmt --all --check
```

#### 4.2. Linting (Required - Zero Warning Policy)
Clippy must be run with all warnings treated as errors. **Submissions with any clippy warnings will be rejected.**
```bash
cargo clippy --all -- -D warnings
```

#### 4.3. Submission Gate (Strict)
**No commit or PR is allowed unless ALL items below pass in order.**
1. **Format** (enforced):
   ```bash
   cargo fmt --all
   cargo fmt --all --check
   ```
2. **Lint** (zero warnings):
   ```bash
   cargo clippy --all -- -D warnings
   ```
3. **Core tests** (must pass):
   ```bash
   cargo test --lib
   ```

If any step fails, fix and re-run from step 1.

#### 4.4. Pre-Submission Checklist
Perform these checks immediately before final submission:
- [ ] **Formatting**: `cargo fmt --all` then `cargo fmt --all --check`
- [ ] **Linting**: `cargo clippy --all -- -D warnings` (0 warnings)
- [ ] **Tests**: `cargo test --lib`
- [ ] **Documentation**: All new public types/functions have `///` comments
- [ ] **Proof**: Include fmt/clippy/test results in PR description (or paste logs in task update)

#### 4.5. CI/CD Integration (Enforced)

**GitHub Actions / Automation Tools**:
- Automation tools run `cargo fmt --all --check` to verify formatting.
- CI runs `cargo clippy --all -- -D warnings` and blocks PRs with any warnings.

#### 4.6. Local Guardrails (Required)
Set up local hooks or scripts so formatting/linting cannot be skipped.

**Minimum requirement**: a local pre-commit/pre-push hook that runs:
```bash
cargo fmt --all
cargo fmt --all --check
cargo clippy --all -- -D warnings
```

**Strict rule**: If hooks are unavailable, manually run the exact commands above before every commit.

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

## Strategy Shared Kernel (Core)

The strategy logic is now centralized under `src/strategy/core/` and reused by live trading and
backtest/research paths. `src/strategy/arbitrage.rs` is a thin facade that re-exports core types.

**Modules:**
- `core/detector.rs`: `ArbitrageDetector`, `ArbitrageOpportunity`, detection logic
- `core/fees.rs`: fee curve + fee quoting helpers
- `core/risk.rs`: minimal risk rules used pre-execution
- `core/mod.rs`: shared types (`MarketInfo`, `OrderbookSide`, `TradeSide`, `MarketOrderbook`)

**Backtest integration:**
- `src/strategy/backtest/mod.rs` uses `strategy::core` types directly.
- `src/bin/research.rs` uses `ArbitrageDetector` from `strategy::core`.

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
- **Maker (Limit Order)**: **0% Fee** ✅ - Providing liquidity is completely free
- **Taker (Market Order)**: **Dynamic Fee 0% - 2%** - Paid on execution

### Fee Calculation Formula
```
Base Fee Rate = topic_rate × price × (1 - price)
Effective Fee Rate = Base Fee Rate × (1 - vip_discount) × (1 - referral_discount)
Actual Fee = max(Trade Amount × Effective Fee Rate, $0.50)
```

### Fee Examples (topic_rate = 0.02)
| Price | Base Fee Rate | Description |
|-------|--------------|-------------|
| 0.1 | 0.18% | Low uncertainty |
| 0.5 | **0.50%** | Highest fee (maximum uncertainty) |
| 0.9 | 0.18% | Low uncertainty |

### Key Limits
- **Minimum Order Amount**: $5.00 USDT
- **Minimum Fee**: $0.50 USDT (even if calculated value is lower)
- **Minimum Withdrawal**: $5.00 USDT

### Discount System
- VIP Program: Volume-tiered discounts
- Referral Program: Inviter receives 5% rebate of invitee's fees
- Promotions: Limited-time event discounts
- **Discounts stack** (multiplicative accumulation)

### Actual Cost Examples
**Market Order Taker Buy $100 @ 0.5**:
- Calculated Rate: 0.02 × 0.5 × 0.5 = 0.005 (0.5%)
- Calculated Fee: $100 × 0.5% = $0.50
- Actual Charge: $0.50 (reaches minimum limit)

**Small Order $10 @ 0.5**:
- Calculated Fee: $10 × 0.5% = $0.05
- Actual Charge: $0.50 (minimum fee floor)

**Key Takeaway**: Market orders incur at least $0.50 loss, making small orders even less cost-effective.

### Code Implementation
```rust
// src/strategy/arbitrage.rs:106-114
pub fn calculate_fee_rate(&self, price: f64) -> f64 {
    let base_fee = self.topic_fee_rate * price * (1.0 - price);
    base_fee * (1.0 - self.referral_discount)
}

// Default configuration
topic_fee_rate: 0.02,   // 2% max fee rate
referral_discount: 0.10, // 10% referral discount
```

**Official Documentation**: https://docs.opinion.trade/trade-on-opinion.trade/fees

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

---

## Agent Code Submission Workflow

> **CRITICAL**: Always create GitHub PRs. Never merge locally. This protects the main branch and leverages CI as a safety gate.

### Why PR, Not Local Merge?

| Risk of Local Merge | PR Protection |
|---------------------|---------------|
| Broken code in main | CI runs BEFORE merge |
| "Hallucination" code (missing deps) | CI catches build errors |
| No review opportunity | Clear diff view for human review |
| Destroys worktree isolation | Main branch stays clean |

### Standard Workflow for Agents

#### 1. Setup GitHub CLI

```bash
# Install gh (if not present)
brew install gh  # macOS
sudo apt install gh  # Ubuntu/Debian

# Login (once per machine)
gh auth login
```

#### 2. Submit Code via Worktree

```bash
# Create isolated worktree for your task (recommended)
git worktree add -b feature/agent-task-name /path/to/worktree origin/main

# In your worktree directory:
# ... write code ...

# Run pre-submission checks
cargo fmt --all
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --lib

# Commit your changes
git add .
git commit -m "feat: implement feature X"

# Push to remote branch
git push -u origin feature/agent-task-name
```

#### 3. Create Pull Request

```bash
# Create PR (fills title/body from commit message)
gh pr create --base main --fill

# Optional: Enable auto-merge if CI passes
gh pr merge --auto --rebase
```

#### 4. Handle CI Failures

```bash
# Check CI status
gh pr checks

# If failed, read logs and fix
gh run view <run-id> --log | grep -A 5 "error:"

# Fix code, then update PR
git add .
git commit -m "fix: resolve CI error"
git push

# CI automatically re-runs
```

### CI Safety Gates

The repository has these CI checks (see `.github/workflows/quick-check.yml`):

1. **Format Check**: `cargo fmt --all --check`
2. **Build Check**: `cargo check --lib --all-features`
3. **Clippy**: `cargo clippy --lib -- -D warnings`
4. **Unit Tests**: `cargo test --lib`

**Your code MUST pass all 4 before requesting merge.**

### Forbidden Commands

```bash
# ❌ NEVER do this
git merge main
git checkout main && git merge feature-branch
git push origin main

# ✅ Always do this
git push -u origin feature-branch
gh pr create --base main --fill
```

### Quick Reference

```bash
# Full submission sequence
cd /path/to/worktree
cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test --lib
git add -A && git commit -m "type: description"
git push -u origin feature/branch-name
gh pr create --base main --fill
gh pr checks  # Verify CI passes
```

---

### Branch Protection (Local Hook)

Since this is a private repository without GitHub Pro, we use a **local pre-push hook** to prevent direct main pushes.

**Location**: `.githooks/pre-push`

```bash
# When you try to push to main directly:
$ git push origin main
🚫 DIRECT PUSH TO MAIN BRANCH DENIED

Solution:
  1. Create a feature branch:
     git checkout -b feature/your-task-name
  ...
```

**Why this matters**:
- Prevents accidental direct pushes
- Enforces PR workflow locally
- Works even without GitHub Pro

**To verify setup**:
```bash
git config core.hooksPath .githooks  # If not already configured
```

---

### Automated PR Status Check Script

Agents can use this script to automatically check and handle CI results:

```bash
#!/bin/bash
# pr-check-and-fix.sh - Automated PR CI check with self-fix

BRANCH=$(git branch --show-current)
echo "Checking PR for branch: $BRANCH"

# Get the PR number for current branch
PR_NUM=$(gh pr list --head "$BRANCH" --json number -q '.[0].number')

if [[ -z "$PR_NUM" ]]; then
    echo "❌ No PR found for branch $BRANCH"
    exit 1
fi

echo "Found PR #$PR_NUM"

# Check CI status
STATUS=$(gh pr checks "$PR_NUM" --json status,conclusion -q '.[] | select(.name == "Quick Check") | {status, conclusion}')

if [[ "$STATUS" == *"success"* ]]; then
    echo "✅ CI passed! PR #$PR_NUM is ready to merge."
    echo "You can merge with: gh pr merge $PR_NUM --auto --rebase"
    exit 0
else
    echo "❌ CI failed or still running"
    echo "Status: $STATUS"

    # Show failed jobs
    echo ""
    echo "Failed jobs:"
    gh pr checks "$PR_NUM" --json name,status,conclusion -q '.[] | select(.conclusion != "success") | "- \(.name): \(.status) (\(.conclusion))"'

    echo ""
    echo "Reading error logs..."
    RUN_ID=$(gh pr checks "$PR_NUM" --json runId -q '.[0].runId')
    gh run view "$RUN_ID" --log 2>&1 | grep -A 3 "error:" | head -20

    echo ""
    echo "Fix the issues above, then:"
    echo "  git add -A && git commit -m 'fix: resolve CI error'"
    echo "  git push"
    echo ""
    echo "CI will automatically re-run after the push."
    exit 1
fi
```

---

## MR Merge Agent

> **Trigger Condition**: Only activated when user explicitly requests "merge MR/PR", "merge PR", "handle merge requests".
> **Scope**: PR review, conflict resolution, merge execution, follow-up issue creation.

### Role Definition

MR Merge Agent is an automated agent specifically responsible for Pull Request lifecycle management. It does not proactively intervene in daily development, only engaging when user explicitly requests merge operations.

### Trigger Keywords

The following keywords activate this agent:
- "merge PR", "merge MR"
- "handle merge request", "review and merge"
- "merge these PRs into main"
- "resolve conflicts and merge"

### Workflow

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                     MR MERGE AGENT WORKFLOW                                       │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  Phase 1: Intelligence Gathering                                                  │
│  ├─ List all relevant PRs: gh pr list --repo <repo> --state open                 │
│  ├─ Get each PR details: gh pr view <num> --json title,body,files,state          │
│  ├─ Check CI status: gh pr checks <num>                                          │
│  ├─ Analyze dependencies: Which PRs modify the same files?                       │
│  └─ Check diff with main: gh api repos/.../compare/main...<branch>               │
│         │                                                                        │
│         ▼                                                                        │
│  Phase 2: Code Review                                                             │
│  ├─ Download and read key files (new/modified .rs, .md files)                    │
│  ├─ Check implementation completeness against architecture.md                    │
│  ├─ Verify code standards (rustfmt, clippy, naming conventions)                  │
│  ├─ Identify potential risks (schema changes, API changes, breaking changes)     │
│  └─ Generate detailed Review Comment and post to GitHub                          │
│         │                                                                        │
│         ▼                                                                        │
│  Phase 3: Dependency Analysis                                                     │
│  ├─ Build PR dependency graph (which PR must be merged first)                    │
│  ├─ Identify conflict risks (same files modified by multiple PRs)                │
│  ├─ Determine optimal merge order (infrastructure → features → docs)             │
│  └─ Generate merge plan and confirm with user                                    │
│         │                                                                        │
│         ▼                                                                        │
│  Phase 4: Conflict Resolution                                                     │
│  ├─ Clone repository to temporary directory                                      │
│  ├─ Checkout target branch and attempt merge/rebase main                         │
│  ├─ If conflicts: Read conflict files, analyze <<<<<<< ======= >>>>>>> markers  │
│  ├─ Smart merge: Preserve valid changes from both sides, resolve semantic conflicts│
│  ├─ Commit resolution and push to remote branch                                  │
│  └─ Re-trigger CI checks                                                         │
│         │                                                                        │
│         ▼                                                                        │
│  Phase 5: Merge Execution                                                         │
│  ├─ Merge each PR in order: gh pr merge <num> --squash --delete-branch           │
│  ├─ Wait for GitHub to update after each merge (sleep 10s)                       │
│  ├─ If subsequent PRs need rebase, execute rebase and force push                 │
│  ├─ Verify merge results: git log --oneline -n 5                                 │
│  └─ Clean up temporary directory                                                 │
│         │                                                                        │
│         ▼                                                                        │
│  Phase 6: Follow-up                                                               │
│  ├─ If there are incomplete features, create GitHub Issue to track               │
│  ├─ Update completion status in architecture.md (if needed)                      │
│  └─ Report final results to user                                                 │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### Core Command Reference

```bash
# 1. List PRs
gh pr list --repo <owner/repo> --state open --limit 10

# 2. View PR details
gh pr view <num> --repo <owner/repo> --json title,body,files,state,headRefName,baseRefName

# 3. Check CI status
gh pr checks <num> --repo <owner/repo>

# 4. View PR diff
gh pr diff <num> --repo <owner/repo>

# 5. Compare branch differences
gh api repos/<owner/repo>/compare/main...<branch> --jq '{ahead: .ahead_by, behind: .behind_by}'

# 6. Post Review Comment
gh pr comment <num> --repo <owner/repo> --body-file /path/to/review.md

# 7. Create Follow-up Issue
gh issue create --repo <owner/repo> --title "<title>" --label "enhancement" --body-file /path/to/issue.md

# 8. Execute merge (squash + delete branch)
gh pr merge <num> --repo <owner/repo> --squash --delete-branch --subject "<commit message>"

# 9. Rebase branch
git checkout <feature-branch>
git fetch origin main
git rebase origin/main
git push origin <feature-branch> --force-with-lease
```

### Conflict Resolution Strategies

#### 1. Documentation Conflicts (*.md)
- **Strategy**: Usually preserve changes from both sides, merge content
- **Priority**: New status > old status, detailed descriptions > brief descriptions
- **Example**:
  ```markdown
  # HEAD (current branch)
  | Step 10 | Reorg Detection | ✅ Complete |
  
  # main (target branch)  
  | Step 5-9 | Research Features | 40% Complete |
  
  # Merged result
  | Step 5-9 | Research Features | 40% Complete |
  | Step 10 | Reorg Detection | ✅ Complete |
  ```

#### 2. Schema/Data Structure Conflicts (*.rs)
- **Strategy**: Preserve all new fields, ensure backward compatibility
- **Priority**: New fields must be preserved, field deletions require caution
- **Verification**: Must run `cargo test --lib` after merge

#### 3. Configuration File Conflicts (Cargo.toml, config.rs)
- **Strategy**: Preserve all new dependencies and configuration items
- **Verification**: Must run `cargo build` after merge

### Review Comment Template

```markdown
# PR #<num> Code Review - <Title>

## ✅ Summary
<1-2 sentences summarizing the PR's functionality and completeness>

## 📊 Implementation Status

### ✅ FULLY IMPLEMENTED
<List 100% complete features>

### ⚠️ PARTIAL / MISSING
<List partially complete or missing features>

## 🔍 Code Quality Assessment
- **Strengths**: <advantages>
- **Improvements Needed**: <areas for improvement>

## ✅ Merge Recommendation
**Status**: ✅ APPROVED / ⚠️ NEEDS WORK / ❌ REJECTED

**Pre-Merge Checklist**:
- [ ] Rebase onto latest main
- [ ] cargo test --lib passes
- [ ] cargo clippy --all -- -D warnings passes

**Post-Merge Actions**:
- [ ] <follow-up tasks>

---
**Reviewed by**: AI MR Merge Agent
**Date**: <YYYY-MM-DD>
```

### Follow-up Issue Template

```markdown
# [P<priority>] <Title>

## 📋 Summary
<Why this Issue is needed, background context>

## 🎯 Background
<Related PRs or previous work>

## 📝 Tasks
- [ ] Task 1: <description>
- [ ] Task 2: <description>
- [ ] Task 3: <description>

## ✅ Acceptance Criteria
- [ ] <acceptance criterion 1>
- [ ] <acceptance criterion 2>

## 🔗 Related
- **Original Spec**: <link to architecture.md section>
- **Implemented in**: PR #<num>

## 💡 Notes
<other notes>

**Priority**: P<0-3>
**Estimated Effort**: <X hours/days>
```

### Safety Rules

1. **Never push directly to main**: All changes must go through PR
2. **Never force push to main**: Only use `--force-with-lease` on feature branches
3. **Conflict resolution must be verified**: Run tests after resolving conflicts
4. **Confirm before merging**: Show merge plan to user and wait for confirmation
5. **Preserve audit trail**: Post all Review Comments to GitHub

### Error Handling

| Error | Resolution |
|-------|------------|
| PR status is CONFLICTING | Clone repo, manually resolve conflicts, push and retry |
| CI check failed | Analyze failure reason, request PR author to fix or self-fix |
| Merge failed | Check for new commits, rebase and retry |
| Branch not found | Verify branch name is correct, or PR is already merged |
| Permission denied | Prompt user to check GitHub token permissions |

### Usage Example

**User Input**:
> Merge these three PRs: #2, #4, #5

**Agent Response**:
1. Collect information on all three PRs
2. Analyze dependencies (#2 → #4 → #5)
3. Post Review Comment to each PR
4. Resolve any conflicts
5. Execute merges in order
6. Create Follow-up Issue (if needed)
7. Report final results

---

### Agent Activation Checklist

Before executing any operation, the agent should verify:
- [ ] User explicitly requested merge operation
- [ ] Target repository and PR numbers are confirmed
- [ ] GitHub CLI (`gh`) is logged in with proper permissions
- [ ] Local disk has sufficient space for temporary clone

---

**Last Updated**: 2026-01-24
**Version**: 1.0.0
