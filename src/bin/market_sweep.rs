use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use opinion_hft::config::{ApiConfig, Environment, CHAIN_ID, REST_API_BASE_URL};
use opinion_hft::data::rest::models::{MasterMarketChild, MasterMarketParent};
use opinion_hft::data::rest::OpinionClient;
use opinion_hft::error::{Error, Result};
use opinion_hft::execution::rate_limiter::RateLimiter;
use opinion_hft::telemetry::init_telemetry;
use tracing::info;

struct SweepQuery {
    label: String,
    status: Option<&'static str>,
    market_type: Option<i32>,
    chain_id: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _guards = init_telemetry();

    let (api_config, environment, chain_id) = load_api_config()?;
    let client = OpinionClient::new(&api_config, environment)?;
    let rate_limiter = RateLimiter::new(15, 3);

    if client.has_master_market_url() {
        info!("Master market list detected via OPINION_MASTER_MARKET_URL");
        sweep_master_market_list(&client).await?;
        return Ok(());
    }

    let queries = build_queries(chain_id);
    let mut global_seen: HashSet<i64> = HashSet::new();

    for query in queries {
        info!("Market sweep start: {}", query.label);
        let markets = fetch_markets_paginated(&client, &rate_limiter, &query).await?;
        let (status_counts, type_counts) = summarize_markets(&markets);

        let mut new_markets = 0usize;
        for market in &markets {
            if global_seen.insert(market.market_id) {
                new_markets += 1;
            }
            log_market(&query.label, market);
        }

        info!(
            "Market sweep summary: label={}, total={}, new={}, status_counts={:?}, type_counts={:?}",
            query.label,
            markets.len(),
            new_markets,
            status_counts,
            type_counts
        );
    }

    info!(
        "Market sweep complete: unique_markets={}",
        global_seen.len()
    );
    Ok(())
}

fn load_api_config() -> Result<(ApiConfig, Environment, u64)> {
    let _ = dotenvy::dotenv();

    let api_key = std::env::var("OPINION_API_KEY").map_err(|_| Error::MissingConfig {
        field: "OPINION_API_KEY".into(),
    })?;

    let rest_base_url =
        std::env::var("OPINION_REST_URL").unwrap_or_else(|_| REST_API_BASE_URL.to_string());

    let environment = std::env::var("OPINION_ENV")
        .ok()
        .and_then(|value| Environment::from_str(&value).ok())
        .unwrap_or_default();

    let chain_id = std::env::var("OPINION_CHAIN_ID")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(CHAIN_ID);

    let api_config = ApiConfig {
        api_key,
        rest_base_url,
        ..ApiConfig::default()
    };

    Ok((api_config, environment, chain_id))
}

fn build_queries(chain_id: u64) -> Vec<SweepQuery> {
    vec![
        SweepQuery {
            label: format!("status=activated chain_id={} type=all", chain_id),
            status: Some("activated"),
            market_type: None,
            chain_id: Some(chain_id),
        },
        SweepQuery {
            label: "status=activated chain_id=none type=all".to_string(),
            status: Some("activated"),
            market_type: None,
            chain_id: None,
        },
        SweepQuery {
            label: "status=none chain_id=none type=all".to_string(),
            status: None,
            market_type: None,
            chain_id: None,
        },
        SweepQuery {
            label: "status=activated chain_id=none type=1".to_string(),
            status: Some("activated"),
            market_type: Some(1),
            chain_id: None,
        },
        SweepQuery {
            label: "status=activated chain_id=none type=2".to_string(),
            status: Some("activated"),
            market_type: Some(2),
            chain_id: None,
        },
        SweepQuery {
            label: "status=none chain_id=none type=1".to_string(),
            status: None,
            market_type: Some(1),
            chain_id: None,
        },
        SweepQuery {
            label: "status=none chain_id=none type=2".to_string(),
            status: None,
            market_type: Some(2),
            chain_id: None,
        },
    ]
}

async fn sweep_master_market_list(client: &OpinionClient) -> Result<()> {
    let parents = client.get_master_market_list().await?;
    let (status_counts, type_counts, missing_tokens, unique_entries) =
        summarize_master_market_list(&parents);

    let mut seen: HashSet<i64> = HashSet::new();
    for parent in &parents {
        if let Some(topic_id) = parent.topic_id {
            if !seen.insert(topic_id) {
                continue;
            }
            log_master_market_parent(parent);
        }
        if let Some(children) = &parent.child_list {
            for child in children {
                let topic_id = match child.topic_id {
                    Some(value) => value,
                    None => continue,
                };
                if !seen.insert(topic_id) {
                    continue;
                }
                log_master_market_child(parent, child);
            }
        }
    }

    info!(
        "Master market sweep summary: parents={}, unique_entries={}, missing_tokens={}, status_counts={:?}, type_counts={:?}",
        parents.len(),
        unique_entries,
        missing_tokens,
        status_counts,
        type_counts
    );

    Ok(())
}

async fn fetch_markets_paginated(
    client: &OpinionClient,
    rate_limiter: &RateLimiter,
    query: &SweepQuery,
) -> Result<Vec<opinion_hft::data::rest::ApiMarket>> {
    let mut page = 1;
    let mut markets = Vec::new();
    let page_size = 100u32;

    loop {
        rate_limiter.acquire().await;
        let chunk = client
            .get_markets_with_filters(
                query.status,
                Some(page_size),
                Some(page),
                query.market_type,
                query.chain_id,
                None,
            )
            .await?;

        if chunk.is_empty() {
            break;
        }

        let chunk_len = chunk.len();
        markets.extend(chunk.into_iter());
        if chunk_len < page_size as usize {
            break;
        }

        page += 1;
    }

    Ok(markets)
}

fn summarize_master_market_list(
    parents: &[MasterMarketParent],
) -> (HashMap<String, usize>, HashMap<String, usize>, usize, usize) {
    let mut status_counts: HashMap<String, usize> = HashMap::new();
    let mut type_counts: HashMap<String, usize> = HashMap::new();
    let mut missing_tokens = 0usize;
    let mut total_entries = 0usize;
    let mut seen: HashSet<i64> = HashSet::new();

    let mut record_entry = |status_enum: Option<&String>,
                            status: Option<i32>,
                            market_type: Option<i32>,
                            yes_token_id: Option<&String>,
                            no_token_id: Option<&String>,
                            yes_pos: Option<&String>,
                            no_pos: Option<&String>| {
        let status_key = status_enum
            .map(|s| s.to_lowercase())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                status
                    .map(|s| format!("status:{}", s))
                    .unwrap_or_else(|| "status:unknown".to_string())
            });
        *status_counts.entry(status_key).or_insert(0) += 1;

        let type_key = market_type
            .map(|t| t.to_string())
            .unwrap_or_else(|| "none".to_string());
        *type_counts.entry(type_key).or_insert(0) += 1;

        let yes_id = yes_token_id.or(yes_pos).map(|v| v.as_str()).unwrap_or("0");
        let no_id = no_token_id.or(no_pos).map(|v| v.as_str()).unwrap_or("0");
        if yes_id == "0" && no_id == "0" {
            missing_tokens += 1;
        }

        total_entries += 1;
    };

    for parent in parents {
        if let Some(topic_id) = parent.topic_id {
            if seen.insert(topic_id) {
                record_entry(
                    parent.status_enum.as_ref(),
                    parent.status,
                    parent.market_type,
                    parent.yes_token_id.as_ref(),
                    parent.no_token_id.as_ref(),
                    parent.yes_pos.as_ref(),
                    parent.no_pos.as_ref(),
                );
            }
        }

        if let Some(children) = &parent.child_list {
            for child in children {
                let topic_id = match child.topic_id {
                    Some(value) => value,
                    None => continue,
                };
                if !seen.insert(topic_id) {
                    continue;
                }
                let market_type = child.market_type.or(parent.market_type);
                record_entry(
                    child.status_enum.as_ref(),
                    child.status,
                    market_type,
                    child.yes_token_id.as_ref(),
                    child.no_token_id.as_ref(),
                    child.yes_pos.as_ref(),
                    child.no_pos.as_ref(),
                );
            }
        }
    }

    (status_counts, type_counts, missing_tokens, total_entries)
}

fn summarize_markets(
    markets: &[opinion_hft::data::rest::ApiMarket],
) -> (HashMap<String, usize>, HashMap<String, usize>) {
    let mut status_counts: HashMap<String, usize> = HashMap::new();
    let mut type_counts: HashMap<String, usize> = HashMap::new();

    for market in markets {
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
    }

    (status_counts, type_counts)
}

fn log_master_market_child(parent: &MasterMarketParent, child: &MasterMarketChild) {
    let title = child
        .market_title
        .as_ref()
        .or(parent.market_title.as_ref())
        .map(|value| truncate(value, 120))
        .unwrap_or_else(|| "Unknown".to_string());
    let market_type = child.market_type.or(parent.market_type);
    let parent_id = parent
        .root_market_id
        .clone()
        .or(parent.question_id.clone())
        .or(child.question_id.clone());

    info!(
        "master_market child topic_id={:?} parent_id={:?} status={:?} status_enum={:?} market_type={:?} yes_buy_price={:?} no_buy_price={:?} yes_token_id={:?} no_token_id={:?} yes_pos={:?} no_pos={:?} volume24h={:?} volume={:?} title={}",
        child.topic_id,
        parent_id,
        child.status,
        child.status_enum,
        market_type,
        child.yes_buy_price,
        child.no_buy_price,
        child.yes_token_id,
        child.no_token_id,
        child.yes_pos,
        child.no_pos,
        child.volume24h,
        child.volume,
        title
    );
}

fn log_master_market_parent(parent: &MasterMarketParent) {
    let title = parent
        .market_title
        .as_ref()
        .map(|value| truncate(value, 120))
        .unwrap_or_else(|| "Unknown".to_string());
    let parent_id = parent.root_market_id.clone().or(parent.question_id.clone());

    info!(
        "master_market parent topic_id={:?} parent_id={:?} status={:?} status_enum={:?} market_type={:?} yes_buy_price={:?} no_buy_price={:?} yes_token_id={:?} no_token_id={:?} yes_pos={:?} no_pos={:?} volume24h={:?} volume={:?} title={}",
        parent.topic_id,
        parent_id,
        parent.status,
        parent.status_enum,
        parent.market_type,
        parent.yes_buy_price,
        parent.no_buy_price,
        parent.yes_token_id,
        parent.no_token_id,
        parent.yes_pos,
        parent.no_pos,
        parent.volume24h,
        parent.volume,
        title
    );
}

fn log_market(label: &str, market: &opinion_hft::data::rest::ApiMarket) {
    let title = truncate(&market.market_title, 120);
    info!(
        "market_sweep label={} market_id={} status={} status_enum={} market_type={:?} chain_id={:?} yes_token_id={:?} no_token_id={:?} result_token_id={:?} volume24h={:?} title={}",
        label,
        market.market_id,
        market.status,
        market.status_enum,
        market.market_type,
        market.chain_id,
        market.yes_token_id,
        market.no_token_id,
        market.result_token_id,
        market.volume24h,
        title
    );
}

fn truncate(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }

    let mut truncated = value.chars().take(max_len).collect::<String>();
    truncated.push_str("...");
    truncated
}
