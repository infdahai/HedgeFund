use crate::strategy::core::{MarketOrderbook, OrderbookSide};
use serde::{Deserialize, Serialize};

/// Microstructure feature snapshot, calculated on every orderbook update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicrostructureFeature {
    // === Identity ===
    pub market_id: String,
    pub pair: String, // "123-YES" or "123-NO"
    pub timestamp_ms: u64,

    // === Price Data ===
    pub mid_price: f64, // (bid1 + ask1) / 2
    pub best_bid: f64,
    pub best_ask: f64,
    pub spread_abs: f64, // ask1 - bid1
    pub spread_bps: f64, // spread / mid_price * 10000

    // === Level 1 Depth ===
    pub bid_depth_l1: f64, // Best bid size
    pub ask_depth_l1: f64, // Best ask size
    pub obi_l1: f64,       // (B1 - A1) / (B1 + A1), range [-1, 1]

    // === Level 5 Weighted Depth ===
    pub bid_depth_l5: f64, // Top 5 levels bid size sum
    pub ask_depth_l5: f64, // Top 5 levels ask size sum
    pub obi_l5: f64,       // Weighted OBI for top 5 levels

    // === Order Flow (requires history) ===
    pub ofi: f64,     // ΔQ_bid - ΔQ_ask (requires previous snapshot)
    pub ofi_ema: f64, // OFI EMA smoothing

    // === Liquidity Quality ===
    pub depth_ratio: f64,     // bid_depth_l5 / ask_depth_l5
    pub total_liquidity: f64, // bid_depth_l1 + ask_depth_l1

    // === Backtest Labels (Optional) ===
    pub next_price_10s: Option<f64>,   // Future mid price in 10s
    pub next_price_30s: Option<f64>,   // Future mid price in 30s
    pub price_change_10s: Option<f64>, // Price change ratio in 10s
}

impl MicrostructureFeature {
    /// Calculate features from a MarketOrderbook snapshot
    pub fn from_orderbook(
        market_id: &str,
        pair: &str,
        ob: &MarketOrderbook,
        prev: Option<&MicrostructureFeature>,
        timestamp_ms: u64,
    ) -> Option<Self> {
        let best_bid = ob.best_bid()?;
        let best_ask = ob.best_ask()?;
        let mid_price = (best_bid + best_ask) / 2.0;
        let spread_abs = best_ask - best_bid;
        let spread_bps = if mid_price > 0.0 {
            spread_abs / mid_price * 10000.0
        } else {
            0.0
        };

        // Level 1 imbalance
        let bid_depth_l1 = ob.bids.first().map(|b| b.size).unwrap_or(0.0);
        let ask_depth_l1 = ob.asks.first().map(|a| a.size).unwrap_or(0.0);
        let obi_l1 = calc_obi(bid_depth_l1, ask_depth_l1);

        // Level 5 weighted imbalance
        let (bid_depth_l5, ask_depth_l5, obi_l5) = calc_weighted_obi_l5(&ob.bids, &ob.asks);

        // Order Flow Imbalance (delta-based)
        let ofi = calc_ofi(&ob.bids, &ob.asks, prev);
        let ofi_ema = match prev {
            Some(p) => p.ofi_ema * 0.8 + ofi * 0.2, // EMA(5) roughly
            None => ofi,
        };

        let depth_ratio = if ask_depth_l5 > 0.0 {
            bid_depth_l5 / ask_depth_l5
        } else if bid_depth_l5 > 0.0 {
            f64::INFINITY
        } else {
            0.0
        };

        Some(Self {
            market_id: market_id.to_string(),
            pair: pair.to_string(),
            timestamp_ms,
            mid_price,
            best_bid,
            best_ask,
            spread_abs,
            spread_bps,
            bid_depth_l1,
            ask_depth_l1,
            obi_l1,
            bid_depth_l5,
            ask_depth_l5,
            obi_l5,
            ofi,
            ofi_ema,
            depth_ratio,
            total_liquidity: bid_depth_l1 + ask_depth_l1,
            next_price_10s: None,
            next_price_30s: None,
            price_change_10s: None,
        })
    }
}

/// OBI = (Bid - Ask) / (Bid + Ask), range [-1, 1]
fn calc_obi(bid: f64, ask: f64) -> f64 {
    let total = bid + ask;
    if total > 0.0 {
        (bid - ask) / total
    } else {
        0.0
    }
}

/// Price-weighted OBI for top 5 levels
fn calc_weighted_obi_l5(bids: &[OrderbookSide], asks: &[OrderbookSide]) -> (f64, f64, f64) {
    // Simple sum for depth, could apply distance weighting later
    let bid_sum: f64 = bids.iter().take(5).map(|b| b.size).sum();
    let ask_sum: f64 = asks.iter().take(5).map(|a| a.size).sum();
    let obi = calc_obi(bid_sum, ask_sum);
    (bid_sum, ask_sum, obi)
}

/// OFI = ΔQ_bid - ΔQ_ask (simplified)
/// Ideally needs to track price levels to distinguish cancel vs execute
fn calc_ofi(
    bids: &[OrderbookSide],
    asks: &[OrderbookSide],
    prev: Option<&MicrostructureFeature>,
) -> f64 {
    let curr_bid = bids.first().map(|b| b.size).unwrap_or(0.0);
    let curr_ask = asks.first().map(|a| a.size).unwrap_or(0.0);

    match prev {
        Some(p) => {
            // Simplified OFI: just look at L1 changes
            // Real OFI logic:
            // if bid > prev_bid: OFI += size
            // if bid < prev_bid: OFI -= size
            // This is a rough approximation for MVP
            let delta_bid = curr_bid - p.bid_depth_l1;
            let delta_ask = curr_ask - p.ask_depth_l1;
            delta_bid - delta_ask
        }
        None => 0.0,
    }
}
