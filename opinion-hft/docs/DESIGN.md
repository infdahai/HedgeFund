---
📋 Opinion Trade HFT Framework - Complete Context Document
> Purpose: This document captures ALL context, research findings, design decisions, and implementation details from our planning session. Use this to resume work in a new session.
---

# Architecture Overview

## System Architecture Diagram

```
┌──────────────────────────────────────────────────────────────────────────────────┐
│                           OPINION TRADE HFT FRAMEWORK                             │
├──────────────────────────────────────────────────────────────────────────────────┤
│                                                                                   │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │                              DATA PLANE                                      │ │
│  │                                                                              │ │
│  │   ┌─────────────────┐   ┌─────────────────┐   ┌─────────────────────────┐   │ │
│  │   │  WebSocket      │   │   REST Client   │   │    Chain Client         │   │ │
│  │   │  Client         │   │                 │   │    (Alloy)              │   │ │
│  │   │                 │   │                 │   │                         │   │ │
│  │   │ ┌─────────────┐ │   │  /markets       │   │  balanceOf()            │   │ │
│  │   │ │ market.data │ │   │  /orderbook     │   │  balanceOfBatch()       │   │ │
│  │   │ │ order.update│ │   │  /orders        │   │                         │   │ │
│  │   │ │ trade.record│ │   │                 │   │  ConditionalTokens      │   │ │
│  │   │ └─────────────┘ │   │  ?apikey=XXX    │   │  (ERC-1155)             │   │ │
│  │   └────────┬────────┘   └────────┬────────┘   └────────────┬────────────┘   │ │
│  │            │                     │                         │                 │ │
│  │            └─────────────────────┼─────────────────────────┘                 │ │
│  │                                  ▼                                           │ │
│  │                    ┌─────────────────────────────┐                           │ │
│  │                    │        Event Bus            │                           │ │
│  │                    │   (tokio::broadcast)        │                           │ │
│  │                    └─────────────┬───────────────┘                           │ │
│  └──────────────────────────────────┼───────────────────────────────────────────┘ │
│                                     │                                             │
│                                     ▼                                             │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │                           EXECUTION PLANE                                    │ │
│  │                                                                              │ │
│  │   ┌─────────────────┐   ┌─────────────────┐   ┌─────────────────────────┐   │ │
│  │   │   EIP-712       │   │  Priority Queue │   │    Rate Limiter         │   │ │
│  │   │   Signer        │   │                 │   │    (Governor GCRA)      │   │ │
│  │   │                 │   │  Critical  ━━━▶ │   │                         │   │ │
│  │   │  Precomputed:   │   │  High      ━━━▶ │   │  15 TPS                 │   │ │
│  │   │  • Domain Sep   │   │  Medium    ━━━▶ │   │  Burst: 3               │   │ │
│  │   │  • Type Hash    │   │  Low       ━━━▶ │   │  ~15ns/check            │   │ │
│  │   │                 │   │                 │   │                         │   │ │
│  │   │  k256 ECDSA     │   │  FIFO per level │   │  until_ready().await    │   │ │
│  │   └─────────────────┘   └────────┬────────┘   └─────────────────────────┘   │ │
│  │                                  │                                           │ │
│  │                                  ▼                                           │ │
│  │              ┌───────────────────────────────────────────────┐               │ │
│  │              │           Confirmation Tracker                │               │ │
│  │              │                                               │               │ │
│  │              │  ┌─────────────────────────────────────────┐  │               │ │
│  │              │  │  Mode: TrustWebSocket | OnChainOnly |   │  │               │ │
│  │              │  │        Hybrid (recommended)             │  │               │ │
│  │              │  └─────────────────────────────────────────┘  │               │ │
│  │              │                                               │               │ │
│  │              │  1. Record position_before                    │               │ │
│  │              │  2. Wait for WS status="Finish"               │               │ │
│  │              │  3. Query on-chain balanceOf()                │               │ │
│  │              │  4. Calculate: position_after - position_before│               │ │
│  │              │  5. Emit ConfirmationEvent                    │               │ │
│  │              └───────────────────────────────────────────────┘               │ │
│  └──────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                   │
└───────────────────────────────────────────────────────────────────────────────────┘
```

## Module Interactions

```
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
     ┌─────────────────┐            ┌─────────────────┐            ┌─────────────────┐
     │  Rate Limiter   │            │   REST Client   │            │ Confirmation    │
     │  (wait permit)  │ ─────────▶ │   POST /orders  │ ─────────▶ │ Tracker         │
     └─────────────────┘            └─────────────────┘            └────────┬────────┘
                                                                            │
                                    ┌───────────────────────────────────────┘
                                    ▼
                           ┌─────────────────┐            ┌─────────────────┐
                           │  WebSocket      │ ─────────▶ │  Chain Client   │
                           │  (status=Finish)│  trigger   │  balanceOf()    │
                           └─────────────────┘            └─────────────────┘
```

## Data Flow Summary

| Step | Component | Action | Latency |
|------|-----------|--------|---------|
| 1 | Strategy | Generate OrderIntent | User code |
| 2 | Priority Queue | Enqueue with priority | <0.1ms |
| 3 | Rate Limiter | Wait for permit | 0-66ms |
| 4 | EIP-712 Signer | Sign order | <0.3ms |
| 5 | REST Client | POST /orders | 30-80ms |
| 6 | WebSocket | Wait for status=Finish | 3-5s |
| 7 | Chain Client | Query balanceOf() | 50-100ms |
| 8 | Confirmation | Calculate actual fill | <0.1ms |

**Total: 3.1-5.5 seconds** (dominated by chain confirmation)

## Module Decoupling Principles

1. **Data Plane modules have NO dependencies on Execution Plane**
   - WebSocket, REST, Chain clients are pure I/O
   - They emit events, don't process business logic

2. **Execution Plane modules communicate via events and channels**
   - `broadcast::Sender<WsEvent>` for WebSocket events
   - `mpsc::Sender<ConfirmationEvent>` for confirmations
   - No direct struct references between modules

3. **Configuration is injected, not hardcoded**
   - All modules receive `&Config` or specific config structs
   - Environment variables loaded via `dotenvy`

4. **Each module is independently testable**
   - Mock configs for unit tests
   - Integration tests with `#[ignore]` for live API

---

Table of Contents
1. Project Overview (#1-project-overview)
2. Research Findings (#2-research-findings)
3. Critical Constraints & Parameters (#3-critical-constraints--parameters)
4. Architecture Design (#4-architecture-design)
5. Module Specifications (#5-module-specifications)
6. Code Examples (#6-code-examples)
7. Project Structure (#7-project-structure)
8. Dependencies (#8-dependencies)
9. Test Plan (#9-test-plan)
10. Implementation Order (#10-implementation-order)
11. New Session Prompt (#11-new-session-prompt)
---
1. Project Overview
1.1 Goal
Build a high-frequency trading (HFT) framework for the Opinion Trade prediction market platform on BNB Chain. The framework must achieve:
- Minimum tick-to-trade latency
- Reliable trade confirmation via on-chain verification
- Smart API rate limit handling (15 TPS)
- Multi-market concurrent trading support
1.2 Technology Stack
| Component | Choice | Rationale |
|-----------|--------|-----------|
| Language | Rust | Zero-cost abstractions, memory safety, predictable latency |
| Async Runtime | Tokio | Industry standard for async Rust |
| Ethereum Library | Alloy | 10x faster than ethers-rs, actively maintained |
| EIP-712 Signing | alloy-sol-types + k256 | Pure Rust, no FFI overhead |
| Rate Limiting | Governor | GCRA algorithm, 15ns per check, single AtomicU64 |
| HTTP Client | rrequest + Chrome120 emulation | Bypasses JA3 TLS fingerprinting on all endpoints |
| WebSocket | tokio-tungstenite | Native async WebSocket |
| Concurrent Map | DashMap | Lock-free concurrent HashMap |
1.3 User Requirements
1. Code and comments in English (全英文代码和注释)
2. Test-driven development - All modules must pass tests before approval
3. Design document for Gemini 3.0 Pro audit
4. Multi-agent implementation - Use multiple AI models for parallel development
---
2. Research Findings
2.1 Opinion Trade API Documentation
REST API
| Endpoint | Method | Description |
|----------|--------|-------------|
| /markets | GET | List all markets |
| /orderbook/{pair} | GET | Get orderbook for a trading pair |
| /orders | POST | Create a new order |
| /orders/{id} | DELETE | Cancel an order |
| /orders | GET | Get user's orders |
Base URL: https://proxy.opinion.trade:8443/openapi/
Authentication: 
⚠️ CRITICAL: API key is passed as QUERY PARAMETER, not header!
URL: https://proxy.opinion.trade:8443/openapi/orders?apikey={YOUR_API_KEY}
Rate Limit: 15 TPS (requests per second) default
WebSocket API
Endpoint: wss://ws.opinion.trade?apikey={API_KEY}
Market Channels:
| Channel | Description |
|---------|-------------|
| market.data.{pair} | Real-time orderbook updates |
| market.trade.{pair} | Public trade stream |
User Channels:
| Channel | Description | Events |
|---------|-------------|--------|
| trade.order.update | Order status updates | orderNew, orderFill, orderCancel, orderConfirm |
| trade.record.new | Trade confirmations | Contains txHash, final amounts |
2.2 SDK Reverse Engineering (Python/TypeScript)
EIP-712 Domain (CRITICAL!)
# From Opinion Python SDK
EIP712_DOMAIN = {
    "name": "OPINION CTF Exchange",  # ⚠️ NOTE THE SPACE!
    "version": "1",
    "chainId": 56,
    "verifyingContract": "0xAD1a38cEc043e70E83a3eC30443dB285ED10D774"
}
Order Type Definition
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
Price Precision
⚠️ Price has 3 decimal places: 0.001 - 0.999
   NOT 2 decimals like some other platforms!
2.3 BNB Chain RPC Latency Research
| Provider Type | Latency | Notes |
|---------------|---------|-------|
| Co-location dedicated node | < 10 ms | Best for HFT, expensive |
| Dedicated node (RPC Fast) | < 4 ms | Isolated, excellent performance |
| QuickNode (US) | 45 ms | Best commercial option |
| Atlas Network | 55-58 ms (median) | Stable P95 |
| Public RPC | > 1 second (some) | ⚠️ NOT suitable for HFT |
Key Findings:
- WebSocket subscription is better than HTTP polling (zero latency push vs polling interval)
- BSC block time: 0.75 seconds (post-2025 upgrade)
- BSC finality: ~1.875 seconds
- BSC Fermi upgrade (Jan 2026): Block time → 0.45s, finality → 1.125s
2.4 Rate Limiting Research
Algorithm Comparison
| Algorithm | Burst Support | Memory | CPU | HFT Suitability |
|-----------|---------------|--------|-----|-----------------|
| Token Bucket | Excellent | Higher | Medium | Aggressive strategies |
| Leaky Bucket/GCRA | Limited | Very Low | Very High | Stable strategies |
Governor Crate (Recommended)
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
2.5 Community Feedback (CRITICAL!)
From user conversations about Opinion Trade:
Q: "成交后api推送的数据，不管用order.Shares还是order.FilledShares，都不准"
A: "只要订单状态是Finish就可以去链上查持仓了"
Q: "链上有3-5秒延迟，做和pm套利的这个时间太久了"
A: "这已经是最快的了，他上链本身就慢。等订单更新更慢"
Key Takeaways:
1. ⚠️ DO NOT trust WebSocket quantity data (order.Shares, order.FilledShares)
2. ✅ Only trust WebSocket for status = "Finish" signal
3. ✅ On-chain query is the ONLY reliable source for actual fill amounts
4. ⚠️ On-chain confirmation takes 3-5 seconds (unavoidable)
5. API order updates are slower than on-chain
---
3. Critical Constraints & Parameters
3.1 Contract Addresses
| Contract | Address | Network |
|----------|---------|---------|
| ConditionalTokens (ERC-1155) | 0xAD1a38cEc043e70E83a3eC30443dB285ED10D774 | BNB Chain |
| MultiSend | 0x998739BFdAAdde7C933B942a68053933098f9EDa | BNB Chain |
3.2 API Endpoints
| Service | URL |
|---------|-----|
| REST API | https://proxy.opinion.trade:8443/openapi/ |
| WebSocket | wss://ws.opinion.trade |
3.3 Chain Parameters
| Parameter | Value |
|-----------|-------|
| Chain ID | 56 (BNB Chain Mainnet) |
| Block Time | 0.75 seconds |
| Finality | ~1.875 seconds |
| On-chain Confirmation | 3-5 seconds |
3.4 API Limits
| Limit | Value |
|-------|-------|
| Rate Limit | 15 TPS (default) |
| Price Precision | 3 decimals (0.001 - 0.999) |
3.5 EIP-712 Constants
// ⚠️ CRITICAL: Note the SPACE in domain name!
const EIP712_DOMAIN_NAME: &str = "OPINION CTF Exchange";
const EIP712_DOMAIN_VERSION: &str = "1";
const CHAIN_ID: u64 = 56;
const VERIFYING_CONTRACT: &str = "0xAD1a38cEc043e70E83a3eC30443dB285ED10D774";
// Order Type Hash
const ORDER_TYPEHASH: &str = "Order(uint256 salt,address maker,address signer,address taker,uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint256 expiration,uint256 nonce,uint256 feeRateBps,uint8 side,uint8 signatureType)";
3.6 DO NOT List
❌ DO NOT use Header for API key - MUST use query parameter
❌ DO NOT assume 2 decimal price - MUST use 3 decimals
❌ DO NOT use ethers-rs - Use Alloy (10x faster)
❌ DO NOT block async I/O - CLAUDE.md rules require non-blocking
❌ DO NOT exceed 15 TPS - API will reject
❌ DO NOT trust WebSocket quantity data - Use on-chain query
---
4. Architecture Design
4.1 Three-Plane Architecture
┌─────────────────────────────────────────────────────────────────────────────┐
│                    OPINION TRADE HFT FRAMEWORK                              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │                         DATA PLANE                                    │  │
│  │                                                                       │  │
│  │  ┌────────────────┐    ┌────────────────┐    ┌──────────────────┐    │  │
│  │  │ WebSocket      │    │ REST Poller    │    │ Chain Subscriber │    │  │
│  │  │ Client         │    │ (Fallback)     │    │ (Position Query) │    │  │
│  │  │                │    │                │    │                  │    │  │
│  │  │ • market.data  │    │ • /markets     │    │ • balanceOf()    │    │  │
│  │  │ • trade.order  │    │ • /orderbook   │    │ • event logs     │    │  │
│  │  │ • trade.record │    │                │    │                  │    │  │
│  │  └───────┬────────┘    └───────┬────────┘    └────────┬─────────┘    │  │
│  │          │                     │                      │              │  │
│  │          └─────────────────────┼──────────────────────┘              │  │
│  │                                ▼                                     │  │
│  │                    ┌───────────────────────┐                         │  │
│  │                    │     Event Bus         │                         │  │
│  │                    │  (tokio::broadcast)   │                         │  │
│  │                    └───────────┬───────────┘                         │  │
│  └────────────────────────────────┼─────────────────────────────────────┘  │
│                                   │                                        │
│  ┌────────────────────────────────┼─────────────────────────────────────┐  │
│  │                         CONTROL PLANE                                │  │
│  │                                ▼                                     │  │
│  │  ┌────────────────┐    ┌───────────────┐    ┌───────────────────┐   │  │
│  │  │ Strategy       │    │ Position      │    │ Risk Manager      │   │  │
│  │  │ Engine         │    │ Manager       │    │                   │   │  │
│  │  │                │    │               │    │ • Circuit Breaker │   │  │
│  │  │ (User impl)    │    │ • On-chain    │    │ • Max Position    │   │  │
│  │  │                │    │   positions   │    │ • Loss Limits     │   │  │
│  │  └───────┬────────┘    └───────────────┘    └───────────────────┘   │  │
│  │          │                                                           │  │
│  │          ▼ OrderIntent                                               │  │
│  └────────────────────────────────┼─────────────────────────────────────┘  │
│                                   │                                        │
│  ┌────────────────────────────────┼─────────────────────────────────────┐  │
│  │                        EXECUTION PLANE                               │  │
│  │                                ▼                                     │  │
│  │  ┌─────────────────────────────────────────────────────────────────┐│  │
│  │  │                   Rate Limiter (Governor GCRA)                  ││  │
│  │  │                   15 TPS Token Bucket + Burst Control           ││  │
│  │  └──────────────────────────────┬──────────────────────────────────┘│  │
│  │                                 │                                    │  │
│  │  ┌────────────┐   ┌─────────────▼────────────┐   ┌────────────────┐ │  │
│  │  │ EIP-712    │   │ Priority Queue           │   │ Order Gateway  │ │  │
│  │  │ Signer     │   │                          │   │                │ │  │
│  │  │            │   │ • Critical (stop-loss)   │   │ • REST Client  │ │  │
│  │  │ • Precomp  │   │ • High (arbitrage)       │   │ • Retry Logic  │ │  │
│  │  │   Domain   │   │ • Medium (strategy)      │   │ • Error Handle │ │  │
│  │  │ • k256     │   │ • Low (market making)    │   │                │ │  │
│  │  └────────────┘   └──────────────────────────┘   └───────┬────────┘ │  │
│  │                                                          │          │  │
│  │  ┌───────────────────────────────────────────────────────▼────────┐ │  │
│  │  │               Confirmation Tracker (Hybrid)                    │ │  │
│  │  │                                                                │ │  │
│  │  │  Trigger: WebSocket trade.order.update (status = "Finish")    │ │  │
│  │  │  ⚠️ DO NOT trust WebSocket quantity data!                     │ │  │
│  │  │                                                                │ │  │
│  │  │  Source of Truth: On-chain balanceOf() query                  │ │  │
│  │  │  Latency: 3-5 seconds (unavoidable chain confirmation)        │ │  │
│  │  └────────────────────────────────────────────────────────────────┘ │  │
│  └─────────────────────────────────────────────────────────────────────┘  │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
4.2 Order Confirmation Flow
┌─────────────────────────────────────────────────────────────────────────────┐
│                     CONFIRMATION FLOW (Final Design)                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌──────────┐     ┌──────────────┐     ┌──────────────────────┐            │
│  │ Submit   │ ──▶ │ API returns  │ ──▶ │ Record:              │            │
│  │ Order    │     │ order_id     │     │ • order_id           │            │
│  │ (REST)   │     │              │     │ • token_id           │            │
│  └──────────┘     └──────────────┘     │ • position_before    │            │
│                                        └──────────┬───────────┘            │
│                                                   │                         │
│                                                   ▼                         │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │           WebSocket: trade.order.update                             │    │
│  │           Wait for: status = "Finish"                               │    │
│  │                                                                     │    │
│  │  ⚠️ IGNORE: order.Shares, order.FilledShares (UNRELIABLE!)         │    │
│  │  ✅ ONLY USE: status field as trigger signal                       │    │
│  └────────────────────────────────┬───────────────────────────────────┘    │
│                                   │ status = "Finish"                      │
│                                   ▼                                         │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │           On-Chain Query (ONLY RELIABLE SOURCE)                     │    │
│  │                                                                     │    │
│  │  Contract: ConditionalTokens (ERC-1155)                            │    │
│  │  Address:  0xAD1a38cEc043e70E83a3eC30443dB285ED10D774              │    │
│  │  Method:   balanceOf(account, positionId) → uint256                │    │
│  │                                                                     │    │
│  │  Latency: 3-5 seconds (chain confirmation, UNAVOIDABLE)            │    │
│  └────────────────────────────────┬───────────────────────────────────┘    │
│                                   │                                         │
│                                   ▼                                         │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │           Calculate Actual Fill                                     │    │
│  │                                                                     │    │
│  │  position_after = balanceOf(account, tokenId)                      │    │
│  │                                                                     │    │
│  │  if side == BUY:                                                   │    │
│  │      actual_fill = position_after - position_before                │    │
│  │  else: // SELL                                                     │    │
│  │      actual_fill = position_before - position_after                │    │
│  └────────────────────────────────┬───────────────────────────────────┘    │
│                                   │                                         │
│                                   ▼                                         │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │           Update Local State & Emit Callback                        │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
4.3 Latency Budget
┌─────────────────────────────────────────────────────────────────────────────┐
│                         LATENCY BREAKDOWN                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Stage                      Target Latency    Optimization                  │
│  ───────────────────────────────────────────────────────────────────────    │
│  1. WebSocket Parse         < 0.05 ms        simd-json                      │
│  2. Strategy Signal         < 0.5 ms         Pre-computed caches            │
│  3. Position Snapshot       50-100 ms        RPC call (can cache)           │
│  4. EIP-712 Sign            < 0.3 ms         Precomputed domain/typehash    │
│  5. Rate Limiter Check      0.000015 ms      Governor GCRA (15ns)           │
│  6. Priority Queue          < 0.1 ms         BinaryHeap + Mutex             │
│  7. HTTP POST               30-80 ms         HTTP/2 connection pool         │
│  8. API Processing          10-30 ms         (uncontrollable)               │
│  9. Chain Confirmation      3000-5000 ms     ⚠️ UNAVOIDABLE                 │
│  10. WS Notification        ~0 ms            Event push                      │
│  11. Chain Query            50-100 ms        RPC call                        │
│                                                                             │
│  ───────────────────────────────────────────────────────────────────────    │
│  TOTAL (Tick-to-Submit)     80-350 ms                                       │
│  TOTAL (Tick-to-Confirm)    3.1-5.5 seconds  Chain confirmation dominates   │
│                                                                             │
│  ⚡ After BSC Fermi Upgrade (2026.01):                                      │
│     Block time 0.45s → Total ~2-3 seconds                                   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
---
5. Module Specifications
5.1 Auth Module (src/auth/)
EIP-712 Signer
Purpose: Sign orders using EIP-712 typed data with maximum performance.
Key Optimizations:
1. Precompute Domain Separator at initialization (only once)
2. Precompute Type Hash at compile time
3. Use k256 directly for ECDSA (avoid ethers overhead)
Interface:
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
Test Requirements:
- [ ] Domain separator matches Python SDK output
- [ ] Signature validates against Opinion API
- [ ] Benchmark: < 0.5ms per signature
---
5.2 Rate Limiter Module (src/execution/rate_limiter.rs)
Purpose: Enforce 15 TPS API limit using GCRA algorithm.
Configuration:
pub struct RateLimiterConfig {
    pub requests_per_second: u32,  // Default: 15
    pub burst_capacity: u32,       // Default: 3 (conservative)
}
Interface:
pub trait RateLimiter: Send + Sync {
    async fn acquire(&self);           // Wait for permit
    fn try_acquire(&self) -> bool;     // Non-blocking check
    fn next_available(&self) -> Option<Instant>;
}
Test Requirements:
- [ ] 15 TPS limit enforced
- [ ] Burst allows N immediate requests
- [ ] No requests dropped (only delayed)
- [ ] Benchmark: < 100ns per check
---
5.3 Priority Queue Module (src/execution/priority_queue.rs)
Priority Levels:
pub enum OrderPriority {
    Critical = 4,  // Stop-loss, liquidation
    High = 3,      // Arbitrage
    Medium = 2,    // Strategy signals
    Low = 1,       // Market making
}
Ordering Rules:
1. Higher priority first
2. Same priority: FIFO (earlier timestamp first)
3. Past deadline: dropped
Test Requirements:
- [ ] Priority ordering correct
- [ ] FIFO within same priority
- [ ] Expired orders dropped
- [ ] Thread-safe concurrent access
---
5.4 WebSocket Client (src/data/websocket/)
Endpoint: wss://ws.opinion.trade?apikey={API_KEY}
Channels:
| Channel | Purpose | Trust Level |
|---------|---------|-------------|
| market.data.{pair} | Orderbook | Trusted |
| trade.order.update | Order status | Only trust status |
| trade.record.new | Trade records | ⚠️ UNRELIABLE |
Reconnection: Exponential backoff (1s, 2s, 4s, 8s, 16s max)
Test Requirements:
- [ ] Connection establishment
- [ ] Subscription success
- [ ] Message parsing
- [ ] Reconnection after disconnect
- [ ] Heartbeat handling
---
5.5 REST Client (src/data/rest/)
Endpoints:
| Endpoint | Method | Purpose |
|----------|--------|---------|
| /markets | GET | List markets |
| /orderbook/{pair} | GET | Get orderbook |
| /orders | POST | Create order |
| /orders/{id} | DELETE | Cancel order |
Authentication:
⚠️ API key in QUERY PARAMETER, not header!
URL: https://proxy.opinion.trade:8443/openapi/orders?apikey={KEY}

TLS Fingerprinting (CRITICAL):
Kong API Gateway uses JA3 TLS fingerprinting, blocking standard HTTP clients (reqwest, hyper, etc.) for ALL endpoints.

**Solution**: Pure `rrequest` library with Chrome120 TLS emulation
- Implementation: `unified_client.rs` uses `rrequest` crate with `Emulation::Chrome120`
- Environment-aware proxy configuration:
  - **Local Development** (`OPINION_ENV=local`): Routes through proxy (`OPINION_PROXY_URL`) to bypass TLS fingerprinting
  - **AWS Tokyo** (`OPINION_ENV=aws` or unset): Direct connection (no TLS blocking in cloud environment)
- Performance: Native async performance with tokio, no subprocess overhead, connection pooling, HTTP/2 support

**Configuration**:
- `src/config.rs::Environment` enum defines environment detection
- Default: AWS (production-safe)
- Chrome120 TLS emulation applied in both environments

Test Requirements:
- [ ] GET requests work
- [ ] POST with JSON body works
- [ ] API key in query params
- [ ] Timeout handling
- [ ] Retry on 5xx
- [ ] Chrome120 TLS emulation bypasses Kong Gateway JA3 fingerprinting
- [ ] Environment detection works correctly (local vs AWS)
- [ ] Proxy configuration works in local environment
---
5.6 Chain Client (src/chain/)
Contract: ConditionalTokens (ERC-1155)
Methods:
function balanceOf(address account, uint256 id) → uint256
function balanceOfBatch(address[] accounts, uint256[] ids) → uint256[]

RPC Retry Mechanism:
Automatic retry with exponential backoff for unreliable RPC endpoints:
- Max attempts: 3
- Backoff strategy: Exponential (1s, 2s, 4s)
- Retriable errors: timeout, connection errors, HTTP 503, HTTP 429
- Applied methods: `balance_of()`, `balance_of_batch()`

Benefits:
- Improved reliability on public RPC endpoints
- Handles transient network issues gracefully
- No manual retry logic needed in strategy code

Test Requirements:
- [ ] Balance query works
- [ ] Batch query works
- [ ] Error handling for RPC failures
- [ ] Retry mechanism triggers on transient errors
- [ ] Non-retriable errors fail immediately
---
5.7 Confirmation Tracker (src/execution/confirmation.rs)
Flow:
1. Order submitted → Record order_id, token_id, position_before
2. WebSocket status = "Finish" → Trigger chain query
3. Query balanceOf(account, tokenId)
4. Calculate: actual_fill = position_after - position_before
5. Emit confirmation callback
Test Requirements:
- [ ] Position snapshot before order
- [ ] Trigger on WebSocket "Finish"
- [ ] On-chain query returns correct balance
- [ ] Actual fill calculation
- [ ] Timeout handling
---
5.8 Order Gateway (src/execution/gateway.rs)
Flow:
pub async fn submit_order(&self, intent: OrderIntent) -> Result<OrderResult> {
    // 1. Validate
    // 2. Snapshot position
    // 3. Build order struct
    // 4. Sign (EIP-712)
    // 5. Wait for rate limit
    // 6. POST to API
    // 7. Register for confirmation tracking
}
Retry Strategy:
| Error | Action |
|-------|--------|
| 429 Rate Limited | Wait retry-after, retry |
| 5xx Server Error | Exponential backoff, max 3 |
| 4xx Client Error | No retry, return error |
| Timeout | Retry once, then fail |
---
6. Code Examples
6.1 EIP-712 Signer
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
        let verifying_key = signing_key.verifying_key();
        let address = Address::from_public_key(verifying_key);
        
        // Precompute domain separator
        let domain_separator = Self::compute_domain_separator();
        
        // Precompute type hash
        let order_type_hash = keccak256(
            b"Order(uint256 salt,address maker,address signer,address taker,uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint256 expiration,uint256 nonce,uint256 feeRateBps,uint8 side,uint8 signatureType)"
        );
        
        Self {
            domain_separator,
            order_type_hash,
            signing_key,
            address,
        }
    }
    
    fn compute_domain_separator() -> B256 {
        let type_hash = keccak256(
            b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
        );
        
        let name_hash = keccak256(b"OPINION CTF Exchange"); // Note the space!
        let version_hash = keccak256(b"1");
        let chain_id = U256::from(56);
        let verifying_contract = "0xAD1a38cEc043e70E83a3eC30443dB285ED10D774"
            .parse::<Address>()
            .unwrap();
        
        let encoded = [
            type_hash.as_slice(),
            name_hash.as_slice(),
            version_hash.as_slice(),
            &chain_id.to_be_bytes::<32>(),
            &verifying_contract.into_word().0,
        ].concat();
        
        keccak256(&encoded)
    }
    
    pub fn sign_order(&self, order: &Order) -> Signature {
        let struct_hash = self.hash_order_struct(order);
        
        let message = [
            &[0x19, 0x01][..],
            self.domain_separator.as_slice(),
            struct_hash.as_slice(),
        ].concat();
        
        let digest = keccak256(&message);
        
        let (sig, recovery_id) = self.signing_key
            .sign_prehash_recoverable(digest.as_slice())
            .expect("signing failed");
        
        Signature {
            r: U256::from_be_slice(&sig.r().to_bytes()),
            s: U256::from_be_slice(&sig.s().to_bytes()),
            v: recovery_id.to_byte() + 27,
        }
    }
}
6.2 Rate Limiter
use governor::{Quota, RateLimiter, state::{InMemoryState, NotKeyed}};
use governor::clock::DefaultClock;
use std::num::NonZeroU32;
use std::sync::Arc;
pub struct OpinionRateLimiter {
    inner: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
}
impl OpinionRateLimiter {
    pub fn new(rps: u32, burst: u32) -> Self {
        let quota = Quota::per_second(NonZeroU32::new(rps).unwrap())
            .allow_burst(NonZeroU32::new(burst).unwrap());
        
        Self {
            inner: Arc::new(RateLimiter::direct(quota)),
        }
    }
    
    pub async fn acquire(&self) {
        self.inner.until_ready().await;
    }
    
    pub fn try_acquire(&self) -> bool {
        self.inner.check().is_ok()
    }
}
6.3 Priority Queue
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::time::Instant;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderPriority {
    Critical = 4,
    High = 3,
    Medium = 2,
    Low = 1,
}
pub struct PrioritizedOrder {
    pub order: OrderIntent,
    pub priority: OrderPriority,
    pub enqueue_time: Instant,
    pub deadline: Option<Instant>,
}
impl Ord for PrioritizedOrder {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.priority as u8).cmp(&(other.priority as u8)) {
            Ordering::Equal => other.enqueue_time.cmp(&self.enqueue_time),
            ord => ord,
        }
    }
}
impl PartialOrd for PrioritizedOrder {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
6.4 Confirmation Tracker
use dashmap::DashMap;
use std::time::Instant;
pub struct ConfirmationTracker {
    pending: DashMap<String, PendingOrder>,
    chain_client: Arc<ChainClient>,
}
struct PendingOrder {
    order_id: String,
    token_id: U256,
    side: Side,
    position_before: U256,
    submitted_at: Instant,
}
impl ConfirmationTracker {
    pub async fn on_order_finish(&self, order_id: &str) {
        let pending = match self.pending.get(order_id) {
            Some(p) => p.clone(),
            None => return,
        };
        
        // Query on-chain position
        let position_after = self.chain_client
            .balance_of(&pending.token_id)
            .await
            .unwrap_or(U256::ZERO);
        
        // Calculate actual fill
        let actual_fill = match pending.side {
            Side::Buy => position_after.saturating_sub(pending.position_before),
            Side::Sell => pending.position_before.saturating_sub(position_after),
        };
        
        // Remove from pending
        self.pending.remove(order_id);
        
        // Emit callback...
    }
}
---
7. Project Structure
opinion-hft/
├── Cargo.toml
├── README.md
├── docs/
│   └── DESIGN.md
│
├── src/
│   ├── lib.rs
│   ├── config.rs
│   ├── error.rs
│   │
│   ├── auth/
│   │   ├── mod.rs
│   │   ├── eip712.rs
│   │   ├── signer.rs
│   │   └── tests.rs
│   │
│   ├── data/
│   │   ├── mod.rs
│   │   ├── websocket/
│   │   │   ├── mod.rs
│   │   │   ├── client.rs
│   │   │   ├── messages.rs
│   │   │   ├── channels.rs
│   │   │   └── tests.rs
│   │   ├── rest/
│   │   │   ├── mod.rs
│   │   │   ├── client.rs
│   │   │   ├── endpoints.rs
│   │   │   ├── models.rs
│   │   │   └── tests.rs
│   │   └── event_bus.rs
│   │
│   ├── execution/
│   │   ├── mod.rs
│   │   ├── rate_limiter.rs
│   │   ├── priority_queue.rs
│   │   ├── gateway.rs
│   │   ├── confirmation.rs
│   │   └── tests.rs
│   │
│   ├── chain/
│   │   ├── mod.rs
│   │   ├── client.rs
│   │   ├── conditional_tokens.rs
│   │   └── tests.rs
│   │
│   └── types/
│       ├── mod.rs
│       ├── order.rs
│       ├── market.rs
│       └── position.rs
│
├── tests/
│   ├── integration/
│   │   ├── order_flow_test.rs
│   │   ├── websocket_test.rs
│   │   └── chain_test.rs
│   └── common/
│       └── mod.rs
│
└── examples/
    ├── simple_order.rs
    ├── market_maker.rs
    └── arbitrage.rs
---
8. Dependencies
[package]
name = "opinion-hft"
version = "0.1.0"
edition = "2021"
rust-version = "1.75"
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
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
# Error Handling
thiserror = "1.0"
anyhow = "1.0"
# Utilities
hex = "0.4"
rand = "0.8"
chrono = { version = "0.4", features = ["serde"] }
[dev-dependencies]
tokio-test = "0.4"
wiremock = "0.6"
mockall = "0.13"
proptest = "1.4"
criterion = "0.5"
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
---
9. Test Plan
9.1 Unit Tests
| Module | Test Cases | Priority |
|--------|------------|----------|
| auth/eip712 | Domain separator, type hash, signature | P0 |
| execution/rate_limiter | TPS enforcement, burst, timing | P0 |
| execution/priority_queue | Ordering, expiration, concurrency | P0 |
| data/websocket | Message parsing, reconnection | P1 |
| data/rest | Request building, error handling | P1 |
| execution/confirmation | Position diff calculation | P0 |
| chain/conditional_tokens | Balance decoding | P1 |
9.2 Integration Tests
| Test | Description |
|------|-------------|
| order_flow | Full order lifecycle with mocks |
| websocket_reconnect | Disconnect/reconnect handling |
| rate_limit_backpressure | Queue behavior under load |
9.3 Acceptance Criteria
✅ Unit test coverage > 80%
✅ All integration tests pass
✅ No compiler warnings
✅ clippy passes with no warnings
✅ cargo fmt check passes
---
10. Implementation Order
Phase 1: Foundation (No external dependencies)
├── 1.1 types/order.rs
├── 1.2 types/market.rs
├── 1.3 error.rs
├── 1.4 config.rs
└── 1.5 auth/eip712.rs + tests
Phase 2: Core Execution (Internal only)
├── 2.1 execution/rate_limiter.rs + tests
├── 2.2 execution/priority_queue.rs + tests
└── 2.3 chain/conditional_tokens.rs + tests
Phase 3: External Communication
├── 3.1 data/rest/client.rs + tests (with mocks)
├── 3.2 data/websocket/client.rs + tests (with mocks)
└── 3.3 chain/client.rs + tests (with mocks)
Phase 4: Integration
├── 4.1 execution/confirmation.rs + tests
├── 4.2 execution/gateway.rs + tests
└── 4.3 Integration tests
Phase 5: Examples & Documentation
├── 5.1 examples/simple_order.rs
├── 5.2 examples/market_maker.rs
└── 5.3 Final documentation
---
11. New Session Prompt
Copy and paste this prompt to start a new implementation session:
---
# Opinion Trade HFT Framework - Implementation Request
## Context
I have a complete design document for an Opinion Trade HFT framework in Rust. The design has been reviewed and approved. Now I need to implement it.
## Project: opinion-hft
A high-frequency trading framework for Opinion Trade prediction market on BNB Chain.
## Key Requirements
1. **Code and comments in English**
2. **Test-driven development** - All modules must pass tests before approval
3. **Use multiple AI agents for parallel implementation**
## Critical Parameters (MUST USE EXACTLY)
- **EIP-712 Domain Name**: `"OPINION CTF Exchange"` (note the SPACE!)
- **EIP-712 Version**: `"1"`
- **Chain ID**: `56` (BNB Chain)
- **ConditionalTokens Address**: `0xAD1a38cEc043e70E83a3eC30443dB285ED10D774`
- **REST API Base**: `https://proxy.opinion.trade:8443/openapi/`
- **WebSocket**: `wss://ws.opinion.trade`
- **API Key**: Query parameter (NOT header!) `?apikey={KEY}`
- **Rate Limit**: 15 TPS
- **Price Precision**: 3 decimals (0.001 - 0.999)
## Critical Design Decision
**Order Confirmation**: 
- ⚠️ DO NOT trust WebSocket quantity data (Shares, FilledShares are UNRELIABLE)
- ✅ Only use WebSocket `status = "Finish"` as trigger
- ✅ Query on-chain `balanceOf()` for actual fill amounts
- On-chain confirmation takes 3-5 seconds (unavoidable)
## Implementation Order
Phase 1: Foundation
- types/order.rs, types/market.rs
- error.rs, config.rs
- auth/eip712.rs + tests
Phase 2: Core Execution
- execution/rate_limiter.rs + tests (Governor GCRA, 15 TPS)
- execution/priority_queue.rs + tests
- chain/conditional_tokens.rs + tests
Phase 3: External Communication
- data/rest/client.rs + tests
- data/websocket/client.rs + tests
- chain/client.rs + tests
Phase 4: Integration
- execution/confirmation.rs + tests
- execution/gateway.rs + tests
- Integration tests
## Technology Stack
- Rust 1.75+
- Tokio async runtime
- Alloy (NOT ethers-rs) for Ethereum
- Governor for rate limiting
- reqwest for HTTP
- tokio-tungstenite for WebSocket
- DashMap for concurrent data structures
## Acceptance Criteria
- All unit tests pass
- All integration tests pass
- No compiler warnings
- clippy clean
- cargo fmt clean
## Start Implementation
Please begin with Phase 1. Create the project structure and implement the foundation modules with tests. Each module must pass tests before moving to the next.
---
