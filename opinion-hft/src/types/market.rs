use alloy_primitives::U256;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    pub id: String,
    pub pair: String,
    pub question: String,
    pub token_id_yes: U256,
    pub token_id_no: U256,
    pub active: bool,
    pub end_date: Option<u64>,
}

impl Market {
    pub fn token_id(&self, outcome_yes: bool) -> U256 {
        if outcome_yes {
            self.token_id_yes
        } else {
            self.token_id_no
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrderbookLevel {
    pub price: f64,
    pub quantity: U256,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Orderbook {
    pub pair: String,
    pub bids: Vec<OrderbookLevel>,
    pub asks: Vec<OrderbookLevel>,
    pub timestamp: u64,
}

impl Orderbook {
    pub fn best_bid(&self) -> Option<&OrderbookLevel> {
        self.bids.first()
    }

    pub fn best_ask(&self) -> Option<&OrderbookLevel> {
        self.asks.first()
    }

    pub fn spread(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask.price - bid.price),
            _ => None,
        }
    }

    pub fn mid_price(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid.price + ask.price) / 2.0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_token_id() {
        let market = Market {
            id: "test".into(),
            pair: "TEST-YES/TEST-NO".into(),
            question: "Test?".into(),
            token_id_yes: U256::from(100),
            token_id_no: U256::from(101),
            active: true,
            end_date: None,
        };

        assert_eq!(market.token_id(true), U256::from(100));
        assert_eq!(market.token_id(false), U256::from(101));
    }

    #[test]
    fn test_orderbook_spread() {
        let orderbook = Orderbook {
            pair: "TEST".into(),
            bids: vec![OrderbookLevel {
                price: 0.45,
                quantity: U256::from(100),
            }],
            asks: vec![OrderbookLevel {
                price: 0.55,
                quantity: U256::from(100),
            }],
            timestamp: 0,
        };

        assert!((orderbook.spread().unwrap() - 0.1).abs() < f64::EPSILON);
        assert!((orderbook.mid_price().unwrap() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_orderbook_empty() {
        let orderbook = Orderbook::default();
        assert!(orderbook.spread().is_none());
        assert!(orderbook.mid_price().is_none());
    }
}
