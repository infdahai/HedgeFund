use rand::seq::SliceRandom;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::data::rest::OpinionClient;
use crate::error::Result;
pub use crate::strategy::core::{MarketInfo, MarketOrderbook, OrderbookSide};

pub struct MarketScanner {
    rest_client: OpinionClient,
    markets_cache: HashMap<String, MarketInfo>,
    last_refresh: Option<Instant>,
    refresh_interval: Duration,
    page_size: u32,
    chain_id: u64,
    fallback_min_active: usize,
}

impl MarketScanner {
    pub fn new(rest_client: OpinionClient, chain_id: u64) -> Self {
        Self {
            rest_client,
            markets_cache: HashMap::new(),
            last_refresh: None,
            refresh_interval: Duration::from_secs(300),
            page_size: 100,
            chain_id,
            fallback_min_active: 20,
        }
    }

    pub async fn refresh_markets_if_stale(&mut self) -> Result<Vec<MarketInfo>> {
        let stale = self
            .last_refresh
            .map(|ts| ts.elapsed() >= self.refresh_interval)
            .unwrap_or(true);
        if !stale {
            return Ok(self.markets_cache.values().cloned().collect());
        }
        self.fetch_all_markets().await
    }

    pub async fn fetch_all_markets(&mut self) -> Result<Vec<MarketInfo>> {
        if self.rest_client.has_master_market_url() {
            let markets = self.fetch_master_market_list().await?;
            self.log_market_info_diagnostics(&markets);

            self.markets_cache.clear();
            for market in &markets {
                self.markets_cache.insert(market.id.clone(), market.clone());
            }
            self.last_refresh = Some(Instant::now());
            return Ok(markets);
        }

        let mut api_markets = self.fetch_markets_paginated(Some("activated")).await?;
        if api_markets.len() < self.fallback_min_active {
            tracing::warn!(
                "Activated market count {} below threshold {}, retrying without status filter",
                api_markets.len(),
                self.fallback_min_active
            );
            api_markets = self.fetch_markets_paginated(None).await?;
        }

        self.log_market_diagnostics(&api_markets);

        let markets: Vec<MarketInfo> = api_markets
            .into_iter()
            .map(|m| self.convert_market(m))
            .collect();

        self.markets_cache.clear();
        for market in &markets {
            self.markets_cache.insert(market.id.clone(), market.clone());
        }
        self.last_refresh = Some(Instant::now());

        Ok(markets)
    }

    async fn fetch_markets_paginated(
        &self,
        status: Option<&str>,
    ) -> Result<Vec<crate::data::rest::models::ApiMarket>> {
        let mut page = 1;
        let mut markets = Vec::new();

        loop {
            let api_markets = self
                .rest_client
                .get_markets_with_filters(
                    status,
                    Some(self.page_size),
                    Some(page),
                    None,
                    Some(self.chain_id),
                    Some(5),
                )
                .await?;

            if api_markets.is_empty() {
                break;
            }

            let api_count = api_markets.len();
            markets.extend(api_markets.into_iter());

            if api_count < self.page_size as usize {
                break;
            }

            page += 1;
        }

        Ok(markets)
    }

    async fn fetch_master_market_list(&self) -> Result<Vec<MarketInfo>> {
        let master_list = self.rest_client.get_master_market_list().await?;
        let mut markets = Vec::new();
        let mut seen: HashSet<i64> = HashSet::new();

        for parent in master_list {
            let parent_title = parent.market_title.clone();
            let parent_id = parent.root_market_id.clone().or(parent.question_id.clone());

            if let Some(topic_id) = parent.topic_id {
                if !seen.insert(topic_id) {
                    continue;
                }
                let id = topic_id.to_string();
                let yes_pair = format!("{}-YES", id);
                let no_pair = format!("{}-NO", id);
                let status_enum = parent
                    .status_enum
                    .clone()
                    .unwrap_or_default()
                    .to_lowercase();
                let is_active = if status_enum.is_empty() {
                    parent.status.unwrap_or_default() == 2
                } else {
                    status_enum == "activated"
                };
                let parse_volume = |value: &Option<String>| -> Option<f64> {
                    value.as_ref().and_then(|v| v.parse().ok())
                };
                let volume_24h = parse_volume(&parent.volume24h)
                    .or_else(|| parse_volume(&parent.volume))
                    .unwrap_or(0.0);
                let yes_token_id = parent
                    .yes_token_id
                    .clone()
                    .or(parent.yes_pos.clone())
                    .unwrap_or_else(|| "0".to_string());
                let no_token_id = parent
                    .no_token_id
                    .clone()
                    .or(parent.no_pos.clone())
                    .unwrap_or_else(|| "0".to_string());
                let question = parent_title
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string());

                markets.push(MarketInfo {
                    id,
                    question,
                    yes_token_id,
                    no_token_id,
                    yes_pair,
                    no_pair,
                    volume_24h,
                    liquidity: 0.0,
                    is_active,
                    market_type: parent.market_type,
                    root_market_id: parent_id.clone(),
                    parent_title: parent_title.clone(),
                });
            }

            let child_list = match parent.child_list {
                Some(children) => children,
                None => continue,
            };

            for child in child_list {
                let child_parent_id = parent_id.clone().or(child.question_id.clone());
                let topic_id = match child.topic_id {
                    Some(value) => value,
                    None => {
                        tracing::debug!("Skipping master list entry without topicId");
                        continue;
                    }
                };
                if !seen.insert(topic_id) {
                    continue;
                }
                let id = topic_id.to_string();
                let yes_pair = format!("{}-YES", id);
                let no_pair = format!("{}-NO", id);
                let status_enum = child.status_enum.clone().unwrap_or_default().to_lowercase();
                let is_active = if status_enum.is_empty() {
                    child.status.unwrap_or_default() == 2
                } else {
                    status_enum == "activated"
                };

                let parse_volume = |value: &Option<String>| -> Option<f64> {
                    value.as_ref().and_then(|v| v.parse().ok())
                };
                let volume_24h = parse_volume(&child.volume24h)
                    .or_else(|| parse_volume(&child.volume))
                    .unwrap_or(0.0);

                let yes_token_id = child
                    .yes_token_id
                    .clone()
                    .or(child.yes_pos.clone())
                    .unwrap_or_else(|| "0".to_string());
                let no_token_id = child
                    .no_token_id
                    .clone()
                    .or(child.no_pos.clone())
                    .unwrap_or_else(|| "0".to_string());
                let child_title = child.market_title.clone();
                let question = child_title
                    .or_else(|| parent_title.clone())
                    .unwrap_or_else(|| "Unknown".to_string());

                markets.push(MarketInfo {
                    id,
                    question,
                    yes_token_id,
                    no_token_id,
                    yes_pair,
                    no_pair,
                    volume_24h,
                    liquidity: 0.0,
                    is_active,
                    market_type: child.market_type.or(parent.market_type),
                    root_market_id: child_parent_id,
                    parent_title: parent_title.clone(),
                });
            }
        }

        Ok(markets)
    }

    fn log_market_info_diagnostics(&self, markets: &[MarketInfo]) {
        let mut status_counts: HashMap<String, usize> = HashMap::new();
        let mut type_counts: HashMap<String, usize> = HashMap::new();
        let mut missing_tokens = 0usize;

        for market in markets {
            let status_key = if market.is_active {
                "activated".to_string()
            } else {
                "inactive".to_string()
            };
            *status_counts.entry(status_key).or_insert(0) += 1;

            let type_key = match market.market_type {
                Some(t) => t.to_string(),
                None => "none".to_string(),
            };
            *type_counts.entry(type_key).or_insert(0) += 1;

            if market.yes_token_id == "0" && market.no_token_id == "0" {
                missing_tokens += 1;
            }
        }

        tracing::info!(
            total_markets = markets.len(),
            missing_tokens,
            ?status_counts,
            ?type_counts,
            "Market list diagnostics (master)"
        );

        if let Some(sample) = markets.first() {
            tracing::debug!(
                market_id = sample.id,
                root_market_id = ?sample.root_market_id,
                market_type = ?sample.market_type,
                yes_token_id = %sample.yes_token_id,
                no_token_id = %sample.no_token_id,
                volume24h = sample.volume_24h,
                "Market sample (master)"
            );
        }
    }

    fn log_market_diagnostics(&self, api_markets: &[crate::data::rest::models::ApiMarket]) {
        let mut status_counts: HashMap<String, usize> = HashMap::new();
        let mut type_counts: HashMap<String, usize> = HashMap::new();
        let mut missing_tokens = 0usize;

        for market in api_markets {
            let status_key = if market.status_enum.is_empty() {
                format!("status:{}", market.status)
            } else {
                market.status_enum.to_lowercase()
            };
            *status_counts.entry(status_key).or_insert(0) += 1;

            let type_key = match market.market_type {
                Some(t) => t.to_string(),
                None => "none".to_string(),
            };
            *type_counts.entry(type_key).or_insert(0) += 1;

            let yes_missing = market.yes_token_id.as_deref().unwrap_or("0") == "0";
            let no_missing = market.no_token_id.as_deref().unwrap_or("0") == "0";
            if yes_missing && no_missing {
                missing_tokens += 1;
            }
        }

        tracing::info!(
            total_markets = api_markets.len(),
            missing_tokens,
            ?status_counts,
            ?type_counts,
            "Market list diagnostics"
        );

        if let Some(sample) = api_markets.first() {
            tracing::debug!(
                market_id = sample.market_id,
                status = sample.status,
                status_enum = %sample.status_enum,
                market_type = ?sample.market_type,
                yes_token_id = ?sample.yes_token_id,
                no_token_id = ?sample.no_token_id,
                volume24h = ?sample.volume24h,
                volume = ?sample.volume,
                "Market sample"
            );
        }
    }

    fn convert_market(&self, m: crate::data::rest::models::ApiMarket) -> MarketInfo {
        let id = m.market_id.to_string();

        let status_enum = m.status_enum.to_lowercase();
        let is_active = if status_enum.is_empty() {
            m.status == 2
        } else {
            status_enum == "activated"
        };

        let yes_pair = format!("{}-YES", id);
        let no_pair = format!("{}-NO", id);

        let parse_volume =
            |value: &Option<String>| -> Option<f64> { value.as_ref().and_then(|v| v.parse().ok()) };
        let volume_24h = parse_volume(&m.volume24h)
            .or_else(|| parse_volume(&m.volume))
            .unwrap_or(0.0);

        MarketInfo {
            id: id.clone(),
            question: m.market_title,
            yes_token_id: m.yes_token_id.unwrap_or("0".to_string()),
            no_token_id: m.no_token_id.unwrap_or("0".to_string()),
            yes_pair,
            no_pair,
            volume_24h,
            liquidity: 0.0, // Not present in ApiMarket, default to 0
            is_active,
            market_type: m.market_type,
            root_market_id: None,
            parent_title: None,
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
            .filter(|m| m.is_active && (m.volume_24h >= min_volume || m.volume_24h == 0.0))
            .collect()
    }

    pub async fn fetch_orderbook(
        &self,
        token_id: &str,
        pair_label: &str,
    ) -> Result<(MarketOrderbook, crate::data::rest::models::TokenOrderbook)> {
        // Use typed client method which calls /token/orderbook?token_id=...
        let token_ob = self.rest_client.get_orderbook(token_id).await?;
        let market_ob = self.convert_orderbook(pair_label, token_ob.clone());
        Ok((market_ob, token_ob))
    }

    pub async fn fetch_categorical_market(
        &self,
        market_id: i64,
    ) -> Result<crate::data::rest::models::CategoricalMarketData> {
        self.rest_client.get_categorical_market(market_id).await
    }

    fn convert_orderbook(
        &self,
        pair: &str,
        ob: crate::data::rest::models::TokenOrderbook,
    ) -> MarketOrderbook {
        let convert_side =
            |entries: Vec<crate::data::rest::models::OrderbookEntry>| -> Vec<OrderbookSide> {
                entries
                    .into_iter()
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
                OrderbookSide {
                    price: 0.45,
                    size: 100.0,
                },
                OrderbookSide {
                    price: 0.44,
                    size: 200.0,
                },
            ],
            asks: vec![
                OrderbookSide {
                    price: 0.47,
                    size: 150.0,
                },
                OrderbookSide {
                    price: 0.48,
                    size: 250.0,
                },
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
