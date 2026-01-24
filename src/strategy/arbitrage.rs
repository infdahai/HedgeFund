pub use crate::strategy::core::detector::{
    ArbScanStats, ArbitrageConfig, ArbitrageDetector, ArbitrageLeg, ArbitrageOpportunity,
    ArbitrageType, OutcomeOrderbook,
};
pub use crate::strategy::core::fees::{fee_amount, fee_rate, quote_fee, FeeConfig, FeeQuote};
pub use crate::strategy::core::TradeSide;
use crate::types::order::{OrderIntent, OrderSide};
use crate::types::OrderPriority;
use alloy_primitives::U256;
use std::str::FromStr;

pub fn opportunity_to_order_intents(opportunity: &ArbitrageOpportunity) -> Vec<OrderIntent> {
    let priority = match opportunity.arb_type {
        ArbitrageType::NegRisk => OrderPriority::Critical,
        ArbitrageType::YesNo | ArbitrageType::MintSell => OrderPriority::High,
        ArbitrageType::SpreadAnomaly => OrderPriority::Medium,
    };

    opportunity
        .legs
        .iter()
        .map(|leg| {
            let side = match leg.side {
                TradeSide::Buy => OrderSide::Buy,
                TradeSide::Sell => OrderSide::Sell,
            };

            let token_id = U256::from_str(&leg.token_id).unwrap_or(U256::ZERO);
            let quantity = U256::from((leg.size * 1e18) as u128);

            OrderIntent::new(
                opportunity.market_id.clone(),
                token_id,
                side,
                leg.price,
                quantity,
            )
            .with_priority(priority)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::core::{MarketInfo, MarketOrderbook, OrderbookSide};

    const DETECTED_AT: u64 = 1_700_000_000;

    fn mock_market() -> MarketInfo {
        MarketInfo {
            id: "TEST-MARKET".into(),
            question: "Test question?".into(),
            yes_token_id: "1".into(),
            no_token_id: "2".into(),
            yes_pair: "TEST-YES".into(),
            no_pair: "TEST-NO".into(),
            volume_24h: 10000.0,
            liquidity: 5000.0,
            is_active: true,
            market_type: None,
            root_market_id: None,
            parent_title: None,
            resolution_ts: None,
        }
    }

    fn mock_orderbook(pair: &str, best_ask: f64, liquidity: f64) -> MarketOrderbook {
        MarketOrderbook {
            pair: pair.into(),
            bids: vec![OrderbookSide {
                price: best_ask - 0.02,
                size: liquidity,
            }],
            asks: vec![OrderbookSide {
                price: best_ask,
                size: liquidity,
            }],
        }
    }

    #[test]
    fn test_yesno_arbitrage_detected() {
        let mut detector = ArbitrageDetector::with_defaults();
        let market = mock_market();

        // Yes @ 0.48, No @ 0.49 = 0.97 total (3% gross profit)
        let yes_ob = mock_orderbook("TEST-YES", 0.48, 1000.0);
        let no_ob = mock_orderbook("TEST-NO", 0.49, 1000.0);

        let opportunity = detector.detect_yesno_arbitrage(
            &market,
            &yes_ob.bids,
            &yes_ob.asks,
            &no_ob.bids,
            &no_ob.asks,
            DETECTED_AT,
        );

        assert!(opportunity.is_some());
        let opp = opportunity.unwrap();
        assert_eq!(opp.arb_type, ArbitrageType::YesNo);
        assert!((opp.total_cost - 0.97).abs() < 0.0001);
        assert!(opp.gross_profit_rate > 0.02);
        assert!(opp.net_profit_rate > 0.0);
        assert_eq!(opp.legs.len(), 2);
    }

    #[test]
    fn test_mint_sell_arbitrage_detected() {
        let mut detector = ArbitrageDetector::with_defaults();
        let market = mock_market();

        // Yes Bid @ 0.52, No Bid @ 0.51 = 1.03 revenue
        // Mint @ 1.0, Sell @ 1.03 -> 3% gross
        let mut yes_ob = mock_orderbook("TEST-YES", 0.54, 1000.0); // Ask 0.54, Bid 0.52
        let mut no_ob = mock_orderbook("TEST-NO", 0.53, 1000.0); // Ask 0.53, Bid 0.51

        // Fix bids for mock
        yes_ob.bids[0].price = 0.52;
        no_ob.bids[0].price = 0.51;

        let opportunity = detector.detect_mint_sell_arbitrage(
            &market,
            &yes_ob.bids,
            &yes_ob.asks,
            &no_ob.bids,
            &no_ob.asks,
            DETECTED_AT,
        );

        assert!(opportunity.is_some());
        let opp = opportunity.unwrap();
        assert_eq!(opp.arb_type, ArbitrageType::MintSell);
        assert_eq!(opp.total_cost, 1.0);
        assert!((opp.gross_profit_rate - 0.03).abs() < 0.0001);
        assert_eq!(opp.legs[0].side, TradeSide::Sell);
    }

    #[test]
    fn test_neg_risk_arbitrage_detected() {
        let mut detector = ArbitrageDetector::with_defaults();

        let outcomes = vec![
            OutcomeOrderbook {
                outcome_id: "A".into(),
                token_id: "1".into(),
                pair: "TEST-A".into(),
                bids: vec![OrderbookSide {
                    price: 0.20,
                    size: 500.0,
                }],
                asks: vec![OrderbookSide {
                    price: 0.22,
                    size: 500.0,
                }],
            },
            OutcomeOrderbook {
                outcome_id: "B".into(),
                token_id: "2".into(),
                pair: "TEST-B".into(),
                bids: vec![OrderbookSide {
                    price: 0.23,
                    size: 500.0,
                }],
                asks: vec![OrderbookSide {
                    price: 0.25,
                    size: 500.0,
                }],
            },
            OutcomeOrderbook {
                outcome_id: "C".into(),
                token_id: "3".into(),
                pair: "TEST-C".into(),
                bids: vec![OrderbookSide {
                    price: 0.26,
                    size: 500.0,
                }],
                asks: vec![OrderbookSide {
                    price: 0.28,
                    size: 500.0,
                }],
            },
            OutcomeOrderbook {
                outcome_id: "D".into(),
                token_id: "4".into(),
                pair: "TEST-D".into(),
                bids: vec![OrderbookSide {
                    price: 0.18,
                    size: 500.0,
                }],
                asks: vec![OrderbookSide {
                    price: 0.20,
                    size: 500.0,
                }],
            },
        ];

        // Total: 0.22 + 0.25 + 0.28 + 0.20 = 0.95
        let opportunity =
            detector.detect_neg_risk_arbitrage("MULTI-MARKET", &outcomes, DETECTED_AT);

        assert!(opportunity.is_some());
        let opp = opportunity.unwrap();
        assert_eq!(opp.arb_type, ArbitrageType::NegRisk);
        assert!((opp.total_cost - 0.95).abs() < 0.0001);
        assert_eq!(opp.legs.len(), 4);
    }

    #[test]
    fn test_opportunity_summary() {
        let opp = ArbitrageOpportunity {
            id: "arb-yesno-1-1".into(),
            arb_type: ArbitrageType::YesNo,
            market_id: "BTC-100K".into(),
            total_cost: 0.97,
            total_spend: 0.97,
            gross_profit_rate: 0.03,
            net_profit_abs: 0.025,
            net_profit_rate: 0.026,
            max_size: 500.0,
            legs: vec![],
            detected_at: 0,
            is_valid: true,
        };

        let summary = opp.summary();
        assert!(summary.contains("YES/NO"));
        assert!(summary.contains("BTC-100K"));
        assert!(summary.contains("0.9700"));
    }
}
