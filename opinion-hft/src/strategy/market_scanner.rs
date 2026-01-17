use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use rand::seq::SliceRandom;

use crate::data::rest::OpinionClient;
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookSide {
    pub price: f64,
    pub size: f64,
}

#[derive(Debug, Clone)]
pub struct MarketOrderbook {
    pub pair: String,
    pub bids: Vec<OrderbookSide>,
    pub asks: Vec<OrderbookSide>,
}

impl MarketOrderbook {
    pub fn best_bid(&self) -> Option<f64> {
        self.bids.first().map(|b| b.price)
    }

    pub fn best_ask(&self) -> Option<f64> {
        self.asks.first().map(|a| a.price)
    }

    pub fn spread(&self) -> Option<f64> {
        match (self.best_ask(), self.best_bid()) {
            (Some(ask), Some(bid)) => Some(ask - bid),
            _ => None,
        }
    }

    pub fn mid_price(&self) -> Option<f64> {
        match (self.best_ask(), self.best_bid()) {
            (Some(ask), Some(bid)) => Some((ask + bid) / 2.0),
            _ => None,
        }
    }
}

pub struct MarketScanner {
    rest_client: OpinionClient,
    markets_cache: HashMap<String, MarketInfo>,
}

impl MarketScanner {
    pub fn new(rest_client: OpinionClient) -> Self {
        Self {
            rest_client,
            markets_cache: HashMap::new(),
        }
    }

    pub async fn fetch_all_markets(&mut self) -> Result<Vec<MarketInfo>> {
        // Use the typed client method
        let api_markets = self.rest_client.get_markets().await?;

        let markets: Vec<MarketInfo> = api_markets.into_iter()
            .map(|m| self.convert_market(m))
            .collect();

        for market in &markets {
            self.markets_cache.insert(market.id.clone(), market.clone());
        }

        Ok(markets)
    }

    fn convert_market(&self, m: crate::data::rest::models::ApiMarket) -> MarketInfo {
        let id = m.market_id.to_string();
        
        // Debug status mapping
        if m.status != 1 {
            tracing::debug!("Market {} status: {} ({})", id, m.status, m.status_enum);
        }

        let yes_pair = format!("{}-YES", id);
        let no_pair = format!("{}-NO", id);
        
        MarketInfo {
            id: id.clone(),
            question: m.market_title,
            yes_token_id: m.yes_token_id.unwrap_or("0".to_string()),
            no_token_id: m.no_token_id.unwrap_or("0".to_string()),
            yes_pair,
            no_pair,
            volume_24h: m.volume24h.and_then(|v| v.parse().ok()).unwrap_or(0.0),
            liquidity: 0.0, // Not present in ApiMarket, default to 0
            is_active: m.status == 2, // 2 = Activated
        }
    }

    // Deprecated: Internal JSON parsing methods removed in favor of typed client
    
    pub fn get_random_markets(&self, count: usize) -> Vec<&MarketInfo> {
        let mut rng = rand::thread_rng();
        let mut markets: Vec<&MarketInfo> = self.markets_cache.values().collect();
        markets.shuffle(&mut rng);
        markets.into_iter().take(count).collect()
    }

    pub fn filter_liquid_markets(&self, min_volume: f64) -> Vec<&MarketInfo> {
        self.markets_cache
            .values()
            .filter(|m| m.is_active && m.volume_24h >= min_volume)
            .collect()
    }

    pub async fn fetch_orderbook(&self, token_id: &str, pair_label: &str) -> Result<MarketOrderbook> {
        // Use typed client method which calls /token/orderbook?token_id=...
        let token_ob = self.rest_client.get_orderbook(token_id).await?;
        
        Ok(self.convert_orderbook(pair_label, token_ob))
    }

    fn convert_orderbook(&self, pair: &str, ob: crate::data::rest::models::TokenOrderbook) -> MarketOrderbook {
        let convert_side = |entries: Vec<crate::data::rest::models::OrderbookEntry>| -> Vec<OrderbookSide> {
            entries.into_iter()
                .filter_map(|e| {
                    Some(OrderbookSide {
                        price: e.price.parse().ok()?,
                        size: e.size.parse().ok()?,
                    })
                })
                .collect()
        };

        MarketOrderbook {
            pair: pair.to_string(),
            bids: convert_side(ob.bids),
            asks: convert_side(ob.asks),
        }
    }

    pub fn get_market(&self, id: &str) -> Option<&MarketInfo> {
        self.markets_cache.get(id)
    }

    pub fn cached_markets(&self) -> &HashMap<String, MarketInfo> {
        &self.markets_cache
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orderbook_calculations() {
        let ob = MarketOrderbook {
            pair: "TEST-YES".into(),
            bids: vec![
                OrderbookSide { price: 0.45, size: 100.0 },
                OrderbookSide { price: 0.44, size: 200.0 },
            ],
            asks: vec![
                OrderbookSide { price: 0.47, size: 150.0 },
                OrderbookSide { price: 0.48, size: 250.0 },
            ],
        };

        assert_eq!(ob.best_bid(), Some(0.45));
        assert_eq!(ob.best_ask(), Some(0.47));
        assert!((ob.spread().unwrap() - 0.02).abs() < 0.0001);
        assert!((ob.mid_price().unwrap() - 0.46).abs() < 0.0001);
    }

    #[test]
    fn test_empty_orderbook() {
        let ob = MarketOrderbook {
            pair: "TEST-YES".into(),
            bids: vec![],
            asks: vec![],
        };

        assert_eq!(ob.best_bid(), None);
        assert_eq!(ob.best_ask(), None);
        assert_eq!(ob.spread(), None);
    }
}
