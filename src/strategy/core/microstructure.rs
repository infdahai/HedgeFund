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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_orderbook(bids: Vec<(f64, f64)>, asks: Vec<(f64, f64)>) -> MarketOrderbook {
        MarketOrderbook {
            pair: "test".to_string(),
            bids: bids
                .into_iter()
                .map(|(p, s)| OrderbookSide { price: p, size: s })
                .collect(),
            asks: asks
                .into_iter()
                .map(|(p, s)| OrderbookSide { price: p, size: s })
                .collect(),
        }
    }

    #[test]
    fn test_obi_strong_buy() {
        // Heavy bid pressure: 1000 bid vs 100 ask
        let obi = calc_obi(1000.0, 100.0);
        assert!(obi > 0.7, "Expected strong buy signal, got {}", obi);
        assert!(obi < 1.0, "OBI should be < 1.0");
    }

    #[test]
    fn test_obi_strong_sell() {
        // Heavy ask pressure: 100 bid vs 1000 ask
        let obi = calc_obi(100.0, 1000.0);
        assert!(obi < -0.7, "Expected strong sell signal, got {}", obi);
        assert!(obi > -1.0, "OBI should be > -1.0");
    }

    #[test]
    fn test_obi_neutral() {
        let obi = calc_obi(500.0, 500.0);
        assert!(
            (obi - 0.0).abs() < 0.001,
            "Expected neutral OBI ~0, got {}",
            obi
        );
    }

    #[test]
    fn test_obi_zero_depth() {
        let obi = calc_obi(0.0, 0.0);
        assert_eq!(obi, 0.0, "Zero depth should return 0");
    }

    #[test]
    fn test_spread_bps_calculation() {
        let ob = make_orderbook(vec![(0.50, 100.0)], vec![(0.51, 100.0)]);
        let feature =
            MicrostructureFeature::from_orderbook("123", "123-YES", &ob, None, 0).unwrap();

        // spread = 0.51 - 0.50 = 0.01
        // mid = 0.505
        // spread_bps = 0.01 / 0.505 * 10000 ≈ 198.02
        assert!(
            (feature.spread_bps - 198.02).abs() < 1.0,
            "Expected ~198 bps, got {}",
            feature.spread_bps
        );
    }

    #[test]
    fn test_feature_from_orderbook() {
        let ob = make_orderbook(
            vec![(0.60, 800.0), (0.59, 200.0)],
            vec![(0.61, 200.0), (0.62, 100.0)],
        );

        let feature =
            MicrostructureFeature::from_orderbook("market1", "market1-YES", &ob, None, 1000)
                .expect("Should create feature");

        assert_eq!(feature.market_id, "market1");
        assert_eq!(feature.pair, "market1-YES");
        assert_eq!(feature.best_bid, 0.60);
        assert_eq!(feature.best_ask, 0.61);
        assert!((feature.mid_price - 0.605).abs() < 0.001);

        // L1 OBI: (800 - 200) / (800 + 200) = 0.6
        assert!(
            (feature.obi_l1 - 0.6).abs() < 0.01,
            "Expected OBI L1 ~0.6, got {}",
            feature.obi_l1
        );
    }

    #[test]
    fn test_weighted_obi_l5() {
        let bids = vec![
            OrderbookSide {
                price: 0.50,
                size: 100.0,
            },
            OrderbookSide {
                price: 0.49,
                size: 100.0,
            },
            OrderbookSide {
                price: 0.48,
                size: 100.0,
            },
            OrderbookSide {
                price: 0.47,
                size: 100.0,
            },
            OrderbookSide {
                price: 0.46,
                size: 100.0,
            },
        ];
        let asks = vec![
            OrderbookSide {
                price: 0.51,
                size: 50.0,
            },
            OrderbookSide {
                price: 0.52,
                size: 50.0,
            },
        ];

        let (bid_sum, ask_sum, obi) = calc_weighted_obi_l5(&bids, &asks);
        assert_eq!(bid_sum, 500.0);
        assert_eq!(ask_sum, 100.0);
        // OBI = (500 - 100) / (500 + 100) = 400/600 = 0.667
        assert!(
            (obi - 0.667).abs() < 0.01,
            "Expected weighted OBI ~0.667, got {}",
            obi
        );
    }

    #[test]
    fn test_ofi_calculation() {
        let ob1 = make_orderbook(vec![(0.50, 100.0)], vec![(0.51, 100.0)]);
        let feature1 =
            MicrostructureFeature::from_orderbook("m1", "m1-YES", &ob1, None, 0).unwrap();

        // Bids increase, asks decrease
        let ob2 = make_orderbook(vec![(0.50, 150.0)], vec![(0.51, 50.0)]);
        let feature2 =
            MicrostructureFeature::from_orderbook("m1", "m1-YES", &ob2, Some(&feature1), 100)
                .unwrap();

        // OFI = (150 - 100) - (50 - 100) = 50 - (-50) = 100
        assert!(
            (feature2.ofi - 100.0).abs() < 0.001,
            "Expected OFI = 100, got {}",
            feature2.ofi
        );
    }

    #[test]
    fn test_feature_empty_orderbook() {
        let ob = MarketOrderbook {
            pair: "test".to_string(),
            bids: vec![],
            asks: vec![],
        };
        let result = MicrostructureFeature::from_orderbook("m1", "m1-YES", &ob, None, 0);
        assert!(result.is_none(), "Empty orderbook should return None");
    }
}
