# Opinion HFT Design Document

## Architecture Overview

### Data Plane (I/O)
- **WebSocketClient**: Real-time market data feed
- **OpinionClient**: REST API client with dual-mode HTTP transport
- **ChainClient**: BNB Chain RPC client with retry mechanism

### Execution Plane (Logic)
- **OrderGateway**: Order routing and execution
- **RateLimiter**: API rate limiting (15 TPS)
- **ConfirmationTracker**: Order fill verification (WebSocket + on-chain)
- **RiskManager**: Pre-trade and post-trade risk controls

## Dual-Mode HTTP Client

### Environment Detection

The bot automatically detects the deployment environment via `OPINION_ENV` variable:

| Environment | Value | HTTP Client | Use Case |
|-------------|-------|-------------|----------|
| Local | `OPINION_ENV=local` | curl subprocess | Dev with proxy (FlClash/TUN) |
| AWS | `OPINION_ENV=aws` | rquest native | Production (direct API access) |
| Default | (unset) | rquest native | Production-safe fallback |

### Performance Comparison

| Metric | Local (curl) | AWS (rquest) |
|--------|--------------|--------------|
| Latency | +10-50ms (subprocess overhead) | Baseline (native async) |
| TLS Fingerprint | Bypassed via system proxy | Not applicable |
| Concurrency | Limited by process spawning | Full async |

### Implementation Details

**src/config.rs**:
```rust
pub enum Environment {
    Local,  // Uses curl subprocess
    Aws,    // Uses native rquest
}
```

**src/data/rest/unified_client.rs**:
- `OpinionClient::new()` detects environment at initialization
- `use_curl` boolean field determines transport mode
- Request methods (`get`, `post`, `delete`) dispatch based on mode

## Order Signing (EIP-712)

Critical configuration for order submission:
- **Domain Name**: `"OPINION CTF Exchange"` (note the space!)
- **Chain ID**: 56 (BNB Chain)
- **Signature Type**: 2 (PolyGnosisSafe)
- **Maker**: Proxy/Safe address (from `/user/auth`)
- **Signer**: EOA address

## Confirmation Flow

1. Submit order via REST API
2. Wait for WebSocket `status="Finish"` event
3. Query on-chain `balanceOf()` for actual filled quantity
4. Target latency: 3-5s (chain confirmation unavoidable)

## Risk Management

### Pre-Trade Checks
- Position limits per market
- Total exposure limits
- Minimum profit threshold

### Post-Trade Monitoring
- Slippage verification
- P&L tracking
- Circuit breaker (3 consecutive losses)

### Drawdown Protection
- Halts if daily drawdown > 10%
- Configurable risk parameters in `RiskConfig`

## Deployment Architecture

### Local Development
```
Developer Machine
├── FlClash/TUN Proxy (utun4)
│   └── Routes curl traffic via VPN
└── Opinion HFT Bot
    ├── OPINION_ENV=local
    └── Uses curl subprocess (bypasses JA3 fingerprint)
```

### AWS Production
```
AWS Tokyo (ap-northeast-1)
├── Direct API access (no proxy)
└── Opinion HFT Bot
    ├── OPINION_ENV=aws
    └── Uses native rquest client (optimal performance)
```

## API Quirks

| API Feature | Correct Usage | Common Mistake |
|-------------|---------------|----------------|
| Auth Header | `apikey: <key>` | `Authorization: Bearer <key>` |
| Quote Token | `/quoteToken?chainId=56` | `/quoteToken/56` |
| Order Endpoint | `/order` (singular) | `/orders` (plural) |
| Cancel Order | `POST /order/cancel` | `DELETE /order/{id}` |
| Side Values | `"0"` (Buy) / `"1"` (Sell) | `"BUY"` / `"SELL"` |
| Price Precision | 3 decimals (0.001-0.999) | 2 decimals |
| Minimum Order | 1.30 USDT | N/A |

## Configuration Files

### .env.example
```bash
OPINION_API_KEY=your_api_key_here
OPINION_EOA_ADDRESS=0x...
OPINION_PRIVATE_KEY=0x...
BSC_RPC_URL=https://bsc-dataseed.binance.org
OPINION_ENV=local  # or 'aws' for production
```

## Testing Strategy

- **Unit Tests**: 63 tests covering core logic
- **Integration Tests**: Live API tests (ignored by default)
- **Dual-Mode Tests**: Environment detection and client mode switching
- **Trading Scenarios**: End-to-end arbitrage workflows

## Future Enhancements

1. **Automatic Environment Detection**
   - IP-based detection (AWS IP ranges)
   - Region detection via EC2 metadata

2. **Performance Monitoring**
   - Latency metrics per transport mode
   - Success rate tracking
   - Auto-fallback on repeated failures

3. **Advanced Proxy Support**
   - SOCKS5 proxy configuration
   - Proxy health monitoring
   - Automatic failover
