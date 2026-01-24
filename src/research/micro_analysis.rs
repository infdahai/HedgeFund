use anyhow::Result;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;

use super::config::CliConfig;
use super::types::MarketTick;
use crate::strategy::core::microstructure::MicrostructureFeature;
use crate::strategy::core::{MarketOrderbook, OrderbookSide};

pub fn analyze_market_microstructure(
    markets: &HashMap<String, Vec<MarketTick>>,
    _config: &CliConfig,
) -> Result<()> {
    let mut all_features = Vec::new();

    for (market_id, ticks) in markets {
        if ticks.is_empty() {
            continue;
        }

        let features = extract_features(market_id, ticks);
        all_features.extend(features);
    }

    if all_features.is_empty() {
        tracing::warn!("No features extracted");
        return Ok(());
    }

    // Convert to DataFrame and write to Parquet
    let mut df = features_to_dataframe(&all_features)?;

    // Determine output path: use --output or default to "features.parquet"
    // Since config.output is usually for CSV reports, we might want a specific flag or just hardcode for now
    // logic in research.rs usually handles output paths.
    // If we are in 'microstructure' mode, research.rs might pass specific config.
    // For now let's use a fixed name in the current directory or derived from config.
    let output_path = "features.parquet";

    let file = File::create(output_path)?;
    ParquetWriter::new(file).finish(&mut df)?;

    tracing::info!("Wrote {} features to {}", all_features.len(), output_path);
    Ok(())
}

fn extract_features(market_id: &str, ticks: &[MarketTick]) -> Vec<MicrostructureFeature> {
    let mut result = Vec::new();
    result.extend(extract_outcome_features(market_id, "YES", ticks, |t| {
        &t.yes
    }));
    result.extend(extract_outcome_features(market_id, "NO", ticks, |t| &t.no));

    // Sort by timestamp to ensure mixed YES/NO stream is ordered (though not strictly necessary for analysis if we group by pair)
    result.sort_by_key(|f| f.timestamp_ms);

    result
}

fn extract_outcome_features<F>(
    market_id: &str,
    outcome: &str,
    ticks: &[MarketTick],
    extractor: F,
) -> Vec<MicrostructureFeature>
where
    F: Fn(&MarketTick) -> &super::types::TickBook,
{
    let mut features = Vec::with_capacity(ticks.len());
    let mut prev: Option<MicrostructureFeature> = None;
    let pair = format!("{}-{}", market_id, outcome);

    for (i, tick) in ticks.iter().enumerate() {
        let book = extractor(tick);

        // Note: MarketTick only has top of book (best_bid/ask).
        // So L5 metrics will be identical to L1 in this implementation.
        let ob = MarketOrderbook {
            pair: pair.clone(),
            bids: vec![OrderbookSide {
                price: book.best_bid,
                size: book.bid_size,
            }],
            asks: vec![OrderbookSide {
                price: book.best_ask,
                size: book.ask_size,
            }],
        };

        if let Some(mut f) = MicrostructureFeature::from_orderbook(
            market_id,
            &pair,
            &ob,
            prev.as_ref(),
            tick.ts_us as u64 / 1000,
        ) {
            // Look ahead for labels (future price in 10s)
            let target_ts = tick.ts_us + 10_000_000;

            // Optimization: since ticks are sorted, we can search forward
            // For large datasets, a window iterator is better, but this is simple for MVP.
            // Using binary search might be faster but `ticks` is already indexed.
            // Linear scan bounded by 10s window is O(window_size), which is small.

            // Find first tick >= target_ts
            let future_idx = ticks[i..].iter().position(|t| t.ts_us >= target_ts);

            if let Some(offset) = future_idx {
                let future_tick = &ticks[i + offset];
                let future_book = extractor(future_tick);
                let future_mid = (future_book.best_bid + future_book.best_ask) / 2.0;
                if future_mid > 0.0 && f.mid_price > 0.0 {
                    f.next_price_10s = Some(future_mid);
                    f.price_change_10s = Some((future_mid - f.mid_price) / f.mid_price);
                }
            }

            // 30s lookahead
            let target_ts_30 = tick.ts_us + 30_000_000;
            let future_idx_30 = ticks[i..].iter().position(|t| t.ts_us >= target_ts_30);
            if let Some(offset) = future_idx_30 {
                let future_tick = &ticks[i + offset];
                let future_book = extractor(future_tick);
                let future_mid = (future_book.best_bid + future_book.best_ask) / 2.0;
                if future_mid > 0.0 {
                    f.next_price_30s = Some(future_mid);
                }
            }

            features.push(f.clone());
            prev = Some(f);
        }
    }
    features
}

fn features_to_dataframe(features: &[MicrostructureFeature]) -> Result<DataFrame> {
    let market_ids: Series = Series::new(
        "market_id".into(),
        features
            .iter()
            .map(|f| f.market_id.as_str())
            .collect::<Vec<_>>(),
    );
    let pairs: Series = Series::new(
        "pair".into(),
        features.iter().map(|f| f.pair.as_str()).collect::<Vec<_>>(),
    );
    let timestamps: Series = Series::new(
        "timestamp_ms".into(),
        features.iter().map(|f| f.timestamp_ms).collect::<Vec<_>>(),
    );
    let mid_prices: Series = Series::new(
        "mid_price".into(),
        features.iter().map(|f| f.mid_price).collect::<Vec<_>>(),
    );
    let best_bids: Series = Series::new(
        "best_bid".into(),
        features.iter().map(|f| f.best_bid).collect::<Vec<_>>(),
    );
    let best_asks: Series = Series::new(
        "best_ask".into(),
        features.iter().map(|f| f.best_ask).collect::<Vec<_>>(),
    );

    let obi_l1: Series = Series::new(
        "obi_l1".into(),
        features.iter().map(|f| f.obi_l1).collect::<Vec<_>>(),
    );
    let ofi: Series = Series::new(
        "ofi".into(),
        features.iter().map(|f| f.ofi).collect::<Vec<_>>(),
    );
    let ofi_ema: Series = Series::new(
        "ofi_ema".into(),
        features.iter().map(|f| f.ofi_ema).collect::<Vec<_>>(),
    );

    let spread_bps: Series = Series::new(
        "spread_bps".into(),
        features.iter().map(|f| f.spread_bps).collect::<Vec<_>>(),
    );
    let spread_abs: Series = Series::new(
        "spread_abs".into(),
        features.iter().map(|f| f.spread_abs).collect::<Vec<_>>(),
    );

    let bid_depth: Series = Series::new(
        "bid_depth_l1".into(),
        features.iter().map(|f| f.bid_depth_l1).collect::<Vec<_>>(),
    );
    let ask_depth: Series = Series::new(
        "ask_depth_l1".into(),
        features.iter().map(|f| f.ask_depth_l1).collect::<Vec<_>>(),
    );

    let next_prices_10s: Series = Series::new(
        "next_price_10s".into(),
        features
            .iter()
            .map(|f| f.next_price_10s)
            .collect::<Vec<_>>(),
    );
    let price_changes_10s: Series = Series::new(
        "price_change_10s".into(),
        features
            .iter()
            .map(|f| f.price_change_10s)
            .collect::<Vec<_>>(),
    );
    let next_prices_30s: Series = Series::new(
        "next_price_30s".into(),
        features
            .iter()
            .map(|f| f.next_price_30s)
            .collect::<Vec<_>>(),
    );

    DataFrame::new(vec![
        market_ids.into(),
        pairs.into(),
        timestamps.into(),
        mid_prices.into(),
        best_bids.into(),
        best_asks.into(),
        obi_l1.into(),
        ofi.into(),
        ofi_ema.into(),
        spread_bps.into(),
        spread_abs.into(),
        bid_depth.into(),
        ask_depth.into(),
        next_prices_10s.into(),
        price_changes_10s.into(),
        next_prices_30s.into(),
    ])
    .map_err(Into::into)
}
