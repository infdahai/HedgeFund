use std::env;
use std::str::FromStr;
use std::time::{Duration, Instant};

use opinion_hft::config::{Environment, WebSocketConfig, WEBSOCKET_URL};
use opinion_hft::data::websocket::{WebSocketClient, WsEvent};
use opinion_hft::error::{Error, Result};
use opinion_hft::telemetry::init_telemetry;
use tokio::time::timeout;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let _guards = init_telemetry();

    let api_key = env::var("OPINION_API_KEY").map_err(|_| Error::MissingConfig {
        field: "OPINION_API_KEY".into(),
    })?;

    let ws_url = env::var("OPINION_WS_URL").unwrap_or_else(|_| WEBSOCKET_URL.to_string());
    let environment = env::var("OPINION_ENV")
        .ok()
        .and_then(|value| Environment::from_str(&value).ok())
        .unwrap_or_default();
    let probe_secs = env::var("OPINION_WS_PROBE_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(20);

    let raw_payloads = parse_raw_payloads();
    let channels = if raw_payloads.is_empty() {
        Some(parse_channels()?)
    } else {
        None
    };

    let config = WebSocketConfig {
        url: ws_url,
        ..WebSocketConfig::default()
    };

    let mut ws_client = WebSocketClient::new(&config, api_key, environment);
    let mut event_rx = ws_client.subscribe_events();

    ws_client.connect().await?;
    if let Some(channels) = channels {
        info!("WS probe using subscription channels");
        for channel in channels {
            ws_client.subscribe(&channel).await?;
        }
    } else {
        info!("WS probe using raw payload(s)");
        for payload in &raw_payloads {
            ws_client.send_raw(payload).await?;
        }
    }

    info!("WS probe running for {}s...", probe_secs);
    let deadline = Instant::now() + Duration::from_secs(probe_secs);

    while Instant::now() < deadline {
        match timeout(Duration::from_secs(2), event_rx.recv()).await {
            Ok(Ok(event)) => log_event(event),
            _ => continue,
        }
    }

    ws_client.close().await.ok();
    info!("WS probe complete");
    Ok(())
}

fn parse_channels() -> Result<Vec<String>> {
    if let Ok(raw) = env::var("OPINION_WS_CHANNELS") {
        let channels: Vec<String> = raw
            .split(',')
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        if !channels.is_empty() {
            return Ok(channels);
        }
    }

    if let Ok(raw) = env::var("OPINION_WS_CHANNEL") {
        if !raw.trim().is_empty() {
            return Ok(vec![raw.trim().to_string()]);
        }
    }

    Err(Error::MissingConfig {
        field: "OPINION_WS_CHANNEL(S)".into(),
    })
}

fn log_event(event: WsEvent) {
    match event {
        WsEvent::Connected => info!("WS connected"),
        WsEvent::Disconnected { reason } => info!("WS disconnected: {}", reason),
        WsEvent::MarketData { pair, data } => info!("MarketData pair={} data={}", pair, data),
        WsEvent::MarketTrade { pair, data } => info!("MarketTrade pair={} data={}", pair, data),
        WsEvent::OrderUpdate(update) => info!(
            "OrderUpdate id={} status={}",
            update.order_id, update.status
        ),
        WsEvent::TradeRecord(record) => info!("TradeRecord order_id={}", record.order_id),
        WsEvent::MarketDepthDiff {
            market_id,
            outcome,
            outcome_side,
            side,
            price,
            size,
        } => {
            let outcome_label = outcome.unwrap_or_else(|| "unknown".to_string());
            let outcome_side_label = outcome_side
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string());
            info!(
                "MarketDepthDiff market_id={} outcome={} outcome_side={} side={} price={} size={}",
                market_id, outcome_label, outcome_side_label, side, price, size
            );
        }
    }
}

fn parse_raw_payloads() -> Vec<String> {
    let args: Vec<String> = env::args().skip(1).collect();
    args.into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}
