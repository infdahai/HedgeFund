use std::collections::HashMap;
use std::fs;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use tracing::info;

use crate::data::rest::models::{ApiMarket, MarketListResult, OpinionResponse};

pub(crate) fn load_market_metadata(path: Option<&str>) -> Result<HashMap<String, i64>> {
    let Some(path) = path else {
        return Ok(HashMap::new());
    };

    let raw = fs::read_to_string(path).with_context(|| format!("read market metadata {path}"))?;
    let markets = parse_market_metadata(&raw)?;
    let mut map = HashMap::new();
    let mut missing = 0usize;

    for market in markets {
        let resolution_ts = market.resolved_at.or(market.cutoff_at);
        if let Some(ts) = resolution_ts {
            map.insert(market.market_id.to_string(), ts);
        } else {
            missing += 1;
        }
    }

    info!(
        source = %path,
        total = map.len() + missing,
        with_resolution = map.len(),
        missing_resolution = missing,
        "market metadata loaded"
    );

    Ok(map)
}

fn parse_market_metadata(raw: &str) -> Result<Vec<ApiMarket>> {
    if let Ok(response) = serde_json::from_str::<OpinionResponse<MarketListResult>>(raw) {
        if let Some(result) = response.result {
            return Ok(result.list);
        }
    }

    if let Ok(result) = serde_json::from_str::<MarketListResult>(raw) {
        return Ok(result.list);
    }

    if let Ok(list) = serde_json::from_str::<Vec<ApiMarket>>(raw) {
        return Ok(list);
    }

    let value: Value = serde_json::from_str(raw).context("invalid market metadata json")?;
    if let Some(list) = value.get("result").and_then(|v| v.get("list")) {
        if let Ok(markets) = serde_json::from_value::<Vec<ApiMarket>>(list.clone()) {
            return Ok(markets);
        }
    }
    if let Some(list) = value.get("list") {
        if let Ok(markets) = serde_json::from_value::<Vec<ApiMarket>>(list.clone()) {
            return Ok(markets);
        }
    }

    Err(anyhow!("unsupported market metadata format"))
}
