use crate::strategy::core::OrderbookSide;

#[derive(Debug, Clone)]
pub struct SnapshotRow {
    pub ts: u64,
    pub market_id: String,
    pub yes_bids: Vec<OrderbookSide>,
    pub yes_asks: Vec<OrderbookSide>,
    pub no_bids: Vec<OrderbookSide>,
    pub no_asks: Vec<OrderbookSide>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::core::{ArbitrageDetector, MarketInfo, OrderbookSide};

    #[test]
    fn test_snapshot_row_yesno_signal() {
        let market = MarketInfo {
            id: "TEST-MARKET".into(),
            question: "Backtest snapshot".into(),
            yes_token_id: "1".into(),
            no_token_id: "2".into(),
            yes_pair: "TEST-YES".into(),
            no_pair: "TEST-NO".into(),
            volume_24h: 0.0,
            liquidity: 0.0,
            is_active: true,
            market_type: None,
            root_market_id: None,
            parent_title: None,
        };

        let snapshot = SnapshotRow {
            ts: 1_700_000_123,
            market_id: market.id.clone(),
            yes_bids: vec![OrderbookSide {
                price: 0.46,
                size: 500.0,
            }],
            yes_asks: vec![OrderbookSide {
                price: 0.48,
                size: 500.0,
            }],
            no_bids: vec![OrderbookSide {
                price: 0.47,
                size: 500.0,
            }],
            no_asks: vec![OrderbookSide {
                price: 0.49,
                size: 500.0,
            }],
        };

        let mut detector = ArbitrageDetector::with_defaults();
        let signal = detector.detect_yesno_arbitrage(
            &market,
            &snapshot.yes_bids,
            &snapshot.yes_asks,
            &snapshot.no_bids,
            &snapshot.no_asks,
            snapshot.ts,
        );

        assert!(signal.is_some());
    }
}
