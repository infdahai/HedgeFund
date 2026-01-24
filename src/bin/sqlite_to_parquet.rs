#[cfg(not(feature = "polars"))]
fn main() -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "sqlite_to_parquet requires --features polars"
    ))
}

#[cfg(feature = "polars")]
use chrono::Timelike;
#[cfg(feature = "polars")]
use std::collections::HashMap;
#[cfg(feature = "polars")]
use std::path::PathBuf;

#[cfg(feature = "polars")]
fn main() -> anyhow::Result<()> {
    use std::fs::{self, File};

    use anyhow::{anyhow, Context};
    use glob::glob;
    use polars::io::parquet::write::ParquetWriter;
    use polars::prelude::*;

    let (input_dir, output_dir) = parse_args()?;
    let mut partitions: HashMap<String, Vec<SnapshotRow>> = HashMap::new();

    let pattern = format!("{}/*.sqlite", input_dir.display());
    let mut found = false;
    for entry in glob(&pattern).context("invalid sqlite glob pattern")? {
        let path = entry.context("invalid sqlite path")?;
        found = true;
        load_sqlite(&path, &mut partitions)?;
    }

    if !found {
        return Err(anyhow!(
            "no sqlite files found under {}",
            input_dir.display()
        ));
    }

    let output_root = output_dir.join("orderbook_snapshots_v1");
    for (partition, rows) in partitions {
        if rows.is_empty() {
            continue;
        }
        let dir = output_root.join(partition);
        fs::create_dir_all(&dir).with_context(|| format!("create dir {}", dir.display()))?;
        let part = next_part(&dir)?;
        let file_path = dir.join(format!("part-{part:06}.parquet"));
        let mut df = build_dataframe(&rows)?;
        let mut file = File::create(&file_path)?;
        ParquetWriter::new(&mut file)
            .with_compression(ParquetCompression::Zstd(None))
            .finish(&mut df)?;
        let success_path = dir.join("_SUCCESS");
        let _ = File::create(success_path);
    }

    Ok(())
}

#[cfg(feature = "polars")]
#[derive(Debug, Clone)]
struct Level {
    p: f64,
    s: f64,
}

#[cfg(feature = "polars")]
#[derive(Debug, Clone)]
struct SnapshotRow {
    snapshot_id: i64,
    market_id: String,
    pair: String,
    outcome: String,
    source: String,
    exchange_ts_us: i64,
    local_recv_ts_us: i64,
    local_write_ts_us: i64,
    best_bid: f64,
    best_ask: f64,
    spread: f64,
    bids: Vec<Level>,
    asks: Vec<Level>,
}

#[cfg(feature = "polars")]
fn parse_args() -> anyhow::Result<(PathBuf, PathBuf)> {
    use std::env;

    use anyhow::{anyhow, Context};

    // Default to relative paths that work across machines
    let mut input_dir = PathBuf::from("data");
    let mut output_dir = PathBuf::from("data/parquet");

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input-dir" => {
                let value = args.next().context("--input-dir requires a value")?;
                input_dir = PathBuf::from(value);
            }
            "--output-dir" => {
                let value = args.next().context("--output-dir requires a value")?;
                output_dir = PathBuf::from(value);
            }
            "--help" => {
                println!(
                    "sqlite_to_parquet --input-dir <dir> --output-dir <dir>\n\nDefaults:\n  --input-dir data\n  --output-dir data/parquet"
                );
                std::process::exit(0);
            }
            _ => return Err(anyhow!("unknown argument: {arg}")),
        }
    }

    Ok((input_dir, output_dir))
}

#[cfg(feature = "polars")]
fn load_sqlite(
    path: &std::path::Path,
    partitions: &mut HashMap<String, Vec<SnapshotRow>>,
) -> anyhow::Result<()> {
    use anyhow::Context;
    use rusqlite::Connection;

    let conn = Connection::open(path).with_context(|| format!("open sqlite {}", path.display()))?;
    let mut levels_map: HashMap<i64, (Vec<Level>, Vec<Level>)> = HashMap::new();

    let mut stmt = conn.prepare(
        "SELECT snapshot_id, side, price, size, level FROM orderbook_levels ORDER BY snapshot_id, level",
    )?;
    let rows = stmt.query_map([], parse_level_row)?;
    for row in rows {
        let (snapshot_id, side, level) = row?;
        let entry = levels_map.entry(snapshot_id).or_default();
        if side == "bid" {
            entry.0.push(level);
        } else if side == "ask" {
            entry.1.push(level);
        }
    }

    let mut stmt = conn.prepare(
        "SELECT id, market_id, pair, snapshot_ts, captured_at_ms FROM orderbook_snapshots ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        let snapshot_id: i64 = row.get(0)?;
        let market_id: String = row.get(1)?;
        let pair: String = row.get(2)?;
        let snapshot_ts: i64 = row.get(3)?;
        let captured_at_ms: i64 = row.get(4)?;
        Ok((snapshot_id, market_id, pair, snapshot_ts, captured_at_ms))
    })?;

    for row in rows {
        let (snapshot_id, market_id, pair, snapshot_ts, captured_at_ms) = row?;
        let Some((bids, asks)) = levels_map.remove(&snapshot_id) else {
            continue;
        };
        if bids.is_empty() || asks.is_empty() {
            continue;
        }
        let best_bid = bids.first().map(|level| level.p).unwrap_or(0.0);
        let best_ask = asks.first().map(|level| level.p).unwrap_or(0.0);
        if best_bid <= 0.0 || best_ask <= 0.0 {
            continue;
        }
        let exchange_ts_us = snapshot_ts.saturating_mul(1000);
        let local_recv_ts_us = captured_at_ms.saturating_mul(1000);
        let local_write_ts_us = local_recv_ts_us;
        let outcome = derive_outcome(&pair);
        let spread = (best_ask - best_bid).max(0.0);
        let row = SnapshotRow {
            snapshot_id,
            market_id,
            pair,
            outcome,
            source: "sqlite".to_string(),
            exchange_ts_us,
            local_recv_ts_us,
            local_write_ts_us,
            best_bid,
            best_ask,
            spread,
            bids,
            asks,
        };

        let partition = resolve_partition_dir(local_recv_ts_us);
        partitions.entry(partition).or_default().push(row);
    }

    Ok(())
}

#[cfg(feature = "polars")]
fn parse_level_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, String, Level)> {
    let snapshot_id: i64 = row.get(0)?;
    let side: String = row.get(1)?;
    let price: String = row.get(2)?;
    let size: String = row.get(3)?;
    let price = price.parse::<f64>().unwrap_or(0.0);
    let size = size.parse::<f64>().unwrap_or(0.0);
    Ok((snapshot_id, side, Level { p: price, s: size }))
}

#[cfg(feature = "polars")]
fn derive_outcome(pair: &str) -> String {
    if let Some((_, suffix)) = pair.rsplit_once('-') {
        suffix.trim().to_uppercase()
    } else {
        "UNKNOWN".to_string()
    }
}

#[cfg(feature = "polars")]
fn resolve_partition_dir(ts_us: i64) -> String {
    let timestamp = chrono::DateTime::<chrono::Utc>::from_timestamp_micros(ts_us)
        .unwrap_or_else(chrono::Utc::now);
    format!(
        "date={}/hour={:02}",
        timestamp.format("%Y-%m-%d"),
        timestamp.hour()
    )
}

#[cfg(feature = "polars")]
fn next_part(dir: &std::path::Path) -> anyhow::Result<u64> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(0),
    };
    let mut max_part: Option<u64> = None;
    for entry in entries.flatten() {
        let name = match entry.file_name().to_str() {
            Some(value) => value.to_string(),
            None => continue,
        };
        if let Some(part) = parse_part_number(&name) {
            max_part = Some(max_part.map_or(part, |current| current.max(part)));
        }
    }
    Ok(max_part.map_or(0, |value| value.saturating_add(1)))
}

#[cfg(feature = "polars")]
fn parse_part_number(file_name: &str) -> Option<u64> {
    let suffix = file_name.strip_prefix("part-")?;
    let suffix = suffix.strip_suffix(".parquet")?;
    suffix.parse::<u64>().ok()
}

#[cfg(feature = "polars")]
fn build_dataframe(
    rows: &[SnapshotRow],
) -> polars::prelude::PolarsResult<polars::prelude::DataFrame> {
    use polars::prelude::*;

    let snapshot_ids: Vec<i64> = rows.iter().map(|row| row.snapshot_id).collect();
    let market_ids: Vec<String> = rows.iter().map(|row| row.market_id.clone()).collect();
    let pairs: Vec<String> = rows.iter().map(|row| row.pair.clone()).collect();
    let outcomes: Vec<String> = rows.iter().map(|row| row.outcome.clone()).collect();
    let sources: Vec<String> = rows.iter().map(|row| row.source.clone()).collect();
    let exchange_ts: Vec<i64> = rows.iter().map(|row| row.exchange_ts_us).collect();
    let local_recv_ts: Vec<i64> = rows.iter().map(|row| row.local_recv_ts_us).collect();
    let local_write_ts: Vec<i64> = rows.iter().map(|row| row.local_write_ts_us).collect();
    let best_bids: Vec<f64> = rows.iter().map(|row| row.best_bid).collect();
    let best_asks: Vec<f64> = rows.iter().map(|row| row.best_ask).collect();
    let spreads: Vec<f64> = rows.iter().map(|row| row.spread).collect();

    let bids = build_level_list_series("bids", rows, |row| &row.bids)?;
    let asks = build_level_list_series("asks", rows, |row| &row.asks)?;

    DataFrame::new(vec![
        Series::new("snapshot_id".into(), snapshot_ids).into(),
        Series::new("market_id".into(), market_ids).into(),
        Series::new("pair".into(), pairs).into(),
        Series::new("outcome".into(), outcomes).into(),
        Series::new("source".into(), sources).into(),
        Series::new("exchange_ts_us".into(), exchange_ts).into(),
        Series::new("local_recv_ts_us".into(), local_recv_ts).into(),
        Series::new("local_write_ts_us".into(), local_write_ts).into(),
        Series::new("best_bid".into(), best_bids).into(),
        Series::new("best_ask".into(), best_asks).into(),
        Series::new("spread".into(), spreads).into(),
        bids.into(),
        asks.into(),
    ])
}

#[cfg(feature = "polars")]
fn build_level_list_series<F>(
    name: &str,
    rows: &[SnapshotRow],
    extractor: F,
) -> polars::prelude::PolarsResult<polars::prelude::Series>
where
    F: Fn(&SnapshotRow) -> &Vec<Level>,
{
    use polars::prelude::*;

    let mut structs: Vec<Series> = Vec::with_capacity(rows.len());
    for row in rows {
        structs.push(build_level_struct_series(extractor(row))?);
    }

    let list_chunked: ListChunked = structs.into_iter().collect();
    Ok(list_chunked.into_series().with_name(PlSmallStr::from(name)))
}

#[cfg(feature = "polars")]
fn build_level_struct_series(
    levels: &[Level],
) -> polars::prelude::PolarsResult<polars::prelude::Series> {
    use polars::prelude::*;

    let prices: Vec<f64> = levels.iter().map(|level| level.p).collect();
    let sizes: Vec<f64> = levels.iter().map(|level| level.s).collect();

    let price_series = Series::new("p".into(), prices);
    let size_series = Series::new("s".into(), sizes);
    let fields = [price_series, size_series];

    StructChunked::from_series(PlSmallStr::from("item"), levels.len(), fields.iter())
        .map(Into::into)
}
