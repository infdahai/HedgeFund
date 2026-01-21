# Opinion Trade HFT Framework - Complete Context Document

> **Purpose**: This document captures **ALL** context, research findings, design decisions, and implementation details from our planning session. Use this to resume work in a new session.

---

## Table of Contents

1. [Architecture Overview](https://www.google.com/search?q=%23architecture-overview)
2. [Project Overview](https://www.google.com/search?q=%231-project-overview)
3. [Research Findings](https://www.google.com/search?q=%232-research-findings)
4. [Critical Constraints & Parameters](https://www.google.com/search?q=%233-critical-constraints--parameters)
5. [Architecture Design](https://www.google.com/search?q=%234-architecture-design)
6. [Module Specifications](https://www.google.com/search?q=%235-module-specifications)
7. [API Compatibility Design](https://www.google.com/search?q=%236-api-compatibility-design)
8. [Code Examples](https://www.google.com/search?q=%237-code-examples)
9. [Project Structure](https://www.google.com/search?q=%238-project-structure)
10. [Dependencies](https://www.google.com/search?q=%239-dependencies)
11. [Test Plan](https://www.google.com/search?q=%2310-test-plan)
12. [Implementation Order](https://www.google.com/search?q=%2311-implementation-order)
13. [New Session Prompt](https://www.google.com/search?q=%2312-new-session-prompt)

---

# Architecture Overview

### System Architecture Diagram

```text
┌──────────────────────────────────────────────────────────────────────────────────┐
│                            OPINION TRADE HFT FRAMEWORK                           │
├──────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │                              DATA PLANE                                     │
│  │                                                                             │
│  │   ┌─────────────────┐   ┌─────────────────┐   ┌─────────────────────────┐   │ │
│  │   │  WebSocket      │   │   REST Client   │   │    Chain Client         │   │ │
│  │   │  Client         │   │                 │   │    (Alloy)              │   │ │
│  │   │                 │   │                 │   │                         │   │ │
│  │   │ ┌─────────────┐ │   │  /markets       │   │  balanceOf()            │   │ │
│  │   │ │ market.depth.diff │ │   │  /orderbook     │   │  balanceOfBatch()       │   │ │
│  │   │ │ order.update│ │   │  /order         │   │                         │   │ │
│  │   │ │ trade.record│ │   │                 │   │  ConditionalTokens      │   │ │
│  │   │ └─────────────┘ │   │  ?apikey=XXX    │   │  (ERC-1155)             │   │ │
│  │   └────────┬────────┘   └────────┬────────┘   └────────────┬────────────┘   │ │
│  │            │                     │                         │                │ │
│  │            └─────────────────────┼─────────────────────────┘                │ │
│  │                                  ▼                                          │ │
│  │                    ┌─────────────────────────────┐                          │ │
│  │                    │        Event Bus            │                          │ │
│  │                    │   (tokio::broadcast)        │                          │ │
│  │                    └─────────────┬───────────────┘                          │ │
│  └──────────────────────────────────┼──────────────────────────────────────────┘ │
│                                     │                                            │
│                                     ▼                                            │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │                            EXECUTION PLANE                                  │
│  │                                                                             │
│  │   ┌─────────────────┐   ┌─────────────────┐   ┌─────────────────────────┐   │ │
│  │   │   EIP-712       │   │  Priority Queue │   │    Rate Limiter         │   │ │
│  │   │   Signer        │   │                 │   │    (Governor GCRA)      │   │ │
│  │   │                 │   │                 │   │                         │   │ │
│  │   │  Precomputed:   │   │  Critical ━━━▶ │   │                         │   │ │
│  │   │  • Domain Sep   │   │  High     ━━━▶ │   │  15 TPS                 │   │ │
│  │   │  • Type Hash    │   │  Medium   ━━━▶ │   │  Burst: 3               │   │ │
│  │   │                 │   │  Low      ━━━▶ │   │  ~15ns/check            │   │ │
│  │   │  k256 ECDSA     │   │  FIFO per level │   │  until_ready().await    │   │ │
│  │   └─────────────────┘   └────────┬────────┘   └─────────────────────────┘   │ │
│  │                                  │                                          │ │
│  │                                  ▼                                          │ │
│  │             ┌───────────────────────────────────────────────┐               │ │
│  │             │            Confirmation Tracker               │               │ │
│  │             │                                               │               │ │
│  │             │  ┌─────────────────────────────────────────┐  │               │ │
│  │             │  │  Mode: TrustWebSocket | OnChainOnly |   │  │               │ │
│  │             │  │       Hybrid (recommended)              │  │               │ │
│  │             │  └─────────────────────────────────────────┘  │               │ │
│  │             │                                               │               │ │
│  │             │  1. Record position_before                    │               │ │
│  │             │  2. Wait for WS status="Finish"               │               │ │
│  │             │  3. Query on-chain balanceOf()                │               │ │
│  │             │  4. Calculate: position_after - position_before│              │ │
│  │             │  5. Emit ConfirmationEvent                    │               │ │
│  │             └───────────────────────────────────────────────┘               │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
└──────────────────────────────────────────────────────────────────────────────────┘

```

### Module Interactions

```text
┌─────────────┐     OrderIntent      ┌─────────────────┐
│   Strategy  │ ──────────────────▶  │  Priority Queue │
│   (User)    │                      │                 │
└─────────────┘                      └────────┬────────┘
                                              │
                                              ▼
┌─────────────┐                      ┌─────────────────┐
│  EIP-712    │ ◀─── sign_order ──── │  Order Gateway  │
│  Signer     │                      │  (orchestrator) │
└─────────────┘                      └────────┬────────┘
                                              │
              ┌───────────────────────────────┼───────────────────────────────┐
              ▼                               ▼                               ▼
     ┌─────────────────┐             ┌─────────────────┐             ┌─────────────────┐
      │  Rate Limiter   │             │   REST Client   │             │ Confirmation    │
      │  (wait permit)  │ ─────────▶  │   POST /order   │ ─────────▶  │ Tracker         │
      └─────────────────┘             └─────────────────┘             └────────┬────────┘
                                                                              │
                                    ┌───────────────────────────────────────┘
                                    ▼
                           ┌─────────────────┐             ┌─────────────────┐
                           │   WebSocket     │ ─────────▶  │   Chain Client  │
                           │ (status=Finish) │ trigger     │   balanceOf()   │
                           └─────────────────┘             └─────────────────┘

```

### Data Flow Summary

| Step | Component | Action | Latency |
| --- | --- | --- | --- |
| 1 | Strategy | Generate OrderIntent | User code |
| 2 | Priority Queue | Enqueue with priority | <0.1ms |
| 3 | Rate Limiter | Wait for permit | 0-66ms |
| 4 | EIP-712 Signer | Sign order | <0.3ms |
| 5 | REST Client | POST /order | 30-80ms |
| 6 | WebSocket | Wait for status=Finish | 3-5s |
| 7 | Chain Client | Query balanceOf() | 50-100ms |
| 8 | Confirmation | Calculate actual fill | <0.1ms |

**Total Latency: 3.1-5.5 seconds** (dominated by chain confirmation)

### Module Decoupling Principles

1. **Data Plane modules have NO dependencies on Execution Plane**
* WebSocket, REST, Chain clients are pure I/O.
* They emit events, don't process business logic.


2. **Execution Plane modules communicate via events and channels**
* `broadcast::Sender<WsEvent>` for WebSocket events.
* `mpsc::Sender<ConfirmationEvent>` for confirmations.
* No direct struct references between modules.


3. **Configuration is injected, not hardcoded**
* All modules receive `&Config` or specific config structs.
* Environment variables loaded via `dotenvy`.


4. **Each module is independently testable**
* Mock configs for unit tests.
* Integration tests with `#[ignore]` for live API.



---

## 1. Project Overview

### 1.1 Goal

Build a high-frequency trading (HFT) framework for the Opinion Trade prediction market platform on BNB Chain. The framework must achieve:

* Minimum tick-to-trade latency.
* Reliable trade confirmation via on-chain verification.
* Smart API rate limit handling (15 TPS).
* Multi-market concurrent trading support.

### 1.2 Technology Stack

| Component | Choice | Rationale |
| --- | --- | --- |
| **Language** | Rust | Zero-cost abstractions, memory safety, predictable latency |
| **Async Runtime** | Tokio | Industry standard for async Rust |
| **Ethereum Lib** | Alloy | 10x faster than ethers-rs, actively maintained |
| **EIP-712** | alloy-sol-types + k256 | Pure Rust, no FFI overhead |
| **Rate Limiting** | Governor | GCRA algorithm, 15ns per check, single AtomicU64 |
| **HTTP Client** | rrequest + Chrome120 | Bypasses JA3 TLS fingerprinting on all endpoints |
| **WebSocket** | tokio-tungstenite | Native async WebSocket |
| **Concurrent Map** | DashMap | Lock-free concurrent HashMap |

### 1.3 User Requirements

1. **Code and comments in English** (全英文代码和注释).
2. **Test-driven development** - All modules must pass tests before approval.
3. **Design document** for Gemini 3.0 Pro audit.
4. **Multi-agent implementation** - Use multiple AI models for parallel development.

---

## 2. Research Findings

### 2.1 Opinion Trade API Documentation

#### REST API

**Base URL:** `https://proxy.opinion.trade:8443/openapi/`

| Endpoint | Method | Description |
| --- | --- | --- |
| `/markets` | GET | List all markets |
| `/orderbook/{pair}` | GET | Get orderbook for a trading pair |
| `/order` | POST | Create a new order |
| `/order/cancel` | POST | Cancel an order |
| `/order` | GET | Get user's orders |

#### Authentication

> ⚠️ **CRITICAL:** API key is passed as a **QUERY PARAMETER**, not header!

* **URL Pattern:** `https://proxy.opinion.trade:8443/openapi/order?apikey={YOUR_API_KEY}`
* **Rate Limit:** 15 TPS (requests per second) default.

#### WebSocket API

**Endpoint:** `wss://ws.opinion.trade?apikey={API_KEY}`

**Market Channels:**
| Channel | Description |
| :--- | :--- |
| `market.depth.diff` | Orderbook diff updates (subscribe with `marketId` = `topicId`) |
| `market.trade.{pair}` | Public trade stream |

**Orderbook Subscribe Payload:**
`{"action":"SUBSCRIBE","channel":"market.depth.diff","marketId":<int>}`

**Diff Payload Fields:** `msgType`, `marketId`, `outcome` or `outcomeSide`, `side`, `price`, `size`

**User Channels:**
| Channel | Description | Events |
| :--- | :--- | :--- |
| `trade.order.update` | Order status updates | `orderNew`, `orderFill`, `orderCancel`, `orderConfirm` |
| `trade.record.new` | Trade confirmations | Contains txHash, final amounts |

### 2.2 SDK Reverse Engineering (Python/TypeScript)

#### EIP-712 Domain (CRITICAL!)

```python
# From Opinion Python SDK
EIP712_DOMAIN = {
    "name": "OPINION CTF Exchange",  # ⚠️ NOTE THE SPACE!
    "version": "1",
    "chainId": 56,
    "verifyingContract": "0xAD1a38cEc043e70E83a3eC30443dB285ED10D774"
}

```

#### Order Type Definition

```python
ORDER_TYPES = {
    "Order": [
        {"name": "salt", "type": "uint256"},
        {"name": "maker", "type": "address"},
        {"name": "signer", "type": "address"},
        {"name": "taker", "type": "address"},
        {"name": "tokenId", "type": "uint256"},
        {"name": "makerAmount", "type": "uint256"},
        {"name": "takerAmount", "type": "uint256"},
        {"name": "expiration", "type": "uint256"},
        {"name": "nonce", "type": "uint256"},
        {"name": "feeRateBps", "type": "uint256"},
        {"name": "side", "type": "uint8"},
        {"name": "signatureType", "type": "uint8"},
    ]
}

```

#### Price Precision

⚠️ Price has **1 decimal place**: `0.1` - `0.9`. **NOT** 2 or 3 decimals like some other platforms!

### 2.3 BNB Chain RPC Latency Research

| Provider Type | Latency | Notes |
| --- | --- | --- |
| Co-location dedicated node | < 10 ms | Best for HFT, expensive |
| Dedicated node (RPC Fast) | < 4 ms | Isolated, excellent performance |
| QuickNode (US) | 45 ms | Best commercial option |
| Atlas Network | 55-58 ms (median) | Stable P95 |
| Public RPC | > 1 second (some) | ⚠️ NOT suitable for HFT |

**Key Findings:**

* WebSocket subscription is better than HTTP polling (zero latency push vs polling interval).
* BSC block time: 0.75 seconds (post-2025 upgrade).
* BSC finality: ~1.875 seconds.
* BSC Fermi upgrade (Jan 2026): Block time → 0.45s, finality → 1.125s.

### 2.4 Rate Limiting Research

#### Algorithm Comparison

| Algorithm | Burst Support | Memory | CPU | HFT Suitability |
| --- | --- | --- | --- | --- |
| Token Bucket | Excellent | Higher | Medium | Aggressive strategies |
| Leaky Bucket/GCRA | Limited | Very Low | Very High | Stable strategies |

#### Governor Crate (Recommended)

```rust
use governor::{Quota, RateLimiter};
use nonzero_ext::nonzero;

// 15 TPS with burst of 3
let limiter = RateLimiter::direct(
    Quota::per_second(nonzero!(15u32))
        .allow_burst(nonzero!(3u32))
);

// Check (non-blocking, ~15ns)
if limiter.check().is_ok() {
    // Proceed
}

// Wait (async)
limiter.until_ready().await;

```

### 2.5 Community Feedback (CRITICAL!)

From user conversations about Opinion Trade:

> **Q:** "成交后api推送的数据，不管用order.Shares还是order.FilledShares，都不准"
> **A:** "只要订单状态是Finish就可以去链上查持仓了"

> **Q:** "链上有3-5秒延迟，做和pm套利的这个时间太久了"
> **A:** "这已经是最快的了，他上链本身就慢。等订单更新更慢"

**Key Takeaways:**

1. ⚠️ **DO NOT trust** WebSocket quantity data (`order.Shares`, `order.FilledShares`).
2. ✅ **Only trust** WebSocket for `status = "Finish"` signal.
3. ✅ **On-chain query** is the ONLY reliable source for actual fill amounts.
4. ⚠️ **On-chain confirmation** takes 3-5 seconds (unavoidable).
5. API order updates are slower than on-chain.

---

## 3. Critical Constraints & Parameters

### 3.1 Contract Addresses

| Contract | Address | Network |
| --- | --- | --- |
| **ConditionalTokens** (ERC-1155) | `0xAD1a38cEc043e70E83a3eC30443dB285ED10D774` | BNB Chain |
| **MultiSend** | `0x998739BFdAAdde7C933B942a68053933098f9EDa` | BNB Chain |

### 3.2 API Endpoints

| Service | URL |
| --- | --- |
| REST API | `https://proxy.opinion.trade:8443/openapi/` |
| WebSocket | `wss://ws.opinion.trade` |

### 3.3 Chain Parameters

| Parameter | Value |
| --- | --- |
| Chain ID | 56 (BNB Chain Mainnet) |
| Block Time | 0.75 seconds |
| Finality | ~1.875 seconds |
| On-chain Confirmation | 3-5 seconds |

### 3.4 API Limits

| Limit | Value |
| --- | --- |
| Rate Limit | 15 TPS (default) |
| Price Precision | 1 decimal (0.1 - 0.9) |

### 3.5 EIP-712 Constants

```rust
// ⚠️ CRITICAL: Note the SPACE in domain name!
const EIP712_DOMAIN_NAME: &str = "OPINION CTF Exchange";
const EIP712_DOMAIN_VERSION: &str = "1";
const CHAIN_ID: u64 = 56;
const VERIFYING_CONTRACT: &str = "0xAD1a38cEc043e70E83a3eC30443dB285ED10D774";

// Order Type Hash
const ORDER_TYPEHASH: &str = "Order(uint256 salt,address maker,address signer,address taker,uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint256 expiration,uint256 nonce,uint256 feeRateBps,uint8 side,uint8 signatureType)";

```

### 3.6 DO NOT List

* ❌ **DO NOT** use Header for API key - MUST use query parameter.
* ❌ **DO NOT** assume 2 or 3 decimal price - MUST use 1 decimal.
* ❌ **DO NOT** use `ethers-rs` - Use Alloy (10x faster).
* ❌ **DO NOT** block async I/O - agents.md rules require non-blocking (formerly CLAUDE.md).
* ❌ **DO NOT** exceed 15 TPS - API will reject.
* ❌ **DO NOT** trust WebSocket quantity data - Use on-chain query.

### 3.7 Address Configuration

Key addresses used throughout the system. Some are fetched dynamically at runtime.

| Address | Source | Purpose |
|---------|--------|---------|
| **ConditionalTokens Contract** | Hardcoded in code | ERC-1155 contract for position tokens |
| | `0xAD1a38cEc043e70E83a3eC30443dB285ED10D774` | Used for `balanceOf()` queries |
| **CTF Exchange Contract** | Fetched from API | Contract that verifies EIP-712 signatures |
| | Endpoint: `/quoteToken?chainId=56` | Used as `verifyingContract` in EIP-712 domain |
| | Field: `ctfExchangeAddress` | **NOT** the same as ConditionalTokens! |
| **Quote Token (USDT)** | Fetched from API | Base currency for trading |
| | Address: `0x55d398326f99059fF775485246999027B3197955` | Stablecoin on BNB Chain |
| **Proxy/Safe Address** | Fetched from API | Maker address for orders (Safe multi-sig) |
| | Endpoint: `/user/auth` | Used as `maker` field in orders |
| | Field: `walletUsers["56"]` | Required for order signing |
| **EOA Address** | From environment | Signer address for EIP-712 |
| | `OPINION_EOA_ADDRESS` | Used as `signer` field in orders |

#### Address Fetching Flow

```
1. On startup:
   ├─ Fetch quote tokens: GET /quoteToken?chainId=56
   │  └─ Extract: CTF Exchange address, USDT address
   │
   ├─ Fetch user auth: GET /user/auth
   │  └─ Extract: Proxy/Safe address for chain 56
   │
   └─ Cache addresses for runtime use

2. During trading:
   ├─ Use cached CTF Exchange for EIP-712 domain
   ├─ Use cached Proxy/Safe as order maker
   └─ Use EOA from environment as order signer
```

#### EIP-712 Domain Example

```rust
EIP712Domain {
    name: "OPINION CTF Exchange",  // ⚠️ Note the space!
    version: "1",
    chainId: 56,
    verifyingContract: <CTF Exchange address from /quoteToken>  // DYNAMIC!
}
```

**CRITICAL:** Do NOT hardcode the CTF Exchange address. Always fetch it from `/quoteToken` endpoint at runtime, as it may change.

#### Code Example

```rust
// Initialize client and fetch addresses
let client = OpinionClient::new(&config.api, config.environment)?;
client.init(config.chain.chain_id, &config.chain.quote_token).await?;

// Get cached addresses
let exchange_address = client.get_cached_exchange_address().await.unwrap();
let proxy_address = client.get_cached_proxy_address().await.unwrap();

println!("CTF Exchange: {}", exchange_address);
println!("Proxy/Safe: {}", proxy_address);
```

#### Key Points

1. **Dynamic Fetching**: CTF Exchange and Proxy addresses are fetched at startup, not hardcoded
2. **Caching**: Addresses are cached in `OpinionClient` after first fetch
3. **Separation**: ConditionalTokens (position tracking) ≠ CTF Exchange (order verification)
4. **Two Addresses per Order**: 
   - `maker`: Proxy/Safe address (from API)
   - `signer`: EOA address (from environment)
5. **Chain Specific**: Use `chainId=56` for BNB Chain when fetching

#### Troubleshooting

**Issue**: "Exchange address not found for quote token"
- **Solution**: Verify quote token address matches one returned by `/quoteToken`

**Issue**: "Wallet address mismatch"
- **Solution**: Ensure `OPINION_EOA_ADDRESS` matches the EOA that created the Proxy account

**Issue**: EIP-712 signature verification fails
- **Solution**: Confirm `verifyingContract` is CTF Exchange address, NOT ConditionalTokens

---

## 4. Architecture Design

### 4.1 Three-Plane Architecture

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                     OPINION TRADE HFT FRAMEWORK                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │                          DATA PLANE                                   │  │
│  │                                                                       │  │
│  │  ┌────────────────┐    ┌────────────────┐    ┌──────────────────┐     │  │
│  │  │ WebSocket      │    │ REST Poller    │    │ Chain Subscriber │     │  │
│  │  │ Client         │    │ (Fallback)     │    │ (Position Query) │     │  │
│  │  │                │    │                │    │                  │     │  │
│  │  │ • market.depth.diff  │    │ • /markets     │    │ • balanceOf()    │     │  │
│  │  │ • trade.order  │    │ • /orderbook   │    │ • event logs     │     │  │
│  │  │ • trade.record │    │                │    │                  │     │  │
│  │  └───────┬────────┘    └───────┬────────┘    └────────┬─────────┘     │  │
│  │          │                     │                      │               │  │
│  │          └─────────────────────┼──────────────────────┘               │  │
│  │                                ▼                                      │  │
│  │                    ┌───────────────────────┐                          │  │
│  │                    │      Event Bus        │                          │  │
│  │                    │  (tokio::broadcast)   │                          │  │
│  │                    └───────────┬───────────┘                          │  │
│  └────────────────────────────────┼─────────────────────────────────────┘   │
│                                   │                                         │
│  ┌────────────────────────────────┼─────────────────────────────────────┐   │
│  │                          CONTROL PLANE                               │   │
│  │                                ▼                                     │   │
│  │  ┌────────────────┐    ┌───────────────┐    ┌───────────────────┐    │   │
│  │  │ Strategy       │    │ Position      │    │ Risk Manager      │    │   │
│  │  │ Engine         │    │ Manager       │    │                   │    │   │
│  │  │                │    │               │    │ • Circuit Breaker │    │   │
│  │  │ (User impl)    │    │ • On-chain    │    │ • Max Position    │    │   │
│  │  │                │    │   positions   │    │ • Loss Limits     │    │   │
│  │  └───────┬────────┘    └───────────────┘    └───────────────────┘    │   │
│  │          │                                                           │   │
│  │          ▼ OrderIntent                                               │   │
│  └────────────────────────────────┼─────────────────────────────────────┘   │
│                                   │                                         │
│  ┌────────────────────────────────┼─────────────────────────────────────┐   │
│  │                         EXECUTION PLANE                              │   │
│  │                                ▼                                     │   │
│  │  ┌─────────────────────────────────────────────────────────────────┐ │   │
│  │  │                  Rate Limiter (Governor GCRA)                   │ │   │
│  │  │                  15 TPS Token Bucket + Burst Control            │ │   │
│  │  └──────────────────────────────┬──────────────────────────────────┘ │   │
│  │                                 │                                    │   │
│  │  ┌────────────┐   ┌─────────────▼────────────┐   ┌────────────────┐  │   │
│  │  │ EIP-712    │   │ Priority Queue           │   │ Order Gateway  │  │   │
│  │  │ Signer     │   │                          │   │                │  │   │
│  │  │            │   │ • Critical (stop-loss)   │   │ • REST Client  │  │   │
│  │  │ • Precomp  │   │ • High (arbitrage)       │   │ • Retry Logic  │  │   │
│  │  │   Domain   │   │ • Medium (strategy)      │   │ • Error Handle │  │   │
│  │  │ • k256     │   │ • Low (market making)    │   │                │  │   │
│  │  └────────────┘   └──────────────────────────┘   └───────┬────────┘  │   │
│  │                                                          │           │   │
│  │  ┌───────────────────────────────────────────────────────▼────────┐  │   │
│  │  │               Confirmation Tracker (Hybrid)                    │  │   │
│  │  │                                                                │  │   │
│  │  │  Trigger: WebSocket trade.order.update (status = "Finish")     │  │   │
│  │  │  ⚠️ DO NOT trust WebSocket quantity data!                     │  │   │
│  │  │                                                                │  │   │
│  │  │  Source of Truth: On-chain balanceOf() query                   │  │   │
│  │  │  Latency: 3-5 seconds (unavoidable chain confirmation)         │  │   │
│  │  └────────────────────────────────────────────────────────────────┘  │   │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘

```

**Architecture Explanation:**

* **Data Plane**: Ingests external data streams (WebSocket, REST, Blockchain). No dependencies on Control/Execution planes.
* **Control Plane**: Makes trading decisions (Strategy Engine, Position Manager, Risk Manager).
* **Execution Plane**: Executes orders with rate limiting and confirmation using `tokio::broadcast` for the Event Bus.

### 4.2 Order Confirmation Flow

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                      CONFIRMATION FLOW (Final Design)                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌──────────┐     ┌──────────────┐     ┌──────────────────────┐             │
│  │ Submit   │ ──▶ │ API returns  │ ──▶ │ Record:              │             │
│  │ Order    │     │ order_id     │     │ • order_id           │             │
│  │ (REST)   │     │              │     │ • token_id           │             │
│  └──────────┘     └──────────────┘     │ • position_before    │             │
│                                        └──────────┬───────────┘             │
│                                                   │                         │
│                                                   ▼                         │
│  ┌────────────────────────────────────────────────────────────────────┐     │
│  │            WebSocket: trade.order.update                           │     │
│  │            Wait for: status = "Finish"                             │     │
│  │                                                                    │     │
│  │  ⚠️ IGNORE: order.Shares, order.FilledShares (UNRELIABLE!)        │     │
│  │  ✅ ONLY USE: status field as trigger signal                       │     │
│  └────────────────────────────────┬───────────────────────────────────┘     │
│                                   │ status = "Finish"                       │
│                                   ▼                                         │
│  ┌────────────────────────────────────────────────────────────────────┐     │
│  │            On-Chain Query (ONLY RELIABLE SOURCE)                   │     │
│  │                                                                    │     │
│  │  Contract: ConditionalTokens (ERC-1155)                            │     │
│  │  Address:  0xAD1a38cEc043e70E83a3eC30443dB285ED10D774            │     │
│  │  Method:   balanceOf(account, positionId) → uint256                │     │
│  │                                                                    │     │
│  │  Latency: 3-5 seconds (chain confirmation, UNAVOIDABLE)            │     │
│  └────────────────────────────────┬───────────────────────────────────┘     │
│                                   │                                         │
│                                   ▼                                         │
│  ┌────────────────────────────────────────────────────────────────────┐     │
│  │            Calculate Actual Fill                                   │     │
│  │                                                                    │     │
│  │  position_after = balanceOf(account, tokenId)                      │     │
│  │                                                                    │     │
│  │  if side == BUY:                                                   │     │
│  │      actual_fill = position_after - position_before                │     │
│  │  else: // SELL                                                     │     │
│  │      actual_fill = position_before - position_after                │     │
│  └────────────────────────────────┬───────────────────────────────────┘     │
│                                   │                                         │
│                                   ▼                                         │
│  ┌────────────────────────────────────────────────────────────────────┐     │
│  │            Update Local State & Emit Callback                      │     │
│  └────────────────────────────────────────────────────────────────────┘     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘

```

### 4.3 Latency Budget

| Stage | Component | Target Latency | Optimization |
| --- | --- | --- | --- |
| 1. WebSocket Parse | JSON Parser | < 0.05 ms | `simd-json` |
| 2. Strategy Signal | User Logic | < 0.5 ms | Pre-computed caches |
| 3. Position Snapshot | Chain Client | 50-100 ms | RPC call (can cache) |
| 4. EIP-712 Sign | Signer | < 0.3 ms | Precomputed domain/typehash |
| 5. Rate Limiter | Governor | 0.000015 ms | GCRA (15ns) |
| 6. Priority Queue | Heap | < 0.1 ms | `BinaryHeap` + Mutex |
| 7. HTTP POST | Network | 30-80 ms | HTTP/2 connection pool |
| 8. API Processing | Exchange | 10-30 ms | (Uncontrollable) |
| 9. Chain Confirm | Blockchain | **3000-5000 ms** | ⚠️ UNAVOIDABLE |
| 10. WS Notification | Exchange | ~0 ms | Event push |
| 11. Chain Query | RPC | 50-100 ms | RPC call |

**Totals:**

* **Tick-to-Submit:** 80-350 ms
* **Tick-to-Confirm:** 3.1-5.5 seconds (Dominant factor)
* **Post-Fermi Upgrade (2026.01):** ~2-3 seconds

---

## 5. Module Specifications

### 5.1 Auth Module (`src/auth/`)

**Purpose:** Sign orders using EIP-712 typed data with maximum performance.

**Optimizations:**

1. Precompute Domain Separator at initialization.
2. Precompute Type Hash at compile time.
3. Use `k256` directly for ECDSA.

**Interface:**

```rust
pub trait OrderSigner: Send + Sync {
    fn sign_order(&self, order: &Order) -> Result<Signature, SignerError>;
    fn address(&self) -> Address;
}

pub struct Eip712Signer {
    domain_separator: B256,      // Precomputed
    order_type_hash: B256,       // Precomputed
    signing_key: SigningKey,
    address: Address,
}

```

### 5.2 Rate Limiter Module (`src/execution/rate_limiter.rs`)

**Purpose:** Enforce 15 TPS API limit using GCRA algorithm.

**Configuration:**

```rust
pub struct RateLimiterConfig {
    pub requests_per_second: u32,  // Default: 15
    pub burst_capacity: u32,       // Default: 3 (conservative)
}

```

### 5.3 Priority Queue Module (`src/execution/priority_queue.rs`)

**Levels:**

* `Critical` (4): Stop-loss, liquidation
* `High` (3): Arbitrage
* `Medium` (2): Strategy signals
* `Low` (1): Market making

**Rules:** Higher priority first -> FIFO within same priority -> Expired orders dropped.

### 5.4 WebSocket Client (`src/data/websocket/`)

**Channels & Trust:**

* `market.depth.diff` (outcome-based): **Trusted**
* `trade.order.update`: **Only trust status**
* `trade.record.new`: **⚠️ UNRELIABLE**

### 5.5 REST Client (`src/data/rest/`)

**TLS Fingerprinting (CRITICAL):**
Kong API Gateway uses JA3 TLS fingerprinting.

* **Solution:** Use `rrequest` crate with `Emulation::Chrome120`.
* **Environment Awareness:**
* `OPINION_ENV=local`: Use Proxy to bypass.
* `OPINION_ENV=aws`: Direct connection.



### 5.6 Chain Client (`src/chain/`)

**Methods:**

* `balanceOf(address account, uint256 id)`
* `balanceOfBatch(address[] accounts, uint256[] ids)`

**Retry Mechanism:** Exponential backoff (1s, 2s, 4s) for retriable errors (timeout, 503, 429).

### 5.6.1 RPC Failover Architecture

The `ChainClient` now supports automatic failover between primary and fallback RPC endpoints.

#### Configuration

| Environment Variable | Description | Default |
|---------------------|-------------|---------|
| `OPINION_RPC_URL_PRIMARY` | Primary RPC endpoint (NodeReal/QuickNode) | Falls back to `BSC_RPC_URL` |
| `OPINION_RPC_URL_FALLBACK` | Fallback RPC endpoint (public node) | None |
| `OPINION_BATCH_BUFFER_MS` | Batch query buffer window | 50ms |
| `OPINION_HEALTH_CHECK_INTERVAL` | Health check interval | 30s |

#### Failover Logic

```text
┌─────────────────┐
│ RPC Request     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐     healthy?     ┌─────────────────┐
│ Primary RPC     │────────YES──────▶│ Execute Request │
│ (NodeReal)      │                  └────────┬────────┘
└────────┬────────┘                           │
         │NO                                  ▼
         ▼                           ┌─────────────────┐
┌─────────────────┐                  │ Success?        │
│ Fallback RPC    │◀─────YES─────────┤                 │
│ (BSC Public)    │                  └────────┬────────┘
└────────┬────────┘                           │NO
         │                                    ▼
         ▼                           ┌─────────────────┐
┌─────────────────┐                  │ Retry (3x)      │
│ Execute Request │                  │ Exponential     │
└─────────────────┘                  │ Backoff         │
                                     └─────────────────┘
```

#### Health Check

- **Trigger**: When primary is marked unhealthy AND health check interval elapsed
- **Method**: Call `get_chain_id()` on primary provider
- **Recovery**: If chain ID matches 56 (BNB Chain), mark primary as healthy

#### Batch Query Optimization

The `ConfirmationTracker` now buffers balance queries:

1. When order finishes (WebSocket `status=Finish`), add to buffer
2. Wait up to 50ms OR until 10 orders accumulated
3. Call `balanceOfBatch` once for all pending orders
4. Process results and emit `ConfirmedOrder` events

**RPC Call Reduction**: 
- Before: 5 orders = 5 RPC calls
- After: 5 orders within 50ms = 1 RPC call
- Improvement: ~80% reduction in RPC calls during high activity

### 5.7 Confirmation Tracker (`src/execution/confirmation.rs`)

**Flow:**

1. Submit -> Record (`order_id`, `token_id`, `position_before`).
2. WS "Finish" -> Trigger chain query.
3. Calculate `actual_fill`.
4. Emit callback.

### 5.8 Order Gateway (`src/execution/gateway.rs`)

Orchestrates validation, snapshotting, signing, rate limiting, and submission. Handles 429/5xx retries.
Supports scan-only mode via `OPINION_SCAN_ONLY=true` to disable order submission while scanning.

### 5.9 Strategy Module (`src/strategy/`)

**Purpose:** Implement trading strategies for prediction markets.

**Components:**

#### 5.9.1 Strategy Core (Shared Kernel)

**Location:** `src/strategy/core/`

This is the shared kernel used by **both live trading and backtest/research** paths.
`src/strategy/arbitrage.rs` is now a thin facade that re-exports the core types.

```text
                    ┌──────────────────────────────┐
                    │   Strategy Core (Shared)     │
                    │  src/strategy/core/*         │
                    │  - detector / fees / risk    │
                    └──────────────┬───────────────┘
                                   │
                     ┌─────────────┴─────────────┐
                     ▼                           ▼
          ┌──────────────────────┐     ┌──────────────────────┐
          │ Live Engine          │     │ Backtest / Research  │
          │ - WS/REST orderbook  │     │ - Polars IO + replay │
          │ - Execution gateway  │     │ - SimState + latency │
          └──────────────────────┘     └──────────────────────┘
```

**Modules:**

- `core/detector.rs`: `ArbitrageDetector`, `ArbitrageOpportunity`, detection logic
- `core/fees.rs`: fee curve + fee quoting helpers
- `core/risk.rs`: lightweight risk rules used pre-execution
- `core/mod.rs`: shared types (`MarketInfo`, `OrderbookSide`, `TradeSide`, `MarketOrderbook`)

#### 5.9.2 Arbitrage Detector (`core/detector.rs`)

Detects and evaluates arbitrage opportunities across different market types.

**Supported Arbitrage Types:**

| Type | Condition | Strategy | Profit Source |
|------|-----------|----------|---------------|
| **Yes/No** | `yes_ask + no_ask < 1.0` | Buy both tokens | Guaranteed payout when one wins |
| **Neg Risk** | `sum(all_asks) < 1.0` | Buy all outcomes | Guaranteed payout (one must win) |
| **Mint-Sell** | `yes_bid + no_bid > 1.0` | Mint @ $1.0, sell both | Immediate spread profit |
| **Spread** | Wide bid-ask spread | Place orders inside spread | Capture spread + fees |

**Key Structures (authoritative):**

```rust
pub struct ArbitrageDetector {
    config: ArbitrageConfig,
    opportunity_counter: u64,
    stats: ArbScanStats,
}

pub struct ArbitrageOpportunity {
    pub id: String,
    pub arb_type: ArbitrageType,
    pub market_id: String,
    pub total_cost: f64,          // Unit cost (sum of prices)
    pub total_spend: f64,         // Total spend (max_size + fees)
    pub gross_profit_rate: f64,   // Profit % before fees
    pub net_profit_abs: f64,      // Net profit in USDT for max_size
    pub net_profit_rate: f64,     // Profit % after fees
    pub max_size: f64,           // Maximum executable size
    pub legs: Vec<ArbitrageLeg>, // Individual orders
    pub detected_at: u64,
    pub is_valid: bool,
}

pub struct ArbitrageLeg {
    pub pair: String,           // Trading pair (e.g., "BTC-YES")
    pub token_id: String,       // ERC-1155 token ID
    pub side: TradeSide,        // Buy or Sell
    pub price: f64,
    pub size: f64,
    pub available_liquidity: f64,
}
```

**Fee Calculation:**

Opinion uses a symmetric fee curve (implemented in `core/fees.rs`):
```
Fee Rate = topic_rate * price * (1 - price)
```

- Peaks at p = 0.5 (maximum uncertainty)
- Tapers to 0 at edges (p → 0 or p → 1)
- Applies referral discount (default 10%)
- $0.50 minimum fee per order

**Detection Logic:**
Note: `total_spend` was previously referred to as `total_cost` in older docs.

```rust
// Yes/No Example
yes_cost = yes_ask * size + yes_fee
no_cost = no_ask * size + no_fee
total_spend = yes_cost + no_cost

if total_spend < 1.0 * size {
    profit = (1.0 * size) - total_spend
    // Check if profit > min_threshold
}
```

#### 5.9.3 Market Scanner (`market_scanner.rs`)

Continuously monitors markets for opportunities.

**Functions:**
- Fetch active markets from API
- Track orderbook state for each market
- Calculate spreads and liquidity metrics
- Filter by volume, status_enum, and liquidity
- Paginate market list with `chain_id` and `sort_by` (volume24h desc)
- Cache results for scan efficiency; fallback to unfiltered market list if active count is low
- Fetch categorical market details via `/market/categorical/{id}` for multi-outcome arbitrage
- Subscribe to `market.depth.diff` with `topicId` and route updates by `outcome`

**Operational Notes:**
- Market list is paginated and cached (default refresh interval: 5 minutes).
- Active status is determined by `statusEnum == "activated"` (string-based).
- If `volume24h` is missing, total volume is used as a fallback for filtering.
- Market diagnostics log statusEnum and market_type distributions per refresh.
- Orderbooks use a WS-first + REST fallback model: if a local book is missing or stale, a REST snapshot seeds the cache and the scan proceeds.

**Key Structures:**

```rust
pub struct MarketInfo {
    pub id: String,
    pub question: String,
    pub yes_token_id: String,
    pub no_token_id: String,
    pub yes_pair: String,
    pub no_pair: String,
    pub volume_24h: f64,
    pub liquidity: f64,
    pub is_active: bool,
    pub market_type: Option<i32>,
    pub root_market_id: Option<String>,
    pub parent_title: Option<String>,
}

pub struct MarketOrderbook {
    pub pair: String,
    pub bids: Vec<OrderbookSide>,
    pub asks: Vec<OrderbookSide>,
}

impl MarketOrderbook {
    pub fn best_bid(&self) -> Option<f64> { /* ... */ }
    pub fn best_ask(&self) -> Option<f64> { /* ... */ }
    pub fn spread(&self) -> Option<f64> { /* ... */ }
    pub fn mid_price(&self) -> Option<f64> { /* ... */ }
}
```

#### 5.9.2.1 Local Orderbook Store (`orderbook_store.rs`)

Maintains an in-memory snapshot of orderbooks from WebSocket `market.depth.diff` updates.

**Responsibilities:**
- Apply single-level diff updates by `marketId` + `outcome` ("YES"/"NO") to the correct book.
- Parse WS payloads (array or object entries) into normalized bids/asks when snapshots are available.
- Keep last update timestamps to detect stale books.
- Provide fast access for the arbitrage scanner without REST polling.
- Accept REST snapshots when WS data is missing or stale (semi-trust mode).

#### 5.9.3 Strategy Diagnostics

**Purpose:** Explain why no opportunities are detected in live scans.

**Mechanism:**
- `ArbScanStats` tracks skip reasons per strategy (missing book, cost >= 1.0, low liquidity, below min notional, below min profit).
- The scan loop logs a per-cycle summary (markets scanned, orderbook errors, opportunities found).
- Use `RUST_LOG=debug` for detailed decision stats alongside the scan summary.

**Analysis Framework (Systematic Debugging):**
1. **Data Quality**: statusEnum correctness, missing token IDs, stale orderbooks, parse errors.
2. **Coverage**: pagination completeness, scan duration vs interval, API rate limiter saturation.
3. **Pricing & Fees**: 1-decimal tick size, min fee floor, min notional per leg, taker fee curve.
4. **Opportunity Math**: binary sum < 1.0 or > 1.0, categorical sum < 1.0, net profit after fees.
5. **Execution & Risk**: pre-trade validation, slippage buffer, confirmation latency.
6. **Observability**: per-cycle summaries + skip reason counts to pinpoint bottlenecks.

**One-off Market Sweep (metadata reconnaissance):**
- Binary: `src/bin/market_sweep.rs`
- Purpose: enumerate market IDs/metadata across API filters (no `chain_id`, market_type=1/2).
- Output: logs each market entry plus status/type distributions per query.
- If `OPINION_MASTER_MARKET_URL` is set, the sweep uses the master list (`result.list -> childList`) and logs each `topicId`.

#### 5.9.4 Scoreboard (`scoreboard.rs`)

Tracks strategy performance metrics.

**Metrics:**
- Total P&L
- Win rate
- Average profit per trade
- Execution time statistics
- Slippage tracking

#### 5.9.5 Market Rotator (`rotator.rs`)

Manages market selection and rotation.

**Features:**
- Prioritize markets by volume
- Rotate to new markets when conditions change
- Avoid over-concentrated positions
- Track market-specific performance

**Configuration:**

```rust
pub struct RotationConfig {
    pub max_markets: usize,           // Max markets to track
    pub min_volume: f64,              // Minimum 24h volume
    pub rotation_interval: Duration,   // How often to rotate
    pub performance_window: Duration,  // Lookback for performance
}
```

**Configuration Example:**

```rust
let config = ArbitrageConfig {
    min_profit_rate: 0.005,    // 0.5% minimum profit
    min_liquidity: 100.0,      // 100 units minimum
    min_order_notional: 1.30,  // Minimum order value
    min_fee: 0.50,             // Minimum fee per order
    topic_fee_rate: 0.02,      // 2% max fee
    referral_discount: 0.10,    // 10% rebate
    max_slippage: 0.01,        // 1% max slippage
};

let detector = ArbitrageDetector::new(config);
```

**Testing:**

See `tests/small_scale_arbitrage_test.rs` for live arbitrage detection tests.

### 5.9.6 Replay Engine (Offline)

**Purpose:** Validate strategy metrics and risk manager behavior using JSONL replay data without live API dependency.

**Core Types:**
- `ReplayTestClient` (JSONL playback)
- `ReplayEvent` (`init`, `trade`)
- `ReplayTrade` (price, quantity, fee)

**Usage Pattern:**
- Load JSONL via `ReplayTestClient::new_from_jsonl`
- Execute replay events to compute P&L, win rate, drawdown

**Tests:**
- `tests/live_order_tests.rs`: `test_sim_replay_metrics`, `test_sim_replay_metrics_profit`, `test_sim_replay_metrics_loss`, `test_sim_replay_risk_manager`

### 5.10 Telemetry Module (`src/telemetry.rs`)

**Purpose:** Provides observability metrics for monitoring bot performance and health.

**Components:**

#### 5.10.1 Logging (tracing)

Structured logging with stdout output and optional file output for production observability.

**Log Levels:**
- `ERROR`: System failures, critical errors
- `WARN`: Risk limit violations, connection issues
- `INFO`: Order submissions, confirmations, market events
- `DEBUG`: Detailed API responses, internal state changes
- `TRACE`: Very fine-grained execution details

**Output Architecture:**

```
Application Logs
      │
      ▼
┌─────────────┐   tracing_appender::non_blocking
│  tracing    │ ────────────────────────────────────▶
│  subscriber │   (Zero-copy channel to background thread)
└─────────────┘
      │
      └───────────────┬───────────────────────┐
                      │                       │
              ┌───────▼───────┐       ┌───────▼───────┐
              │   Stdout      │       │   File        │
              │   WorkerGuard │       │   WorkerGuard │
              │               │       │               │
              │ Compact format│       │ Full metadata │
              │ + colors      │       │ for debugging │
              └───────┬───────┘       └───────┬───────┘
                      │                       │
                      ▼                       ▼
                Terminal Output       logs/opinion-hft.log.
                                       YYYY-MM-DD (optional)
```

**Lifecycle Management:**

```rust
pub struct TelemetryGuards {
    pub stdout: WorkerGuard,  // Must be held for logging lifetime
    pub file: Option<WorkerGuard>,    // File guard when file logging is enabled
}

pub fn init_telemetry() -> TelemetryGuards {
    let (stdout_writer, stdout_guard) = tracing_appender::non_blocking(std::io::stdout());
    let file_appender = tracing_appender::rolling::daily("logs", "opinion-hft.log");
    let (file_writer, file_guard) = tracing_appender::non_blocking(file_appender);
    
    // Stdout: compact format for real-time monitoring
    let stdout_layer = fmt::layer()
        .with_writer(stdout_writer)
        .with_thread_ids(true)
        .with_target(false)
        .compact();
    
    // File: full metadata for production debugging
    let file_layer = fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false)
        .with_thread_ids(true)
        .with_target(true)
        .with_file(true)
        .with_line_number(true);
    
    Registry::default()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .try_init()?;
    
    TelemetryGuards { stdout: stdout_guard, file: Some(file_guard) }
}
```

**Important**: `WorkerGuard` must be held for the entire program lifetime. Dropping it early will stop logging. The main function returns `TelemetryGuards` to enforce this:

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let _guards = telemetry::init_telemetry();
    // Guards dropped automatically at scope end
}
```

**Log Rotation:**

- Pattern: `logs/opinion-hft.log.YYYY-MM-DD`
- Rotation: Daily at midnight (system timezone)
- Retention: Manual cleanup (no automatic deletion)
- Format: Structured text with timestamp, level, target, message

**Configuration:**

```bash
# File logging controls
export OPINION_LOG_FILE_ENABLED=true
export OPINION_LOG_DIR=logs

# Log level filtering (default: info)
export RUST_LOG=info,opinion_hft=debug

# Module-specific filtering
export RUST_LOG=opinion_hft::execution=debug,opinion_hft::strategy=trace
```

**Test Infrastructure:**

Integration tests use a helper function to initialize logging safely:

```rust
pub fn init_logs() {
    static INIT: Once = Once::new();
    static mut GUARDS: Option<TelemetryGuards> = None;
    unsafe {
        INIT.call_once(|| {
            let guards = telemetry::init_telemetry();
            GUARDS = Some(guards);
        });
    }
}
```

This pattern prevents duplicate initialization crashes in multi-threaded test environments.

**Log Format Example:**

```
2026-01-19T10:30:45.123Z INFO src/execution/gateway.rs:245 ThreadId(02) opinion_hft::execution::gateway: Order submitted successfully order_id="ord_abc123" market_id=12345 side="Buy" price="0.450" size="100.0"
```

**Key Events Logged:**
- Order lifecycle events (submit → fill → confirm)
- WebSocket connection state changes
- API rate limit warnings
- Risk manager alerts
- Arbitrage opportunity detection

#### 5.10.2 Metrics (planned)

Performance metrics for monitoring and alerting.

**Metric Categories:**

| Category | Metrics |
|----------|---------|
| **Trading** | Orders submitted, filled, cancelled; P&L; Win rate; Avg execution time |
| **API** | Request latency; Success rate; Rate limit hits; Error rate |
| **WebSocket** | Connection uptime; Messages received; Latency |
| **Strategy** | Opportunities detected; Opportunities executed; Avg profit per arb |
| **Risk** | Drawdown; Daily P&L; Circuit breaker triggers; Position exposure |

**Key Metrics (Examples):**

```
# Trading Metrics
opinion_hft_orders_total{status="submitted"} 1523
opinion_hft_orders_total{status="filled"} 1498
opinion_hft_pnl_total 45.23
opinion_hft_win_rate 0.847
opinion_hft_execution_time_ms 3200

# API Metrics
opinion_hft_api_latency_ms 45
opinion_hft_api_success_rate 0.999
opinion_hft_rate_limit_hits 12

# Strategy Metrics
opinion_hft_arbitrage_detected 234
opinion_hft_arbitrage_executed 189
opinion_hft_avg_arb_profit 0.0056

# Risk Metrics
opinion_hft_drawdown 0.023
opinion_hft_daily_pnl -2.34
opinion_hft_circuit_breaker_triggered 0
```

**Planned Integration:**
- Prometheus metrics endpoint (`/metrics`)
- Grafana dashboard templates
- Alert rules for critical thresholds

#### 5.10.3 Tracing (planned)

Distributed tracing for request lifecycle analysis.

**Trace Spans:**
- `order_execution`: From strategy signal to confirmation
- `api_request`: Individual REST API calls
- `websocket_message`: Message processing
- `chain_query`: On-chain balance queries

**Example Trace:**

```
┌─────────────────────────────────────────────────────────────┐
│ order_execution                                           │
│  ├─ strategy_signal_detection (0.5ms)                    │
│  ├─ order_validation (0.2ms)                              │
│  ├─ rate_limit_acquire (5ms)                              │
│  ├─ eip712_signing (0.3ms)                               │
│  ├─ api_post_order (45ms)                                 │
│  ├─ websocket_wait_finish (3200ms)                       │
│  └─ chain_query_balance (100ms)                           │
└─────────────────────────────────────────────────────────────┘
Total: 3351ms
```

**Planned Integration:**
- OpenTelemetry for tracing
- Jaeger/Tempo UI for visualization
- Correlation with metrics

#### 5.10.4 Health Checks

Endpoint for system health monitoring.

**Health Check Endpoint:**

```rust
pub async fn health_check() -> Result<HealthStatus> {
    Ok(HealthStatus {
        status: "healthy",
        uptime: calculate_uptime(),
        api_latency: check_api_latency().await?,
        websocket_connected: ws_connected(),
        chain_sync: check_chain_sync().await?,
        circuit_breaker_active: is_circuit_breaker_active(),
    })
}
```

**Response:**

```json
{
  "status": "healthy",
  "uptime": 86400,
  "checks": {
    "api": {
      "latency_ms": 45,
      "success_rate": 0.999
    },
    "websocket": {
      "connected": true,
      "last_message_age_ms": 100
    },
    "chain": {
      "synced": true,
      "latest_block": 45123456
    },
    "risk": {
      "circuit_breaker_active": false,
      "daily_pnl": 12.34,
      "drawdown_pct": 0.015
    }
  }
}
```

**Usage:**

```bash
curl http://localhost:8080/health
```

#### Configuration

Environment variables for telemetry:

```bash
# Logging
RUST_LOG=info,opinion_hft=debug

# Metrics (when implemented)
OPINION_METRICS_ENABLED=true
OPINION_METRICS_PORT=9090

# Tracing (when implemented)
OPINION_TRACING_ENABLED=true
OPINION_TRACING_ENDPOINT=http://jaeger:14268/api/traces
```

**Implementation Status:**

| Component | Status | Notes |
|-----------|--------|-------|
| Logging (tracing) | ✅ Implemented | Structured logging across all modules |
| Metrics | ⏳ Planned | Prometheus integration |
| Tracing | ⏳ Planned | OpenTelemetry spans |
| Health Check | ⏳ Planned | HTTP endpoint |

**See Also:**
- `src/telemetry.rs` for implementation details
- `agents.md` (formerly `CLAUDE.md`) for logging guidelines

---

## 6. API Compatibility Design

### 6.1 Order Submission Flow

The order submission process follows a strict sequence to ensure API compatibility with the Opinion CTF SDK specification:

1. **Calculate amounts using Fraction-based arithmetic**
   - Uses `Fraction` type from `num-traits` for exact price matching
   - Converts decimal prices to wei (18 decimals) with precision
   - Formula: `makerAmount = price * quantity`, `takerAmount = (1 - price) * quantity`

2. **Build Order struct with all required fields**
   - `salt`: Unique random value for order identification
   - `maker`: Proxy/Safe address (not EOA)
   - `signer`: EOA address that signs the order
   - `taker`: Zero address (0x0000...0000) for open orders
   - `tokenId`: Conditional token ID
   - `makerAmount`: Wei string (18 decimals)
   - `takerAmount`: Wei string (18 decimals)
   - `expiration`: Unix timestamp in seconds
   - `nonce`: Unique nonce for replay protection
   - `feeRateBps`: Fee rate in basis points
   - `side`: 0 (Buy) or 1 (Sell) as uint8
   - `signatureType`: 2 (PolyGnosisSafe) as uint8

3. **Sign using EIP-712 with CTF Exchange as verifyingContract**
   - Domain separator computed dynamically using fetched CTF Exchange address
   - Type hash matches SDK specification exactly
   - Signature returned as r, s, v components

4. **Submit to /order endpoint with correct payload structure**
   - POST with JSON body containing order and signature
   - All field names in camelCase (e.g., `makerAmount`, `takerAmount`)
   - Response includes `orderId` for tracking

### 6.2 EIP-712 Signing Process

**Domain:**
```rust
{
    "name": "OPINION CTF Exchange",  // ⚠️ NOTE THE SPACE!
    "version": "1",
    "chainId": 56,
    "verifyingContract": <dynamic CTF Exchange address from /quoteToken>
}
```

**Order Struct:**
```rust
Order(
    uint256 salt,
    address maker,
    address signer,
    address taker,
    uint256 tokenId,
    uint256 makerAmount,
    uint256 takerAmount,
    uint256 expiration,
    uint256 nonce,
    uint256 feeRateBps,
    uint8 side,
    uint8 signatureType
)
```

**Critical Requirements:**
- `verifyingContract` must be the CTF Exchange address, NOT ConditionalTokens
- Domain name includes the space: "OPINION CTF Exchange"
- All amounts are Wei strings (18 decimals)
- `signatureType` = 2 for PolyGnosisSafe signatures

### 6.3 Amount Calculation Strategy

The bot uses `Fraction` from `num-traits` for precise decimal arithmetic:

```rust
use num_traits::Float;

// Convert price to wei
let price_fraction = Fraction::new(price * 10, 10); // 1 decimal
let maker_amount = (price_fraction * U256::from(quantity)).to_u256().unwrap();
let taker_amount = U256::from(quantity) - maker_amount;
```

**Why Fraction-based?**
- Avoids floating-point precision errors
- Exact arithmetic for decimal conversions
- Consistent with SDK behavior
- Prevents slippage from rounding errors

### 6.4 Exchange Address Management

The CTF Exchange address is fetched dynamically from the `/quoteToken?chainId=56` endpoint:

```rust
pub async fn get_exchange_address(&self) -> Result<Address, Error> {
    let response: QuoteTokenResponse = self.client
        .get("/quoteToken")
        .query(&[("chainId", "56")])
        .send()
        .await?
        .json()
        .await?;
    
    Ok(response.ctf_exchange)
}
```

**Why dynamic fetching?**
- Exchange address may change over time
- Supports multiple chains with different addresses
- Follows SDK best practices
- Avoids hardcoded values that may become stale

### 6.5 Additional Order Fields

Beyond the basic order fields, the API requires these additional fields:

**Sign:**
- String representation of the EIP-712 signature
- Format: `0x{r}{s}{v}`
- Serialized using hex encoding

**SafeRate:**
- Safety rate parameter for order matching
- Typically set to `100` (100%)
- Controls slippage tolerance

**OrderExpTime:**
- Order expiration timestamp in seconds
- Must be greater than current time
- Recommended: `current_time + 3600` (1 hour)

### 6.6 Cancel Order

The cancel order endpoint uses `POST /order/cancel` with:

```json
{
    "orderId": "<order_id>"
}
```

**Important:**
- Field name is `orderId` (camelCase)
- HTTP method is POST (not DELETE)
- Body contains only the order ID string
- Response confirms cancellation

---

## 7. Code Examples

### 6.1 EIP-712 Signer

```rust
use alloy_primitives::{keccak256, Address, B256, U256};
use k256::ecdsa::{SigningKey, signature::Signer};

pub struct Eip712Signer {
    domain_separator: B256,
    order_type_hash: B256,
    signing_key: SigningKey,
    address: Address,
}

impl Eip712Signer {
    pub fn new(private_key: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(private_key.into()).unwrap();
        // ... Initialization code ...
        // Note: name_hash includes the space: "OPINION CTF Exchange"
    }

    pub fn sign_order(&self, order: &Order) -> Signature {
        // ... Implementation ...
    }
}

```

### 6.2 Rate Limiter

```rust
use governor::{Quota, RateLimiter, state::{InMemoryState, NotKeyed}};
use std::sync::Arc;

pub struct OpinionRateLimiter {
    inner: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
}

impl OpinionRateLimiter {
    pub fn new(rps: u32, burst: u32) -> Self {
        let quota = Quota::per_second(NonZeroU32::new(rps).unwrap())
            .allow_burst(NonZeroU32::new(burst).unwrap());
        Self { inner: Arc::new(RateLimiter::direct(quota)) }
    }
    
    pub async fn acquire(&self) {
        self.inner.until_ready().await;
    }
}

```

### 6.3 Confirmation Tracker Logic

```rust
impl ConfirmationTracker {
    pub async fn on_order_finish(&self, order_id: &str) {
        let pending = match self.pending.get(order_id) {
            Some(p) => p.clone(),
            None => return,
        };
        
        // Query on-chain position
        let position_after = self.chain_client
            .balance_of(&pending.token_id).await.unwrap_or(U256::ZERO);
        
        // Calculate actual fill
        let actual_fill = match pending.side {
            Side::Buy => position_after.saturating_sub(pending.position_before),
            Side::Sell => pending.position_before.saturating_sub(position_after),
        };
        
        self.pending.remove(order_id);
        // Emit callback...
    }
}

```

---

## 8. Project Structure

```text
├── Cargo.toml
├── README.md
├── agents.md               # Development guidelines + formatting standard
├── .env.example
├── docs/
│   ├── architecture.md       # Architecture and design decisions (formerly DESIGN.md)
│   └── QUICKSTART.md         # Quick start guide + verification notes
├── src/
│   ├── lib.rs                # Library root
│   ├── main.rs               # Bot entry point
│   ├── config.rs             # Configuration from env
```

**Key Design Decisions:**
1. **Three-Plane Architecture**: Data → Control → Execution
2. **Event-Driven**: `tokio::broadcast` for loose coupling
3. **Rate-Limited**: 15 TPS with GCRA algorithm
4. **Hybrid Confirmation**: WebSocket trigger + on-chain query
5. **Environment-Aware**: Local (proxy) vs AWS (direct) TLS handling


---

## 9. Dependencies

```toml
[dependencies]
# Async Runtime
tokio = { version = "1.35", features = ["full", "tracing"] }
futures = "0.3"

# Ethereum / EIP-712
alloy-primitives = "0.8"
alloy-sol-types = "0.8"
alloy = { version = "0.3", features = ["providers", "signers"] }
k256 = { version = "0.13", features = ["ecdsa"] }

# Rate Limiting
governor = "0.6"
nonzero_ext = "0.3"

# Networking
reqwest = { version = "0.12", features = ["json", "rustls-tls", "http2"] }
tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }
url = "2.5"

# Data Structures
dashmap = "6.0"
priority-queue = "2.0"

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Observability
tracing = "0.1"

```

---

## 10. Test Plan

### 9.1 Unit Tests

| Module | Test Cases | Priority |
| --- | --- | --- |
| `auth/eip712` | Domain separator, type hash, signature | P0 |
| `execution/rate_limiter` | TPS enforcement, burst, timing | P0 |
| `execution/priority_queue` | Ordering, expiration, concurrency | P0 |
| `data/websocket` | Message parsing, reconnection | P1 |
| `execution/confirmation` | Position diff calculation | P0 |

### 9.2 Acceptance Criteria

* ✅ Unit test coverage > 80%
* ✅ Replay tests pass (`test_sim_replay_*`)
* ✅ All integration tests pass
* ✅ No compiler warnings
* ✅ `clippy` passes with no warnings
* ✅ `cargo fmt` check passes

---

## 11. Implementation Order

1. **Phase 1: Foundation** (Types, Error, Config, Auth/EIP712)
2. **Phase 2: Core Execution** (Rate Limiter, Priority Queue, Conditional Tokens)
3. **Phase 3: External Communication** (REST Client, WebSocket Client, Chain Client)
4. **Phase 4: Integration** (Confirmation Tracker, Gateway, Integration Tests)
5. **Phase 5: Examples & Docs**

---

## 11.1 Opinion Trade Strategy Iteration Roadmap (SQLite-first)

Phase 0: Data Collector (no trading)

Note: current code still records orderbook snapshots into SQLite
(`orderbook_snapshots`, `orderbook_levels`) alongside confirmed orders. The
long-term target is to keep SQLite only for `confirmed_orders` once Parquet is
fully adopted.

Initialize SQLite (WAL) and record snapshots + confirmations (legacy; see note above).
Capture orderbook snapshots for YES/NO pairs with depth limit.
Persist confirmed orders for offline analysis.

SQLite schema (current)

`orderbook_snapshots` (market_id, pair, snapshot_ts, captured_at_ms)
`orderbook_levels` (snapshot_id, side, price, size, level)
`confirmed_orders` (order_id, market_id, token_id, side, expected_price, actual_fill, position_before, position_after, confirmation_time_ms, timestamp_ms)

Daily rotation

New file naming: `{base}-{YYYY-MM-DD}.sqlite` (base comes from `OPINION_DATA_DB_PATH`)
Rotation check: Before each insert, check if date changed vs current_db_date
If rotated: Close old connection, create new DB, init schema, apply pragmas

Env flags

`OPINION_DATA_ENABLED` (default true)
`OPINION_DATA_DB_PATH` (default `data/opinion-hft.sqlite`, base path with date suffix added)
`OPINION_DATA_WAL` (default true)
`OPINION_DATA_BATCH_SIZE` (default 200)
`OPINION_DATA_FLUSH_MS` (default 1000)
`OPINION_DATA_OB_DEPTH` (default 10)
`OPINION_DATA_QUEUE_CAP` (default 2000)

Phase 1: Offline Analysis

Run SQL/Pandas analysis to derive rulebook thresholds (spread, liquidity, fee-adjusted EV).
Produce an aggregated strategy taxonomy (no user-level profiling).

Phase 2: Backtest & Live Iteration

Replay from SQLite snapshots (or export to JSONL if needed) using ReplayTestClient.
Promote best-performing configs to `config.toml` and iterate.

Opinion Trade-specific strategy additions

Mint-Sell arbitrage: execute when sum(bids) > 1.0 after fees.
Neg-Risk arbitrage: buy all outcomes when sum(asks) < 1.0 after fees.

Risk guardrail addition

Max holding time: force exit if a position remains open beyond X minutes.

---

## 11.2 Strategy Learning Roadmap (Structured Data Lake)

Goal: enable a fast, reliable research loop without adding latency to execution.
Avoid parsing human logs (`journalctl`) for analytics; emit structured data alongside logs.

Priority order (current):
- P0: Stage A (Local Structured Recorder)
- P1: Stage C (Research Backend)
- P2: Strategy iteration driven by Stage C results (see 11.1 Phase 1/2)
- P3: Stage B (S3 Pipeline)
- P4: Stage D/E (Optimistic Execution + Feature Layer)

Rationale: without structured data, strategy work is guesswork. Capture data first, then analyze, then iterate.
Strategic note: external limits (API TPS, chain confirmation) dominate latency. Avoid low-ROI micro-optimizations (e.g., swapping JSON libs) until data capture and analysis are in place.

### Stage A: Local Structured Recorder (Parquet sidecar)

Intent: keep SQLite for small, transactional data; move high-volume market snapshots to columnar files.

- Keep SQLite for `confirmed_orders` (small, high-value, transactional).
- Orderbook snapshot tables still exist in SQLite today; remove once Parquet is the sole snapshot store.
- Add a `ParquetRecorder` that receives data via `tokio::mpsc` to avoid blocking the hot path.
- Batch + rotate files: flush every N records or T seconds, rotate hourly or by size.
- File layout example (Hive partitioning): `data/parquet/orderbook_snapshots_v1/date=YYYY-MM-DD/hour=HH/part-000.parquet`.
- Do not parse `journalctl` for machine learning; use Parquet + schema.
- Store bids/asks as nested lists inside the snapshot row to avoid joins.

Stage A detailed design (P0):

Scope:
- Capture orderbook snapshots from REST refresh (high-quality baseline).
- Capture scan stats per scan cycle (why opportunities were skipped).
- Keep confirmed orders in SQLite (already implemented).
- Defer per-opportunity skip reasons until after the first data pass.

Data contracts (Parquet datasets):
- `orderbook_snapshots` (full depth on REST refresh, nested bids/asks):
  - `snapshot_id` (u64), `market_id` (string), `pair` (string), `outcome` (string)
  - `source` (string: "rest"), `exchange_ts_us` (i64, optional), `local_recv_ts_us` (i64), `local_write_ts_us` (i64)
  - `best_bid` (f64), `best_ask` (f64), `spread` (f64)
  - `bids` (list<struct<p: f64, s: f64>>), `asks` (list<struct<p: f64, s: f64>>)
  - `p`/`s` keep nested field metadata compact; document mapping in code.
  - List order defines depth (index 0 = best).
- `scan_stats` (one row per scan cycle):
  - `scan_ts_us` (i64), `duration_us` (u64), `markets_scanned` (u64)
  - `yesno_checked`/`yesno_found`/`yesno_missing_ask`/`yesno_cost_ge_one`
  - `yesno_low_liquidity`/`yesno_below_min_notional`/`yesno_below_min_profit`
  - `mint_sell_checked`/`mint_sell_found`/`mint_sell_missing_bid`/`mint_sell_revenue_le_one`
  - `mint_sell_low_liquidity`/`mint_sell_below_min_notional`/`mint_sell_below_min_profit`
  - `neg_risk_checked`/`neg_risk_found`/`neg_risk_missing_ask`/`neg_risk_cost_ge_one`
  - `neg_risk_low_liquidity`/`neg_risk_below_min_notional`/`neg_risk_below_min_profit`

MVP success criteria:
- Parquet files produced without blocking the scan loop.
- REST refresh snapshots captured with full depth (configurable).
- Scan stats recorded every scan cycle.
- No changes to execution logic.

Snapshot ID:
- Use `local_recv_ts_us` + a per-process atomic counter to ensure uniqueness.

Timestamp policy:
- Use microseconds for local timestamps to preserve ordering and internal jitter analysis.
- Store exchange-provided timestamps when available (convert ms to us).

Writer design:
- Single background writer thread per dataset; no disk I/O or compression on the trading loop.
- Run the writer in `std::thread` or `tokio::task::spawn_blocking` to avoid blocking Tokio workers.
- Use `try_send` or drop-on-full policy to avoid blocking; increment `parquet_dropped_total` atomic counter.
- Log a heartbeat every minute with the dropped count; alert if non-zero.
- Batch flush by size and time; close file on rotate, open new writer.
- Parse price/size strings inside the writer task (not on the hot path). Invalid values are skipped and counted.
- Optional: pin the writer thread to a dedicated core only after measuring contention.

Rotation policy:
- Rotate hourly or when file exceeds a size threshold (default 100MB).
- Directory structure uses Hive partitioning: `.../date=YYYY-MM-DD/hour=HH/`.
- File name uses `part-000.parquet` with a monotonically increasing part index per partition.
- Store `schema_version`, `git_commit`, `config_hash` as Parquet file metadata.

Sampling policy:
- Always record REST refresh snapshots (high quality).
- Optional: add top-of-book snapshots from local WS book (later if needed).

Config (new env keys):
- `OPINION_PARQUET_ENABLED` (default false)
- `OPINION_PARQUET_DIR` (default `data/parquet`)
- `OPINION_PARQUET_BATCH_SIZE` (default 1000)
- `OPINION_PARQUET_FLUSH_MS` (default 1000)
- `OPINION_PARQUET_ROTATE_MB` (default 100)
- `OPINION_PARQUET_COMPRESSION` (default `zstd`)
- `OPINION_PARQUET_QUEUE_CAP` (default 4096)
- `OPINION_PARQUET_HEARTBEAT_SECS` (default 60)

Parquet implementation choice (current decision):
- Primary: `polars` crate with `parquet` feature (columnar IO + compression).
- Reason: higher DX, robust Parquet defaults (row groups, stats, encoding), still Arrow-compatible.
- Constraint: project Rust version is 1.90; keep `polars` features minimal (`default-features = false`).

Schema versioning approach:
- Define a constant `PARQUET_SCHEMA_VERSION` (e.g., "v1.0.0").
- Embed metadata keys: `schema_version`, `schema_hash`, `git_commit`, `config_hash`.
- Dataset directory includes major version: `data/parquet/orderbook_snapshots_v1/date=YYYY-MM-DD/hour=HH/part-000.parquet`.
- Backward compatible changes: add columns only, bump minor.
- Incompatible changes: remove/rename columns, bump major and write to a new file prefix.

Integration points:
- `src/main.rs`: spawn parquet recorder next to SQLite recorder.
- On REST orderbook refresh in `MarketScanner`, call `parquet.record_orderbook(...)`.
- After each scan cycle, record `ArbScanStats` (already available via `detector.drain_stats()`).

Validation/tests:
- Schema test: write and read a small Parquet file, confirm column types and nested list layout.
- Rotation test: verify file roll-over on size/time.
- Drop policy test: ensure trading loop never blocks when queue is full and dropped counter increments.
- Local verification: see `docs/QUICKSTART.md` (Parquet verification section).

Implementation notes:
- Parquet is columnar (fast analytics, high compression); SQLite is row-oriented (good for small OLTP tables).
- Never write to disk on the trading loop. Use a background writer.
- Nested list assembly in Polars is still verbose; keep conversion in the writer thread and test the schema early.
- Buffer guideline: 1000 records or 1 second per flush (tune later).
- Rotation guideline: hourly or 100MB per file to keep analysis chunks small.

Example event (analysis-ready):

```rust
struct StrategySnapshot {
    ts_us: i64,
    market_id: String,
    outcome: String,
    best_bid: f64,
    best_ask: f64,
    spread: f64,
    decision: bool,
    latency_us: u64,
}
```

### Stage B: Data Pipeline (EC2 -> S3)

Intent: decouple trading node from data ingestion.

- Use a sidecar uploader (cron, systemd timer, or Filebeat/Vector).
- Watch rotated Parquet files, upload to S3, then delete locally.
- Keep the trading process unaware of S3 credentials.

Minimal pipeline (cron or systemd timer):

```bash
# Upload completed files (example: move all rotated files, keep current hour local)
aws s3 mv "data/parquet/orderbook_snapshots_v1/" "s3://your-bucket/orderbook_snapshots_v1/" --exclude "*" --include "date=*/hour=*/*.parquet"
```

### Stage C: Research Backend (offline)

Intent: convert recorded data into validated strategy parameters without touching production.

#### Decision (Tooling)

**Primary stack: Pure Rust + Polars (Lazy API)**

Rationale:
- Avoids implementation drift by reusing production logic (fees, thresholds, arbitrage detection).
- Keeps a single language/toolchain (Rust) and removes Python env overhead.
- Polars Lazy API provides predicate/projection pushdown and streaming execution for large Parquet datasets.

**Hybrid execution model (mandatory):**
- Use Polars for IO + stateless filtering/aggregation.
- Use Rust iterators for stateful simulation (latency, position lock, risk rules).
- Do **not** attempt to encode stateful trading logic in Polars expressions.

```text
Parquet (Stage A)            Stage C Pipeline (Offline)
┌────────────────────┐       ┌────────────────────────────────────────────┐
│ orderbook snapshots │──────▶│ Polars Lazy (filter/scan/partition prune) │
│ scan stats          │       └───────────────┬────────────────────────────┘
└────────────────────┘                       │
                                             ▼
                              ┌───────────────────────────────────┐
                              │ Rust Iterator Replay (Stateful)   │
                              │ - SimState (balance/locks)        │
                              │ - latency + risk rules            │
                              │ - reuse Strategy Core             │
                              └───────────────┬───────────────────┘
                                              ▼
                              ┌───────────────────────────────────┐
                              │ Reports / Params / Diagnostics    │
                              └───────────────────────────────────┘
```

**Optional:** DuckDB for ad-hoc SQL exploration (not required for the core workflow).

#### Inputs

- Parquet datasets produced by Stage A (Hive partitioning):
  - `orderbook_snapshots_v1/date=YYYY-MM-DD/hour=HH/part-*.parquet`
  - `scan_stats_v1/date=YYYY-MM-DD/hour=HH/part-*.parquet`
- SQLite `confirmed_orders` (authoritative fills) for PnL attribution.

#### Core Components

1. **Data Preparation (Polars, stateless)**
   - Use `polars::LazyFrame::scan_parquet()` with glob patterns.
   - Enable streaming for datasets larger than RAM.
   - Partition pruning via Hive directory structure (`date=`, `hour=`).
   - Apply stateless filters (spread, liquidity, min notional) and sort by timestamp.

2. **Data Cleaning & Normalization**
   - Enforce price precision: round to 1 decimal (0.1 - 0.9).
   - Reject rows that violate minimum notional (use runtime config default).
   - Strictly reuse fee calculation from `strategy/core/fees.rs` to avoid drift.

3. **Stateful Simulation (Rust iterator)**
   - Partition by `market_id` and simulate each market independently.
   - Use Rayon to parallelize across markets.
   - Maintain per-market state (position, cash, `unlock_ts`).
   - **Blind exposure:** if `ts < unlock_ts`, skip new entries.

4. **Descriptive Research (Market Microstructure Report)**
   - Liquidity availability: % of time top-of-book is non-empty.
   - Spread distribution (best_ask - best_bid) and tail risk.
   - Slippage curves: cost to buy/sell 100/500/1000 shares.
   - Opportunity frequency: how often `yes_ask + no_ask < 1.0 - fee` occurs.

5. **Offline Replay & Fill Simulation**
   - Replay snapshots in time order using a conservative fill simulator.
   - **Latency modeling:** simulate order arrival at T + 200ms (configurable).
   - **State reconstruction:** execute against the snapshot at arrival time, not signal time.
   - **Blind exposure:** account for 3-5s confirmation lock before funds are reusable.
   - Output `potential_trades.csv` with `trigger_time`, `market_id`, `expected_profit`, `duration_ms`.

#### Outputs

- Parameter updates for `config.toml` (spread thresholds, min liquidity, min profit).
- CSV/Parquet reports for iteration (not runtime overrides).

#### Guardrails

- No Python re-implementation of core strategy logic.
- No direct changes to production behavior from research runs.
- Keep backtest logic deterministic and reproducible (fixed seed + fixed snapshot order).

#### Stateful Model (SimState)

Define an explicit minimal virtual-exchange state to avoid logic drift and to express lock/confirmation.

```rust
struct SimState {
    // Capital state
    virtual_balance: f64,
    virtual_inventory: f64,

    // Blind exposure
    unlock_ts_us: i64,
    pending_txs: Vec<PendingTx>,

    // Metrics
    total_trades: u64,
    winning_trades: u64,
    max_drawdown: f64,
}

struct PendingTx {
    submitted_at_us: i64,
    expected_confirm_at_us: i64, // submitted_at + 3000-5000ms
    cost: f64,
}
```

#### Simulation Loop (Minimal Virtual Exchange)

1. **Advance time:** iterate ticks in time order.
2. **Confirm pending:** release funds when `current_ts >= expected_confirm_at_us`.
3. **Check lock:** if `current_ts < unlock_ts_us`, skip signal evaluation.
4. **Evaluate signal:** reuse production logic for detection + fee model.
5. **Submit trade:** create `PendingTx`, update `unlock_ts_us`, record trade.

This “stateful core” must remain in Rust, not Polars.

#### Latency Modeling (MVP)

Prefer **Next-Tick Execution** rather than a complex time buffer:
- Signal at tick `T`.
- Execute at tick `T+1` using the new snapshot (more conservative).
- If price worsened, treat as slippage loss; if condition no longer holds, skip.

This approximates 200ms reaction latency while matching snapshot granularity.

#### Suggested Deliverables

- `src/bin/research.rs` (or `src/bin/backtest.rs`) CLI with:
  - `--date-range`, `--market-id`, `--min-profit`, `--output`
  - `--latency-ms`, `--streaming`, `--report`
- `reports/` output folder with summary CSVs and aggregate metrics.

#### Optional Visualization

- Integrate `rerun` to visualize time series (best_bid, best_ask, signals, pnl) during replay.
- Log **state changes** only (trade executions, lock/unlock, pnl updates) to reduce noise.
- Goal: fast inspection of “why no trade” at specific timestamps without Python.

#### Scalability & Evolution

- Parquet is the storage contract; compute is decoupled from storage.
- Scale-up path: larger single machine (more CPU/RAM/IO) without code changes.
- Scale-out path (future): Iceberg metadata + Spark/Trino/other distributed engines.
- DuckDB remains optional for ad-hoc SQL; it is not required for an Iceberg migration.

#### Implementation Checklist (Stage C)

1. Confirm Stage A Parquet recorder writes `local_recv_ts_us` (time base for replay).
2. Add `src/bin/research.rs` CLI (Polars load → Rust iterator sim → reports).
3. Reuse `opinion_hft::strategy` logic; do not re-implement in research.
4. Implement `SimState` and pending confirmation flow.
5. (Optional) Add Rerun logging for trade points + PnL curve.

#### Example (Polars Lazy API)

```rust
let df = LazyFrame::scan_parquet(
        "data/parquet/orderbook_snapshots_v1/date=*/hour=*/*.parquet",
        Default::default(),
    )?
    .filter(col("spread").gt(lit(0.05)))
    .select([
        col("market_id"),
        col("outcome"),
        col("best_bid"),
        col("best_ask"),
    ])
    .with_streaming(true)
    .collect()?;
```

### Stage D: Execution Evolution (Optimistic Execution)

Intent: reduce idle time during on-chain confirmation delays.

- Maintain a "virtual balance" updated on WS `status="Finish"`.
- Trade on virtual balance immediately.
- Reconcile with on-chain `balanceOf` after 3-5s.
- On mismatch, emit a reconciliation event and apply risk controls.

Guardrails:
- Delay this stage until the bot has been stable for weeks and you understand reorg risk.
- Treat reconciliation mismatches as high-severity risk events.

### Stage E: Feature Layer for Strategy Learning

Intent: separate feature engineering from execution logic.

- Add a `FeatureExtractor` that converts orderbook snapshots to `MarketFeatures`.
- Record `(MarketFeatures, Decision, Outcome)` tuples to Parquet.
- Keep strategy logic consuming features, not raw orderbooks.

Example features:

```rust
struct MarketFeatures {
    spread_bps: f64,
    imbalance: f64,
    volatility_1m: f64,
    implied_probability: f64,
}
```

Deferral guidance:
- Only consider this after you have weeks of clean Parquet data.
- Early-stage rule-based strategies are easier to debug and often outperform premature ML.

### Additional Roadmap Items (Reliability & Reproducibility)

- Data lineage + schema versioning: record `git_commit`, `config_hash`, and `schema_version` in both SQLite and Parquet outputs.
- Orderbook integrity: track WS sequence/gaps and periodically resync with REST snapshots to avoid stale books.
- Adaptive rate limiting: dynamic backoff based on 429/5xx rates; record rate-limit stats as metrics.
- Replay harness parity: deterministic replay that consumes the same event bus as live (WS + REST snapshots).
- Health/kill-switch hardening: "collect-only" mode and alerts on WS staleness, RPC failover, or balance reconciliation mismatches.
- Allocator evaluation (optional): consider jemalloc/mimalloc only if profiling shows allocator contention; keep it feature-gated and opt-in.

Non-goals for this phase:
- No direct ML inference in the hot path.
- No S3 upload from the trading process.
---

## 12. New Session Prompt

Copy and paste the following block to start a new implementation session:

> # Opinion Trade HFT Framework - Implementation Request
> 
> 
> ## Context
> 
> 
> I have a complete design document for an Opinion Trade HFT framework in Rust. The design has been reviewed and approved. Now I need to implement it.
> ## Project: opinion-hft
> 
> 
> A high-frequency trading framework for Opinion Trade prediction market on BNB Chain.
> ## Key Requirements
> 
> 
> 1. **Code and comments in English**
> 2. **Test-driven development** - All modules must pass tests before approval
> 3. **Use multiple AI agents for parallel implementation**
> 
> 
> ## Critical Parameters (MUST USE EXACTLY)
> 
> 
> * **EIP-712 Domain Name**: `"OPINION CTF Exchange"` (note the SPACE!)
> * **EIP-712 Version**: `"1"`
> * **Chain ID**: `56` (BNB Chain)
> * **ConditionalTokens Address**: `0xAD1a38cEc043e70E83a3eC30443dB285ED10D774`
> * **REST API Base**: `https://proxy.opinion.trade:8443/openapi/`
> * **WebSocket**: `wss://ws.opinion.trade`
> * **API Key**: Query parameter (NOT header!) `?apikey={KEY}`
> * **Rate Limit**: 15 TPS
> * **Price Precision**: 1 decimal (0.1 - 0.9)
> 
> 
> ## Critical Design Decision
> 
> 
> **Order Confirmation**:
> * ⚠️ DO NOT trust WebSocket quantity data (Shares, FilledShares are UNRELIABLE)
> * ✅ Only use WebSocket `status = "Finish"` as trigger
> * ✅ Query on-chain `balanceOf()` for actual fill amounts
> * On-chain confirmation takes 3-5 seconds (unavoidable)
> 
> 
> ## Implementation Order
> 
> 
> **Phase 1: Foundation**
> * types/order.rs, types/market.rs
> * error.rs, config.rs
> * auth/eip712.rs + tests
> 
> 
> **Phase 2: Core Execution**
> * execution/rate_limiter.rs + tests (Governor GCRA, 15 TPS)
> * execution/priority_queue.rs + tests
> * chain/conditional_tokens.rs + tests
> 
> 
> **Phase 3: External Communication**
> * data/rest/client.rs + tests
> * data/websocket/client.rs + tests
> * chain/client.rs + tests
> 
> 
> **Phase 4: Integration**
> * execution/confirmation.rs + tests
> * execution/gateway.rs + tests
> * Integration tests
> 
> 
> ## Start Implementation
> 
> 
> Please begin with **Phase 1**. Create the project structure and implement the foundation modules with tests. Each module must pass tests before moving to the next.
