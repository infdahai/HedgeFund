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
13. [CI/CD Pipeline](https://www.google.com/search?q=%2312-cicd-pipeline)
14. [New Session Prompt](https://www.google.com/search?q=%2313-new-session-prompt)

---

# Architecture Overview

### System Architecture Diagram

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                       OPINION TRADE HFT FRAMEWORK                               │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  ┌───────────────────────────────────────────────────────────────────────────┐  │
│  │                              DATA PLANE                                   │  │
│  │                                                                           │  │
│  │   ┌─────────────┐   ┌─────────────┐   ┌─────────────────────────────┐    │  │
│  │   │  WebSocket  │   │ REST Client │   │       Chain Client          │    │  │
│  │   │   Client    │   │             │   │       (Alloy)               │    │  │
│  │   │             │   │             │   │                             │    │  │
│  │   │ market.depth│   │ /markets    │   │ balanceOf()                 │    │  │
│  │   │ order.update│   │ /orderbook  │   │ balanceOfBatch()            │    │  │
│  │   │ trade.record│   │ /order      │   │                             │    │  │
│  │   │             │   │ ?apikey=XXX │   │ ConditionalTokens (ERC-1155)│    │  │
│  │   └──────┬──────┘   └──────┬──────┘   └──────────────┬──────────────┘    │  │
│  │          │                 │                          │                    │  │
│  │          └─────────────────┼──────────────────────────┘                    │  │
│  │                            ▼                                               │  │
│  │                  ┌─────────────────────┐                                   │  │
│  │                  │      Event Bus      │                                   │  │
│  │                  │  (tokio::broadcast) │                                   │  │
│  │                  └─────────────────────┘                                   │  │
│  └───────────────────────────────────┬───────────────────────────────────────┘  │
│                                      ▼                                           │
│  ┌───────────────────────────────────────────────────────────────────────────┐  │
│  │                            EXECUTION PLANE                                │  │
│  │                                                                           │  │
│  │   ┌─────────────┐   ┌─────────────┐   ┌─────────────────────────────┐    │  │
│  │   │   EIP-712   │   │Priority Queue│   │      Rate Limiter           │    │  │
│  │   │   Signer    │   │             │   │      (Governor GCRA)         │    │  │
│  │   │             │   │             │   │                             │    │  │
│  │   │ Precomputed:│   │Critical ───▶│   │                             │    │  │
│  │   │  • Domain   │   │High     ───▶│   │  15 TPS                      │    │  │
│  │   │  • TypeHash │   │Medium   ───▶│   │  Burst: 3                    │    │  │
│  │   │  k256 ECDSA │   │Low      ───▶│   │  ~15ns/check                 │    │  │
│  │   │             │   │FIFO per lvl │   │  until_ready().await         │    │  │
│  │   └─────────────┘   └──────┬──────┘   └─────────────────────────────┘    │  │
│  │                           │                                               │  │
│  │                           ▼                                               │  │
│  │           ┌─────────────────────────────────────────────┐                 │  │
│  │           │          Confirmation Tracker               │                 │  │
│  │           │                                             │                 │  │
│  │           │  Mode: TrustWebSocket | OnChainOnly |       │                 │  │
│  │           │       Hybrid (recommended)                  │                 │  │
│  │           │                                             │                 │  │
│  │           │  1. Record position_before                  │                 │  │
│  │           │  2. Wait for WS status="Finish"             │                 │  │
│  │           │  3. Query on-chain balanceOf()              │                 │  │
│  │           │  4. Calculate: position_after - before      │                 │  │
│  │           │  5. Emit ConfirmationEvent                  │                 │  │
│  │           └─────────────────────────────────────────────┘                 │  │
│  └───────────────────────────────────────────────────────────────────────────┘  │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘

```

### Module Interactions

```text
┌─────────────┐   OrderIntent    ┌─────────────────┐
│   Strategy  │ ───────────────▶ │  Priority Queue │
│   (User)    │                  │                 │
└─────────────┘                  └────────┬────────┘
                                           │
                                           ▼
┌─────────────┐                  ┌─────────────────┐
│  EIP-712    │ ◀── sign_order ─ │  Order Gateway  │
│  Signer     │                  │  (orchestrator) │
└─────────────┘                  └────────┬────────┘
                                           │
         ┌──────────┬──────────┬──────────┴──────────┬──────────┬──────────┐
         ▼          ▼          ▼                     ▼          ▼          ▼
  ┌───────────┐ ┌─────────┐ ┌──────────────────┐ ┌─────────┐ ┌─────────┐       │
  │   Rate    │ │   REST  │ │   Confirmation   │ │    WS   │ │  Chain  │       │
  │  Limiter  │ │  Client │ │     Tracker      │ │  Client │ │  Client │       │
  │  (wait)   │ │POST/order│ │                  │ │ (Finish)│ │balanceOf│       │
  └─────┬─────┘ └────┬────┘ └─────────┬──────────┘ └────┬────┘ └────┬────┘       │
        │            │                │                  │            │            │
        │            └────────────────┼──────────────────┘            │            │
        │                             │                              │            │
        ▼                             ▼                              │            │
  ┌───────────┐              ┌─────────────────┐                     │            │
  │           │              │  Confirmation   │◀────────────────────┘            │
  │           │              │     Event       │                                  │
  │           │              └─────────────────┘                                  │
  │           │                                                                    │
  └─────┬─────┘                                                                    │
        ▼                                                                          ▼
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

Runtime diagnostics are captured via `ArbScanStats` and scan-cycle summaries. The detailed
diagnostics framework and `scan_stats` dataset are specified in Section 11.2 (Stage A) to keep
the authoritative spec in one place.

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

See Section 11.1 (Phase 2) for ReplayTestClient usage and tests; the full research loop is in
Section 11.2.

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

**Block Reorganization (Reorg) Handling** [Expert Recommendation]

BNB Chain occasionally experiences block reorganizations where confirmed transactions may be reverted. This creates data consistency risks.

**Problem**: If `confirmed_orders` records a fill, but the block is later reorged, the SQLite data becomes inconsistent with on-chain state.

**Mitigation Strategy**:

1. **Schema Enhancement**: Add `block_number` and `tx_hash` to `confirmed_orders`:
   ```sql
   ALTER TABLE confirmed_orders ADD COLUMN block_number INTEGER;
   ALTER TABLE confirmed_orders ADD COLUMN tx_hash TEXT;
   ALTER TABLE confirmed_orders ADD COLUMN reorg_checked BOOLEAN DEFAULT FALSE;
   ```

2. **Periodic Validation** (P2 priority, post-MVP):
   - Background task checks recent `confirmed_orders` (last 100 blocks) against on-chain state
   - Query `balanceOf()` for each token and compare with recorded positions
   - Flag mismatches with `reorg_detected` timestamp
   - Alert on any discrepancy

3. **Safe Confirmation Depth**:
   - BNB Chain: Wait for 15+ block confirmations before considering final
   - Current 3-5s delay (~10-15 blocks) is marginal; consider increasing for high-value trades

**Implementation Priority**: P2 (after core strategy iteration works). Reorgs are rare on BNB Chain, but the schema fields should be added now for future-proofing.

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

Replay Engine (Offline)

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

Opinion Trade-specific strategy additions

Mint-Sell arbitrage: execute when sum(bids) > 1.0 after fees.
Neg-Risk arbitrage: buy all outcomes when sum(asks) < 1.0 after fees.

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

### 11.2.0 Next Steps: Implementation Order (CRITICAL)

This section defines the **exact execution order** for remaining work. Follow this sequence to maximize value and minimize risk.

#### Current Implementation Status

| Component | Design | Code | Tests | Status |
|-----------|--------|------|-------|--------|
| **Stage A: Parquet Recorder** | ✅ | ✅ | ✅ | Complete (incl. best-overpay columns) |
| **Stage C: Research CLI** | ✅ | ✅ | ✅ | Complete (gas/fill/resolution lock) |
| **Section 12: CI/CD** | ✅ | ✅ | ✅ | Complete (quick-check.yml) |
| **P2: Strategy Iteration** | ✅ | 40% | - | Steps 1-4 done, Steps 5-10 pending |
| Stage B: S3 Pipeline | ✅ | 0% | - | P3, deferred |
| Stage D/E: Optimization | ✅ | 0% | - | P4, deferred |

**Step-by-Step Status**:

| Step | Task | Status | Evidence |
|------|------|--------|----------|
| 1 | CI quick-check workflow | ✅ Complete | `.github/workflows/quick-check.yml` |
| 2 | `--gas-cost-usd` CLI param | ✅ Complete | `src/research/config.rs:19,84-89` |
| 3 | Fill probability filter | ✅ Complete | `src/research/sim.rs:11` `LIQUIDITY_FILL_BUFFER = 0.8` |
| 3.5 | Resolution lock modeling | ✅ Complete | `src/research/sim.rs:12,187-198` `MIN_ANNUAL_ROI = 0.20` |
| 4 | `scan_stats` Parquet writer | ✅ Complete | `src/data/recorder/parquet.rs` with best_overpay columns |
| 5 | `--mode microstructure` | 🔶 Pending | See Implementation Spec below |
| 6 | `--mode sweep` grid search | 🔶 Pending | See Implementation Spec below |
| 7 | Capital turnover metric | 🔶 Pending | See Implementation Spec below |
| 8 | Train/test validation | 🔶 Pending | See Implementation Spec below |
| 9 | JSON config output | 🔶 Pending | See Implementation Spec below |
| 10 | Reorg detection (SQLite) | 🔶 Pending | See Implementation Spec below |

#### Execution Order (Step-by-Step)

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                     IMPLEMENTATION SEQUENCE                                      │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  Step 1: CI/CD (Safety Net)                          ◀── START HERE             │
│  ├─ Create .github/workflows/quick-check.yml                                    │
│  ├─ Effort: 15 min                                                              │
│  └─ Value: Prevents regression during subsequent changes                        │
│         │                                                                        │
│         ▼                                                                        │
│  Step 2: Gas Cost Modeling                                                       │
│  ├─ Add --gas-cost-usd to research.rs                                           │
│  ├─ Effort: 30 min                                                              │
│  └─ Value: Filters 50%+ of unrealistic "opportunities"                          │
│         │                                                                        │
│         ▼                                                                        │
│  Step 3: Fill Probability Heuristic                                             │
│  ├─ Add 0.8× liquidity filter to SimState                                       │
│  ├─ Effort: 30 min                                                              │
│  └─ Value: Prevents "sniping entire orderbook" in backtest                      │
│         │                                                                        │
│         ▼                                                                        │
│  Step 4: scan_stats Recorder                                                     │
│  ├─ Complete Stage A Parquet integration                                        │
│  ├─ Effort: 2-3 hours                                                           │
│  └─ Value: Required for microstructure analysis                                 │
│         │                                                                        │
│         ▼                                                                        │
│  Step 5: --mode microstructure                                                   │
│  ├─ Analyze spread/liquidity/opportunity distributions                          │
│  ├─ Effort: 2-3 hours                                                           │
│  └─ Value: Understand market before tuning parameters                           │
│         │                                                                        │
│         ▼                                                                        │
│  Step 6: --mode sweep                                                            │
│  ├─ Grid search over min_profit_rate, min_liquidity                             │
│  ├─ Effort: 3-4 hours                                                           │
│  └─ Value: Find optimal parameters for production                               │
│         │                                                                        │
│         ▼                                                                        │
│  Step 7: Capital Turnover Metric                                                 │
│  ├─ Track PnL / avg_locked_capital                                              │
│  ├─ Effort: 1 hour                                                              │
│  └─ Value: Optimize for capital efficiency                                      │
│         │                                                                        │
│         ▼                                                                        │
│  Step 8: Train/Test Validation                                                   │
│  ├─ 70/30 time split, gap analysis                                              │
│  ├─ Effort: 2 hours                                                             │
│  └─ Value: Prevent overfitting                                                  │
│         │                                                                        │
│         ▼                                                                        │
│  Step 9: JSON Config Export                                                      │
│  ├─ Output recommended_config.json                                              │
│  ├─ Effort: 1 hour                                                              │
│  └─ Value: Production-ready parameters                                          │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

#### Detailed Task Breakdown

| Step | Task | File(s) | Priority | Effort | Depends On |
|------|------|---------|----------|--------|------------|
| 1 | CI quick-check workflow | `.github/workflows/quick-check.yml` | **P0** | 15 min | None |
| 2 | `--gas-cost-usd` CLI param | `src/bin/research.rs` | **P0** | 30 min | Step 1 |
| 3 | Fill probability filter | `src/bin/research.rs` | **P0** | 30 min | Step 2 |
| 3.5 | Resolution lock modeling | `src/bin/research.rs`, market metadata | **P0** | 1-2 hr | Step 3 |
| 4 | `scan_stats` Parquet writer | `src/data/recorder/parquet.rs` | **P1** | 2-3 hr | Step 3.5 |
| 5 | `--mode microstructure` | `src/bin/research.rs` | **P1** | 2-3 hr | Step 4 |
| 6 | `--mode sweep` grid search | `src/bin/research.rs` | **P1** | 3-4 hr | Step 5 |
| 7 | Capital turnover metric | `src/bin/research.rs` | **P2** | 1 hr | Step 3.5 |
| 8 | Train/test validation | `src/bin/research.rs` | **P2** | 2 hr | Step 6 |
| 9 | JSON config output | `src/bin/research.rs` | **P2** | 1 hr | Step 8 |
| 10 | Reorg detection (SQLite schema) | `src/data/recorder/mod.rs` | **P2** | 1 hr | None |

**Total Effort Estimate**: 14-18 hours (2-3 focused days)

#### Rationale for This Order

1. **CI First (Step 1)**:
   - Low effort, high value
   - Catches regressions immediately as you modify `research.rs`
   - Prevents "it worked yesterday" debugging sessions

2. **Gas + Fill Before Features (Steps 2-3)**:
   - These are **filters**, not features
   - Without them, all subsequent analysis produces inflated/unrealistic numbers
   - Fix the foundation before building on it

3. **scan_stats Before Modes (Step 4)**:
   - `--mode microstructure` needs skip-reason data
   - Can't diagnose "why no opportunities" without this dataset
   - Enables data-driven parameter selection

4. **Microstructure Before Sweep (Step 5)**:
   - Understand market characteristics before tuning
   - Prevents blind grid search over irrelevant parameter ranges
   - May reveal that certain markets are never profitable

5. **Sweep Before Validation (Step 6)**:
   - Need candidate parameters before validating them
   - Sweep produces the configs that validation will test

6. **Metrics + Export Last (Steps 7-9)**:
   - Nice-to-have, not blocking
   - Core iteration loop works without capital turnover
   - JSON export is cosmetic (can copy values manually)

#### Definition of Done (Per Step)

| Step | Acceptance Criteria |
|------|---------------------|
| 1 | `cargo fmt --check && cargo clippy && cargo test --lib` passes in GitHub Actions |
| 2 | Running with `--gas-cost-usd 0.60` filters out small-profit trades |
| 3 | Trades with `liquidity * 0.8 < order_size` are skipped |
| 3.5 | Trades with `annual_roi < 20%` (based on lock duration) are skipped |
| 4 | `scan_stats_v1/*.parquet` files written with skip-reason counts |
| 5 | `reports/microstructure.csv` contains spread/liquidity/frequency metrics |
| 6 | `reports/param_sweep.csv` contains PnL for each parameter combination |
| 7 | Summary output shows `capital_efficiency` metric |
| 8 | Validation report shows train/test gap percentage |
| 9 | `reports/recommended_config.json` written with best parameters |
| 10 | `confirmed_orders` table includes `block_number` and `tx_hash` columns |

#### Non-Goals (Explicitly Deferred)

| Task | Why Deferred | When to Revisit |
|------|--------------|-----------------|
| Stage B (S3 Pipeline) | Not blocking research iteration | After 1 week of Parquet data collected |
| Stage D (Optimistic Execution) | Risky without stable strategy | After strategy validated |
| Stage E (Feature Layer) | Premature optimization | After winning strategy proven |
| ML-based fill probability | Insufficient data | After 10K+ trades logged |
| Real-time parameter updates | Safety concern | Never (human review required) |

### Implementation Specifications (Steps 5-10)

#### Step 5: Microstructure Analysis Mode (`--mode microstructure`)

**Goal**: Output raw metrics to CSV for external analysis (Python/Pandas) to understand market characteristics before tuning.

**Files**:
- `src/research/config.rs`: Add `RunMode` enum (`Backtest`, `Microstructure`, `Sweep`).
- `src/research/mod.rs`: Branch execution based on mode.
- `src/research/report.rs`: Add `write_microstructure_report`.

**Output**: `reports/microstructure.csv`
- Columns: `market_id`, `liquidity_pct`, `spread_p50`, `spread_p95`, `slippage_100`, `opps_per_hour`

#### Step 6: Parameter Sweep Mode (`--mode sweep`)

**Goal**: Grid search over strategy parameters to find optimal configuration.

**Files**:
- `src/research/sweep.rs` (NEW): Logic to generate grid and run parallel simulations.
- `src/research/mod.rs`: Integrate sweep module.

**Grid**:
- `min_profit_rate`: 0.002, 0.003, 0.005, 0.007, 0.01
- `min_liquidity`: 0, 25, 50, 100, 200
- `gas_cost_usd`: 0.30, 0.60, 1.00

**Output**: `reports/param_sweep.csv`
- Columns: `min_profit_rate`, `min_liquidity`, `gas_cost_usd`, `trades`, `pnl`, `max_drawdown`, `win_rate`, `sharpe`

#### Step 7: Capital Turnover Metric

**Goal**: Optimize for capital efficiency (`PnL / average_locked_capital`) rather than just raw PnL.

**Changes**:
- `SimState`: Track `locked_time_weighted_sum` and `total_traded_notional`.
- `apply_confirmations()`: Update weighted sum based on elapsed time.
- `MarketReport`: Include `capital_efficiency` metric.

#### Step 8: Train/Test Validation

**Goal**: Prevent overfitting by validating parameters on unseen time periods.

**Changes**:
- CLI: Add `--train-ratio` (default 0.7).
- Logic: Split `MarketTick` vectors by timestamp.
- Workflow:
    1. Run sweep on Train set.
    2. Pick best config.
    3. Run single simulation on Test set.
    4. Report PnL gap.

#### Step 9: JSON Config Export

**Goal**: Automate the transfer of research results to production config.

**Changes**:
- `sweep.rs`: Export `RecommendedConfig` struct to `reports/recommended_config.json`.
- Include metadata: `generated_at`, `data_range`, `expected_metrics`.

#### Step 10: Reorg Detection (SQLite Schema)

**Goal**: Add data support for detecting chain reorgs (block number tracking).

**Critical Technical Constraints (Expert Feedback)**:
1.  **Idempotent Migrations**: Do NOT use simple `ALTER TABLE` scripts. Daily DB rotation means every new file needs the schema update. Use `ensure_schema()` pattern in Rust code to apply migrations if columns are missing.
2.  **Optional tx_hash**: We rely on `balanceOf` polling, so we might not have a transaction hash. Make `tx_hash` nullable/optional.
3.  **Mandatory Block Number**: Must call `get_block_number()` immediately after `balanceOf()` to anchor the state to a chain height.

**Schema Updates**:
- `confirmed_orders`: Add `block_number` (INTEGER), `tx_hash` (TEXT, nullable), `reorg_checked` (INTEGER default 0).

**Changes**:
- `src/data/recorder/mod.rs`: Update `init_schema` and `insert_confirmed_order`.
- `src/execution/confirmation.rs`: Fetch block number during confirmation cycle.

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
- `scan_stats` (one row per scan cycle) **[Required by P2 - Strategy Iteration]**:
  - `scan_ts_us` (i64), `duration_us` (u64), `markets_scanned` (u64)
  - `yesno_checked`/`yesno_found`/`yesno_missing_ask`/`yesno_cost_ge_one`
  - `yesno_low_liquidity`/`yesno_below_min_notional`/`yesno_below_min_profit`
  - `mint_sell_checked`/`mint_sell_found`/`mint_sell_missing_bid`/`mint_sell_revenue_le_one`
  - `mint_sell_low_liquidity`/`mint_sell_below_min_notional`/`mint_sell_below_min_profit`
  - `neg_risk_checked`/`neg_risk_found`/`neg_risk_missing_ask`/`neg_risk_cost_ge_one`
  - `neg_risk_low_liquidity`/`neg_risk_below_min_notional`/`neg_risk_below_min_profit`

Strategy Diagnostics (Scan Stats + Live Logs)

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
- On startup, scan the partition directory to resume the next part index and prevent overwrites.
- Store `schema_version`, `git_commit`, `config_hash` as Parquet file metadata.

Atomicity + integrity (best practice):
- Never write directly to `*.parquet`. Write to `*.parquet.tmp` first.
- After `flush` + footer write completes, validate Parquet metadata (footer read).
- Use a filesystem rename (atomic) to move `.tmp` -> `.parquet`.
- When a partition (hour) is closed, write an empty `_SUCCESS` file in the partition directory.
- Readers should only consume partitions that contain `_SUCCESS`.

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

**Intent**: Decouple trading node from data ingestion. The trading process should never handle S3 credentials or perform network uploads.

**Priority**: P3 (after Stage C research workflow is validated)

#### Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                              EC2 Trading Node                                        │
│                                                                                      │
│  ┌─────────────────────┐                    ┌─────────────────────────────────────┐  │
│  │   Trading Process   │                    │        Sidecar Uploader             │  │
│  │   (parquet.rs)      │                    │        (systemd timer)              │  │
│  │                     │                    │                                     │  │
│  │   Writes to:        │     inotify/       │   1. Watch for rotated files        │  │
│  │   data/parquet/     │────────────────────▶   2. Skip current hour (in-use)    │  │
│  │   ├─ date=.../      │     hourly         │   3. Upload to S3                   │  │
│  │   └─ hour=.../      │     rotation       │   4. Delete local after success     │  │
│  │                     │                    │   5. Log metrics                    │  │
│  └─────────────────────┘                    └──────────────┬──────────────────────┘  │
│                                                            │                         │
└────────────────────────────────────────────────────────────┼─────────────────────────┘
                                                             │
                                                             ▼
                                              ┌──────────────────────────────┐
                                              │         S3 Bucket            │
                                              │                              │
                                              │  orderbook_snapshots_v1/     │
                                              │  ├─ date=2026-01-20/         │
                                              │  │  ├─ hour=00/              │
                                              │  │  │  └─ part-000000.parquet│
                                              │  │  └─ hour=01/              │
                                              │  └─ date=2026-01-21/         │
                                              │                              │
                                              │  scan_stats_v1/              │
                                              │  └─ ...                      │
                                              └──────────────────────────────┘
```

#### Design Principles

1. **Separation of Concerns**: Trading process only writes locally; uploader handles S3.
2. **No Blocking**: Upload runs asynchronously, never blocks trading loop.
3. **Fault Tolerance**: Failed uploads are retried; local files preserved until confirmed.
4. **Credential Isolation**: S3 credentials only in uploader, not trading process.

#### Implementation Options

**Option A: Shell Script + Systemd Timer (Recommended for MVP)**

```bash
#!/bin/bash
# /opt/opinion-hft/scripts/upload_parquet.sh

set -euo pipefail

S3_BUCKET="${S3_BUCKET:-opinion-hft-data}"
PARQUET_DIR="${PARQUET_DIR:-/opt/opinion-hft/data/parquet}"
CURRENT_HOUR=$(date -u +"%Y-%m-%d/hour=%H")
UPLOAD_COUNT=0
ERROR_COUNT=0

log() { echo "[$(date -Iseconds)] $*"; }

# Find all parquet files, exclude current hour (still being written)
find "$PARQUET_DIR" -name "*.parquet" -type f | while read -r file; do
    relative_path="${file#$PARQUET_DIR/}"
    
    # Extract partition path (date=YYYY-MM-DD/hour=HH)
    partition=$(echo "$relative_path" | grep -oP 'date=\d{4}-\d{2}-\d{2}/hour=\d{2}' || true)
    
    # Skip current hour (file may still be written)
    if [[ "$partition" == "date=$CURRENT_HOUR" ]]; then
        continue
    fi
    
    # Upload to S3
    if aws s3 cp "$file" "s3://$S3_BUCKET/$relative_path" --only-show-errors; then
        log "Uploaded: $relative_path"
        rm -f "$file"
        ((UPLOAD_COUNT++)) || true
    else
        log "ERROR: Failed to upload $relative_path"
        ((ERROR_COUNT++)) || true
    fi
done

# Clean up empty directories
find "$PARQUET_DIR" -type d -empty -delete 2>/dev/null || true

log "Upload complete: $UPLOAD_COUNT files, $ERROR_COUNT errors"

# Exit with error if any uploads failed
[[ $ERROR_COUNT -eq 0 ]] || exit 1
```

**Systemd Service:**

```ini
# /etc/systemd/system/parquet-upload.service
[Unit]
Description=Upload rotated Parquet files to S3
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
User=opinion-hft
ExecStart=/opt/opinion-hft/scripts/upload_parquet.sh
Environment=S3_BUCKET=opinion-hft-data
Environment=PARQUET_DIR=/opt/opinion-hft/data/parquet
StandardOutput=append:/var/log/parquet-upload.log
StandardError=append:/var/log/parquet-upload.log
```

**Systemd Timer:**

```ini
# /etc/systemd/system/parquet-upload.timer
[Unit]
Description=Run parquet upload every hour at :05

[Timer]
OnCalendar=*:05:00
Persistent=true
RandomizedDelaySec=30

[Install]
WantedBy=timers.target
```

**Option B: Rust Sidecar Binary (Future, if more control needed)**

```rust
// src/bin/parquet_uploader.rs (skeleton)
use aws_sdk_s3::Client;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = aws_config::load_from_env().await;
    let s3 = Client::new(&config);
    
    let parquet_dir = std::env::var("PARQUET_DIR")
        .unwrap_or_else(|_| "data/parquet".into());
    
    // Use inotify to watch for file close events
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = RecommendedWatcher::new(tx, notify::Config::default())?;
    watcher.watch(Path::new(&parquet_dir), RecursiveMode::Recursive)?;
    
    for event in rx {
        if let Ok(event) = event {
            // Handle CLOSE_WRITE events for completed files
            // Skip current hour, upload, delete on success
        }
    }
    Ok(())
}
```

#### S3 Directory Structure

```text
s3://opinion-hft-data/
├── orderbook_snapshots_v1/
│   ├── date=2026-01-20/
│   │   ├── hour=00/
│   │   │   ├── part-000000.parquet
│   │   │   └── part-000001.parquet
│   │   ├── hour=01/
│   │   │   └── part-000000.parquet
│   │   └── ...
│   └── date=2026-01-21/
│       └── ...
├── scan_stats_v1/
│   └── date=2026-01-20/
│       └── ...
└── confirmed_orders/           # Optional: SQLite exports
    └── ...
```

#### Configuration

```bash
# Environment variables for uploader (NOT in trading process)
S3_BUCKET=opinion-hft-data
S3_REGION=ap-northeast-1
PARQUET_DIR=/opt/opinion-hft/data/parquet

# AWS credentials (use IAM role on EC2, or explicit keys)
AWS_ACCESS_KEY_ID=***
AWS_SECRET_ACCESS_KEY=***

# Optional: Local retention before upload
PARQUET_LOCAL_RETENTION_HOURS=24
```

#### Monitoring & Alerting

```yaml
# CloudWatch Alarm (Terraform example)
resource "aws_cloudwatch_metric_alarm" "upload_failure" {
  alarm_name          = "parquet-upload-failure"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 3
  metric_name         = "Errors"
  namespace           = "OpinionHFT/DataPipeline"
  period              = 3600
  statistic           = "Sum"
  threshold           = 0
  alarm_description   = "Parquet upload failed for 3 consecutive hours"
  alarm_actions       = [aws_sns_topic.alerts.arn]
}
```

#### Implementation Checklist

| Task | Priority | Status |
|------|----------|--------|
| Shell upload script | P0 | Pending |
| Systemd timer config | P0 | Pending |
| S3 bucket + IAM role | P0 | Pending |
| Log rotation for upload logs | P1 | Pending |
| CloudWatch metrics/alarms | P1 | Pending |
| Rust sidecar (optional) | P2 | Deferred |

#### Non-Goals

- No S3 SDK in trading process
- No real-time streaming (hourly batch is sufficient)
- No complex retry logic in MVP (systemd timer handles retries)

### Stage C: Research Backend (offline)

**Intent**: Convert recorded data into validated strategy parameters using the **exact same logic** as production, but running offline against Parquet history.

#### 1. Architecture: The Shared Kernel Pattern

The system guarantees **Zero Implementation Drift** by sharing the core library between Live and Backtest engines.

```text
┌─────────────────────────────────────┐
│           Core Strategy Lib         │
│         (src/strategy/core/)        │
│                                     │
│  • Fee Calculator (Pure Fn)         │
│  • Arb Detector (State Machine)     │
│  • Risk Rules (Pure Logic)          │
└──────────────────┬──────────────────┘
                   │
         ┌─────────┴─────────┐
         ▼                   ▼
┌─────────────────┐ ┌─────────────────┐
│   Live Engine   │ │ Backtest Engine │
│  (src/main.rs)  │ │ (src/bin/res..) │
│                 │ │                 │
│  • WebSocket    │ │ • Polars Reader │
│  • REST Client  │ │ • Replay Loop   │
│  • Real Wallet  │ │ • Mock Wallet   │
└─────────────────┘ └─────────────────┘
```

#### 2. Component Implementation Details

**A. Core Strategy Lib (`src/strategy/core/`)**
*Shared "Source of Truth". No Python logic allowed here.*

*   **Fee Calculator (`fees.rs`)**:
    *   Symmetric curve: `Fee = topic_rate * price * (1 - price)`.
    *   Constraints: Min fee $0.50, Max rate at $0.50, Referral discount applied.
*   **Arbitrage Detector (`detector.rs`)**:
    *   Single-pass state machine detecting: `YesNo` (Sum < 1), `NegRisk` (Sum < 1), `MintSell` (Sum > 1), `SpreadAnomaly`.
    *   Returns `ArbitrageOpportunity` with precise `total_spend` (inc. fees) and `net_profit`.
*   **Risk Rules (`risk.rs`)**:
    *   Stateless pre-approval checks (Max Exposure, Max Trade Notional).

**B. Backtest Engine (`src/bin/research.rs`)**
*Hybrid Execution Model: Stateless I/O + Stateful Simulation.*

```text
┌────────────────────────────────────────────┐
│ Newcomer Quickstart                        │
├────────────────────────────────────────────┤
│ 1) Build with polars feature               │
│ 2) Run research with input glob            │
│ 3) (Optional) attach confirmed_orders DB   │
│ 4) Inspect CSV + microstructure summary    │
└────────────────────────────────────────────┘
```

**Run (required feature flag):**
```bash
cargo run --bin research --features polars -- \
  --input "data/parquet/orderbook_snapshots_v1/date=*/hour=*/*.parquet" \
  --output reports/potential_trades.csv \
  --confirmed-db data/opinion-hft.sqlite
```

```text
┌──────────────────────────────┐
│ Parquet snapshots (Hive)     │
│ date=*/hour=*                │
└───────────────┬──────────────┘
                │ scan_parquet + streaming
                ▼
┌──────────────────────────────┐
│ Columnar extraction          │
│ market_id / ts / bids / asks │
└───────────────┬──────────────┘
                ▼
┌──────────────────────────────┐
│ Per-market replay            │
│ Rayon + SimState             │
└───────────────┬──────────────┘
                ▼
┌──────────────────────────────┐
│ Reports                       │
│ trades.csv + microstructure  │
└──────────────────────────────┘
```

1.  **Data Loading (Polars Lazy API)**:
    *   Scans Parquet using Hive partitioning (`date=*/hour=*`).
    *   Applies stateless filters (spread, liquidity) pushing predicates down to scan level.
    *   Enable `with_streaming(true)` for large datasets.
    *   Convert rows via columnar access (avoid `get_row()` per row).

```text
┌──────────────┐   ┌────────────────────┐   ┌───────────────┐
│ Parquet scan │─▶│ Lazy filters (push) │─▶│ Streaming exec │
└──────────────┘   └────────────────────┘   └───────────────┘
```
2.  **Simulation Loop (Rust + Rayon)**:
    *   Parallelizes processing by `market_id`.
    *   **Latency Model**: "Next-Tick Execution" (Execute at T+1). Simulates ~200ms reaction time.
    *   **Constraint Model**: Applies 3-5s confirmation lock (funds unavailable during confirmation).
3.  **Mock Wallet (`SimState`)**:
    *   Tracks virtual balance, inventory, and pending transactions.

```text
┌───────────────────────────┐   ┌───────────────────────────┐
│ Required Parquet Columns  │   │ Optional Columns          │
├───────────────────────────┤   ├───────────────────────────┤
│ market_id (string)        │   │ outcome (string)          │
│ local_recv_ts_us (i64)    │   │ pair (string)             │
│ best_bid (f64)            │   │ bids (list<struct{p,s}>)  │
│ best_ask (f64)            │   │ asks (list<struct{p,s}>)  │
│ bid_size (f64)            │   │ snapshot_ts (i64)         │
│ ask_size (f64)            │   │                           │
└───────────────────────────┘   └───────────────────────────┘
```

#### 3. State Model (SimState)

We simulate a minimal "Virtual Exchange" to track capital lock-up and PnL.

```rust
struct SimState {
    // Capital state
    balance: f64,
    inventory: HashMap<String, f64>, // TokenID -> Amount

    // Blind exposure (Latency & Lock modeling)
    unlock_ts_us: i64,               // Funds locked until this time
    pending_txs: Vec<PendingTx>,     // Txs submitted but not confirmed

    // Metrics
    max_drawdown: f64,
    winning_txs: u64,
}
```

#### 4. Inputs & Outputs

**Inputs**:
*   **Snapshots**: `data/parquet/orderbook_snapshots_v1/date=.../*.parquet`
*   **Truth**: `data/opinion-hft.sqlite` (Table: `confirmed_orders` for reconciliation)

**Outputs**:
*   `potential_trades.csv`: All signaled trades with expected PnL.
*   `config_recommendations.toml`: Optimized thresholds (min_profit, min_liquidity).
*   `microstructure` summary: liquidity availability, spread distribution, slippage, opportunity rate.

```text
┌───────────────────────────┐
│ microstructure metrics    │
├───────────────────────────┤
│ liquidity_availability_pct│ = % of ticks with non-empty TOB
│ spread_p50 / spread_p95   │ = median / p95 of TOB spread
│ slippage_100_shares       │ = median modeled impact @100
│ opportunity_frequency/hr  │ = (arb ticks) / hours
└───────────────────────────┘
```

```text
┌───────────────────────────┐
│ confirmed_orders schema   │
├───────────────────────────┤
│ market_id (text)          │
│ side (text)               │
│ expected_price (real)     │
│ actual_fill (text)        │
│ confirmation_time_ms (int)│
│ timestamp_ms (int)        │
└───────────────────────────┘
```

```text
┌──────────────┐   ┌───────────────────────┐   ┌────────────────────┐
│ Snapshots    │─▶│ Backtest + SimState   │─▶│ Reports + Metrics   │
└──────────────┘   └───────────────────────┘   └────────────────────┘
```

#### 5. Implementation Checklist

1.  [x] **Core Refactor**: Extract `strategy/core` shared library.
2.  [x] **CLI Tool**: `src/bin/research.rs` with Polars integration.
3.  [x] **SimState**: Implement `SimState` struct and lock logic.
4.  [x] **Reporting**: CSV export of trade logs and summary metrics.
5.  [x] **Columnar iteration**: avoid `get_row()` in hot loops.
6.  [x] **Streaming mode**: enable Polars streaming engine.
7.  [x] **Microstructure report**: add liquidity/spread/slippage/opportunity summary.
8.  [x] **Confirmed-order attribution**: load `confirmed_orders` from SQLite for reconciliation.

### P2: Strategy Iteration (Driven by Stage C Results)

**Intent**: Close the feedback loop from data collection → analysis → parameter optimization → validated production config.

**Priority**: P2 (after Stage A recorder and Stage C research CLI are functional)

#### 1. Prediction Market Characteristics

Opinion.trade is a **prediction market**, not a traditional exchange. Strategy iteration must account for these differences:

| Aspect | Traditional Market (BTC) | Prediction Market (Opinion) |
|--------|--------------------------|----------------------------|
| Price meaning | Asset value | Probability of outcome |
| Price range | Unbounded | 0.0 - 1.0 (probability) |
| Resolution | Never | Fixed date (market resolves) |
| Arbitrage type | Cross-exchange price diff | Sum deviation from 1.0 |
| Exit strategy | Stop-loss, take-profit | Hold until resolution |

**Market Types**:

| Type | Structure | Arbitrage Condition | Example |
|------|-----------|---------------------|---------|
| **Binary (Yes/No)** | 2 outcomes | `yes_ask + no_ask < 1.0` | "Will BTC hit 100k?" |
| **Categorical (Multi)** | N outcomes (N ≥ 3) | `sum(all_asks) < 1.0` | "Who wins election?" (A/B/C/D) |

**Key insight**: Once arbitrage is executed (all outcomes bought at sum < 1.0), profit is **locked until resolution**. No need for:
- Stop-loss logic (profit already guaranteed)
- Price-based exit signals (irrelevant once position is hedged)

**Timeout consideration** (low priority): If resolution is months away, capital is locked. An optional `max_holding_days` parameter could trigger early exit if favorable prices appear, but this is **not required for MVP** since arbitrage profit is already secured.

#### 2. Defining "Optimal Parameters"

For prediction market arbitrage, optimization targets depend on goals:

| Objective | Formula | When to Use |
|-----------|---------|-------------|
| **Max PnL** | `sum(net_profit)` | Aggressive, more trades |
| **Risk-Adjusted Return** | `PnL / max_drawdown` | Balanced (recommended) |
| **Capital Efficiency** | `PnL / avg_locked_capital` | When capital is scarce |
| **Win Rate** | `winning / total` | Conservative, fewer trades |

**Recommended default**: `PnL / max_drawdown` (simple risk-adjusted metric).

#### 3. Iteration Loop Architecture

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                        P2: Strategy Iteration Loop                               │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│   ┌────────────────┐                                                             │
│   │  Stage A Data  │                                                             │
│   │  - snapshots   │                                                             │
│   │  - scan_stats  │◀─────────────────────────────────────────────────────────┐  │
│   └───────┬────────┘                                                          │  │
│           │                                                                   │  │
│           ▼                                                                   │  │
│   ┌────────────────┐    ┌────────────────┐    ┌────────────────┐              │  │
│   │ Microstructure │    │   Parameter    │    │   Validation   │              │  │
│   │   Analysis     │───▶│     Sweep      │───▶│    Report      │              │  │
│   │                │    │                │    │                │              │  │
│   │ • spread dist  │    │ • grid search  │    │ • train/test   │              │  │
│   │ • liquidity %  │    │ • objective fn │    │ • gap analysis │              │  │
│   │ • opp frequency│    │ • rayon ∥      │    │ • recommend    │              │  │
│   └────────────────┘    └────────────────┘    └───────┬────────┘              │  │
│                                                       │                       │  │
│                                                       ▼                       │  │
│                                              ┌────────────────┐               │  │
│                                              │  Config Output │               │  │
│                                              │                │               │  │
│                                              │ recommended_   │───────────────┘  │
│                                              │ config.json    │                  │
│                                              └────────────────┘                  │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

#### 4. Component Details

**A. Microstructure Analysis**

Understand market characteristics before parameter tuning.

| Metric | Description | Diagnostic Value |
|--------|-------------|------------------|
| `arb_spread_dist` | `sum(asks) - 1.0` percentiles (p50, p95) | How often is arb possible? |
| `liquidity_pct` | % of ticks with non-empty top-of-book | Can we execute? |
| `opp_freq_per_hour` | Count of `sum < 1.0 - fees` per hour | Market activity |
| `avg_executable_size` | Mean size when opportunity exists | Position sizing |
| `binary_vs_categorical` | Ratio of market types | Strategy mix |

**Output**: `reports/microstructure.csv`

```bash
cargo run --bin research -- --mode microstructure \
  --input "data/parquet/orderbook_snapshots_v1/date=*/hour=*/*.parquet" \
  --output reports/microstructure.csv
```

**B. Parameter Sweep**

Grid search over parameter space with parallel simulation.

| Parameter | Range | Rationale |
|-----------|-------|-----------|
| `min_profit_rate` | 0.001, 0.003, 0.005, 0.007, 0.01 | Trade-off: volume vs per-trade profit |
| `min_liquidity` | 0, 50, 100, 200 | Filter illiquid opportunities |
| `confirm_delay_us` | 3M, 4M, 5M | Sensitivity to chain latency |

**Output**: `reports/param_sweep.csv`

```csv
min_profit_rate,min_liquidity,trades,pnl,win_rate,max_drawdown,risk_adj
0.001,0,245,12.34,0.78,0.05,246.8
0.003,50,142,15.67,0.85,0.03,522.3
0.005,100,87,11.23,0.91,0.02,561.5
```

```bash
cargo run --bin research -- --mode sweep \
  --min-profit-rate 0.001,0.003,0.005,0.007 \
  --min-liquidity 0,50,100,200 \
  --output reports/param_sweep.csv
```

**C. Train/Test Validation**

Prevent overfitting to specific time periods.

1. Split data by time: 70% train, 30% test
2. Run sweep on train set
3. Evaluate top-3 configs on test set
4. Report performance gap

**Output Example**:

```
=== Validation Report ===
Best config: min_profit_rate=0.003, min_liquidity=50.0

        Train     Test      Gap
PnL     15.67     14.23    -9.2%
WinRate 0.85      0.82     -3.5%

Recommendation: Config stable (gap < 15%)
```

#### 5. Config Output

Export recommended parameters for production use.

**Output**: `reports/recommended_config.json`

```json
{
  "generated_at": "2026-01-21T12:00:00Z",
  "data_period": { "start": "2026-01-15", "end": "2026-01-20" },
  "recommended": {
    "min_profit_rate": 0.003,
    "min_liquidity": 50.0,
    "gas_cost_usd": 0.60
  },
  "validation": {
    "train_pnl": 15.67,
    "test_pnl": 14.23,
    "gap_pct": -9.2
  }
}
```

#### 6. Realistic PnL Modeling (Critical for Backtest Accuracy)

The research CLI must model real-world costs accurately to avoid inflated backtest results.

**A. Gas Cost Modeling**

BNB Chain gas costs are non-trivial for small arbitrage opportunities.

| Parameter | Default | Rationale |
|-----------|---------|-----------|
| `gas_cost_usd` | $0.60 | 2 transactions × ~$0.30 each (pessimistic estimate) |

**Why this matters**:
- Small arb ($5 notional × 0.5% profit = $0.025 gain) → Gas eats **2400%** of profit
- Without gas modeling, backtest shows 100 profitable trades; reality shows 5

**CLI Integration**:
```bash
cargo run --bin research -- \
  --gas-cost-usd 0.60 \          # Default: $0.60 (2 txs × $0.30)
  --input "data/parquet/..." \
  --output reports/trades.csv
```

**Implementation in SimState**:
```rust
// In CliConfig
gas_cost_usd: f64,  // Default: 0.60

// In execute_trade()
let gross_profit = opportunity.net_profit_abs;  // After platform fees
let net_profit = gross_profit - config.gas_cost_usd;  // After gas

if net_profit <= 0.0 {
    // Skip: gas eats all profit
    return;
}
```

**Sensitivity Analysis Use Case**:
```bash
# Compare: "If we optimize gas to $0.20, how many more opportunities?"
cargo run --bin research -- --gas-cost-usd 0.20 --output reports/low_gas.csv
cargo run --bin research -- --gas-cost-usd 0.60 --output reports/high_gas.csv
# Diff the trade counts
```

**B. Fill Probability Heuristic**

Backtest must not assume 100% fill rate on quoted liquidity.

**Problem**: Liquidity displayed at T may be gone by T+1 (latency, competition).

**Heuristic Formula**:
```
fill_rate = min(1.0, (available_liquidity × 0.8) / order_size)
```

| Component | Value | Rationale |
|-----------|-------|-----------|
| `0.8` multiplier | 80% | Assume 20% of liquidity disappears before arrival (latency proxy) |
| `order_size` | From opportunity | Planned trade size |
| `available_liquidity` | From orderbook | Sum of sizes at target price level |

**Decision Logic** (Conservative for MVP):
```rust
let fill_rate = (tick.yes.ask_size * 0.8 / opportunity.max_size).min(1.0);

if fill_rate < 1.0 {
    // Skip: insufficient reliable liquidity
    debug!("skipped: fill_rate={:.2} < 1.0", fill_rate);
    continue;
}
```

**Alternative (Aggressive)**: Scale down order size instead of skipping:
```rust
let adjusted_size = opportunity.max_size * fill_rate;
if adjusted_size < config.min_order_notional {
    continue;  // Too small after adjustment
}
```

**Recommendation**: Use **conservative skip** for MVP. Aggressive scaling requires more validation.

**C. Capital Turnover Metric**

Track capital efficiency, not just absolute PnL.

**Formula**:
```
capital_turnover = total_traded_notional / avg_locked_capital
capital_efficiency = PnL / avg_locked_capital
```

**Why this matters**: A strategy that locks $1000 for weeks to earn $10 is worse than one that turns $100 ten times to earn $10.

**Implementation**:
```rust
struct SimState {
    // Existing fields...
    total_traded_notional: f64,   // Sum of all trade notionals
    locked_time_weighted_sum: f64, // For avg_locked_capital calculation
    last_update_ts_us: i64,
}

// In apply_confirmations():
let elapsed_us = current_ts - state.last_update_ts_us;
state.locked_time_weighted_sum += state.locked_balance * elapsed_us as f64;
state.last_update_ts_us = current_ts;

// In print_summary():
let avg_locked = state.locked_time_weighted_sum / total_duration_us as f64;
let capital_efficiency = pnl / avg_locked;
println!("  capital_efficiency: {:.4}", capital_efficiency);
```

**D. Resolution Lock Modeling (Prediction Market Specific)** [Expert Recommendation]

Unlike perpetual futures or spot trading, prediction market arbitrage **locks capital until event resolution**. This fundamentally changes profitability calculations.

**Problem**: A 0.5% profit that locks capital for 30 days has terrible annualized return:
```
Annual ROI = 0.5% × (365 / 30) = 6.08%
vs.
Annual ROI = 0.5% × (365 / 1) = 182.5% (if capital released daily)
```

**Key Insight**: Capital turnover is more important than per-trade profit rate in prediction markets.

**SimState Enhancement**:
```rust
struct SimState {
    // Existing fields...
    
    // Resolution Lock Tracking
    locked_positions: HashMap<String, LockedPosition>,  // market_id -> position
}

struct LockedPosition {
    market_id: String,
    entry_ts_us: i64,
    resolution_ts_us: i64,       // Expected market resolution time
    capital_locked: f64,
    expected_profit: f64,
}
```

**Resolution-Aware Metrics**:
```rust
// Annualized ROI per trade
fn annualized_roi(profit_rate: f64, lock_days: f64) -> f64 {
    profit_rate * (365.0 / lock_days)
}

// In opportunity filtering:
let lock_days = (market.resolution_ts - now) / SECS_PER_DAY;
let annual_roi = annualized_roi(opportunity.net_profit_rate, lock_days);

// Minimum annualized threshold (e.g., 20% = better than risk-free)
const MIN_ANNUAL_ROI: f64 = 0.20;

if annual_roi < MIN_ANNUAL_ROI {
    debug!("skipped: annual_roi={:.2}% < {:.0}%", annual_roi * 100.0, MIN_ANNUAL_ROI * 100.0);
    continue;
}
```

**Integration with Market Data**:
- Fetch `resolution_ts` (settlement date) from market metadata
- Store in `MarketInfo` struct alongside `yes_pair`, `no_pair`
- Use in sweep analysis: plot `profit_rate × volume` vs `lock_duration`

**Parameter Sweep Addition**:
| Parameter | Range | Rationale |
|-----------|-------|-----------|
| `max_lock_days` | 7, 14, 30, 60 | Trade-off: more opportunities vs lower capital velocity |
| `min_annual_roi` | 0.10, 0.20, 0.50 | Filter low-yield long-lock trades |

**Why This Matters**:
- A bot that only trades 7-day markets at 1% profit (52% annual) beats one trading 30-day markets at 2% profit (24% annual)
- Compounding effect: freed capital can be re-deployed to new opportunities
- Risk consideration: shorter lock = less time for unexpected events

**Implementation Priority**: P1 (important for realistic strategy evaluation). Without this, parameter sweep may recommend long-lock trades that destroy capital efficiency.

#### 7. Skip Reasons: The Most Valuable Data [Expert Emphasis]

**Why Skip Reasons Matter More Than Trade Logs**:

Looking at trade logs only tells you "what worked." Skip reasons tell you **"what could work with different parameters"**.

| Skip Reason | Diagnostic Value | Action |
|-------------|------------------|--------|
| `gas_too_high` | Gas cost exceeds profit | Batch orders? Wait for lower gas? |
| `low_liquidity` | Orderbook too thin | Increase `min_liquidity` param |
| `spread_too_small` | Opportunity exists but unprofitable | Lower `min_profit_rate`? |
| `cost_ge_one` | No arbitrage (market efficient) | Skip this market |
| `below_min_notional` | Position too small | Aggregate across markets? |
| `fill_rate_low` | Would move the market | Conservative sizing correct |
| `lock_too_long` | Capital efficiency concern | Prefer short-term markets |

**Aggregation for Analysis**:
```sql
-- Which skip reason is most common?
SELECT skip_reason, COUNT(*) as count, 
       SUM(potential_profit) as missed_profit
FROM scan_stats
GROUP BY skip_reason
ORDER BY missed_profit DESC;
```

**Key Question to Answer**:
> "If I reduce `min_profit_rate` by 0.1%, how many more trades would I have taken, and what would the aggregate PnL be?"

This is only answerable with comprehensive skip-reason logging.

#### 8. Dependency: scan_stats Dataset

P2 requires `scan_stats` from Stage A to diagnose "why no opportunities".

See Stage A: `scan_stats` marked as **[Required by P2 - Strategy Iteration]**.

Without `scan_stats`:
- Cannot distinguish "no data" vs "opportunities filtered out"
- Cannot tune parameters based on skip reasons

#### 9. Implementation Checklist

| Task | Priority | Depends On | Status |
|------|----------|------------|--------|
| `--gas-cost-usd` CLI param | P0 | Stage C CLI | Pending |
| Fill probability heuristic | P0 | Stage C CLI | Pending |
| Resolution lock modeling | P0 | Market metadata | Pending |
| `--mode microstructure` | P0 | Stage C CLI | Pending |
| `--mode sweep` with grid | P0 | Stage C CLI | Pending |
| Capital turnover metric | P1 | SimState | Pending |
| `max_lock_days` param sweep | P1 | Resolution lock | Pending |
| Train/test split logic | P1 | Sweep mode | Pending |
| JSON config output | P1 | Validation | Pending |
| `scan_stats` recorder | P1 | Stage A | Pending |
| Reorg detection (SQLite) | P2 | confirmed_orders | Pending |

#### 10. Non-Goals

- No automatic production config updates (human review required)
- No ML-based optimization (grid search sufficient for MVP)
- No real-time parameter adaptation (batch analysis only)
- No max-holding-time exit logic (profit already locked on execution)

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

## 12. CI/CD Pipeline

### 12.1 Overview

Continuous Integration ensures code quality persists while the codebase evolves. For a trading system, CI is a critical safety net—catching regressions before they reach production.

**Design Philosophy**: Speed over Completeness for daily development; Thoroughness for releases.

### 12.2 Two-Tier CI Strategy

| Pipeline | Trigger | Duration | Purpose |
|----------|---------|----------|---------|
| **Quick Check** | Every push/PR | < 2 min | Fast feedback loop |
| **Full Audit** | Nightly / Release | 5-10 min | Comprehensive validation |

### 12.3 Quick Check Pipeline (Default)

**File**: `.github/workflows/quick-check.yml`

**Goals**:
- Catch formatting/lint violations immediately
- Run fast unit tests (no network, no API keys)
- Block obvious regressions before merge

**Jobs**:

```yaml
name: Quick Check
on: [push, pull_request]

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    name: Rust Quick Check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      
      - name: Cache dependencies
        uses: swatinem/rust-cache@v2  # CRITICAL: 10x faster builds
      
      - name: Check Formatting
        run: cargo fmt --all -- --check
      
      - name: Clippy (Linting)
        run: cargo clippy --all-targets -- -D warnings
      
      - name: Run Unit Tests (Fast)
        run: cargo test --lib  # Excludes integration tests
```

**Key Points**:
- `swatinem/rust-cache@v2` is **mandatory**—without it, Rust builds take 10+ minutes
- `cargo test --lib` excludes integration tests (which require `.env` and network)
- `-D warnings` fails on any clippy warning (strict quality gate)

### 12.4 Full Audit Pipeline (Nightly/Release)

**File**: `.github/workflows/full-audit.yml`

**Goals**:
- Run complete test suite including integration tests
- Validate backtest/research tools
- Generate build artifacts for releases

**Jobs**:

```yaml
name: Full Audit
on:
  schedule:
    - cron: '0 2 * * *'  # Daily at 2 AM UTC
  workflow_dispatch:      # Manual trigger

jobs:
  full-test:
    name: Full Test Suite
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - uses: dtolnay/rust-toolchain@stable
      
      - uses: swatinem/rust-cache@v2
      
      - name: Build All Targets
        run: cargo build --all-targets
      
      - name: Run All Tests
        run: cargo test --all
        env:
          # Integration tests skipped unless secrets provided
          OPINION_API_KEY: ${{ secrets.OPINION_API_KEY }}
          # ... other secrets as needed
      
      - name: Build Release Binary
        run: cargo build --release
```

### 12.5 Integration Test Strategy

**Problem**: Live integration tests require API keys and can be flaky due to external service availability.

**Decision**: Skip integration tests in standard CI; run them only on nightly builds with secrets.

| Test Type | Quick Check | Full Audit | Local Dev |
|-----------|-------------|------------|-----------|
| Unit tests (`--lib`) | ✅ Run | ✅ Run | ✅ Run |
| Integration tests | ❌ Skip | ✅ Run (with secrets) | ✅ Run (with `.env`) |
| Live tests | ❌ Skip | ⚠️ Optional | ✅ Manual |

**Implementation**:
```rust
// In tests/live_*.rs
#[tokio::test]
#[ignore]  // Skipped by default in CI
async fn test_live_order_lifecycle() {
    // Requires OPINION_API_KEY
}

// Run locally with: cargo test -- --ignored
// Run in CI with: cargo test --all (if secrets provided)
```

**Why Not Mock Servers?**:
- Building a mock Opinion Trade API is high effort
- Mocks can drift from real API behavior
- Unit tests + occasional live tests are sufficient for MVP

### 12.6 Caching Strategy

**Critical**: Rust compilation is slow. Caching is mandatory for practical CI.

| Cache Tool | What it Caches | Speedup |
|------------|----------------|---------|
| `swatinem/rust-cache@v2` | `target/`, `~/.cargo/` | 10x |
| GitHub Actions cache | Same, manual config | 5-8x |

**Best Practice**: Use `swatinem/rust-cache@v2`—it handles Cargo.lock hashing and cache invalidation automatically.

### 12.7 Pre-Commit Hooks (Optional)

For stricter local enforcement, add a pre-commit hook:

**File**: `.githooks/pre-commit`
```bash
#!/bin/bash
set -e

echo "Running pre-commit checks..."

# Format check
cargo fmt --all -- --check

# Clippy
cargo clippy --all-targets -- -D warnings

# Quick unit tests
cargo test --lib

echo "All checks passed!"
```

**Setup**:
```bash
git config core.hooksPath .githooks
chmod +x .githooks/pre-commit
```

### 12.8 Implementation Checklist

| Task | Priority | Status |
|------|----------|--------|
| Create `.github/workflows/quick-check.yml` | P0 | ✅ Completed |
| Add `swatinem/rust-cache@v2` | P0 | ✅ Completed |
| Create `.github/workflows/full-audit.yml` | P1 | ✅ Completed |
| Add `#[ignore]` to live tests | P1 | ✅ Completed |
| Setup pre-commit hooks (optional) | P2 | ✅ Completed |

### 12.9 Why CI Matters for Trading Systems

1. **Safety Net**: As you modify strategy logic (gas costs, fill probability), CI catches if you accidentally break the live trading engine
2. **Prevents Drift**: Without CI, research tool changes can diverge from production code
3. **Refactoring Confidence**: With green CI, you can refactor aggressively without fear
4. **Documentation**: CI config is executable documentation of your quality standards

---

## 13. New Session Prompt

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
> See Section 3 (Critical Constraints & Parameters) and Section 6 (API Compatibility Design).
> 
> 
> ## Critical Design Decision
> 
> See Section 4.2 (Order Confirmation Flow).
> 
> 
> ## Implementation Order
> 
> See Section 11 (Implementation Order).
> 
> 
> ## Start Implementation
> 
> 
> Please begin with **Phase 1**. Create the project structure and implement the foundation modules with tests. Each module must pass tests before moving to the next.
