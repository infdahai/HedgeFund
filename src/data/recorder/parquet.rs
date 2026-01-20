use chrono::{DateTime, Timelike, Utc};
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::ParquetConfig;
use crate::data::rest::models::TokenOrderbook;

const SNAPSHOT_ID_SHIFT: u32 = 12;
const SNAPSHOT_ID_MASK: u64 = (1 << SNAPSHOT_ID_SHIFT) - 1;

#[derive(Debug, Clone)]
pub struct Level {
    pub p: f64,
    pub s: f64,
}

#[derive(Debug, Clone)]
pub struct SnapshotRow {
    pub snapshot_id: u64,
    pub market_id: String,
    pub pair: String,
    pub outcome: String,
    pub source: String,
    pub exchange_ts_us: Option<i64>,
    pub local_recv_ts_us: i64,
    pub local_write_ts_us: i64,
    pub best_bid: f64,
    pub best_ask: f64,
    pub spread: f64,
    pub bids: Vec<Level>,
    pub asks: Vec<Level>,
}

#[derive(Debug)]
struct RawSnapshotRow {
    snapshot_id: u64,
    market_id: String,
    pair: String,
    outcome: String,
    source: String,
    exchange_ts_us: Option<i64>,
    local_recv_ts_us: i64,
    bids: Vec<(String, String)>,
    asks: Vec<(String, String)>,
}

enum RecorderMsg {
    Data(RawSnapshotRow),
    Flush,
    Shutdown,
}

#[derive(Clone)]
pub struct ParquetRecorderHandle {
    sender: mpsc::SyncSender<RecorderMsg>,
    dropped_count: Arc<AtomicU64>,
    snapshot_seq: Arc<AtomicU64>,
}

impl ParquetRecorderHandle {
    pub fn record_orderbook(
        &self,
        market_id: &str,
        pair: &str,
        outcome: &str,
        orderbook: &TokenOrderbook,
    ) {
        let local_recv_ts_us = now_ts_us();
        let seq = self.snapshot_seq.fetch_add(1, Ordering::Relaxed);
        let snapshot_id = make_snapshot_id(local_recv_ts_us, seq);
        let exchange_ts_us = Some(orderbook.timestamp.saturating_mul(1_000));

        let row = RawSnapshotRow {
            snapshot_id,
            market_id: market_id.to_string(),
            pair: pair.to_string(),
            outcome: outcome.to_string(),
            source: "rest".to_string(),
            exchange_ts_us,
            local_recv_ts_us,
            bids: orderbook
                .bids
                .iter()
                .map(|level| (level.price.clone(), level.size.clone()))
                .collect(),
            asks: orderbook
                .asks
                .iter()
                .map(|level| (level.price.clone(), level.size.clone()))
                .collect(),
        };

        match self.sender.try_send(RecorderMsg::Data(row)) {
            Ok(_) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                self.dropped_count.fetch_add(1, Ordering::Relaxed);
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {}
        }
    }

    pub fn flush(&self) {
        let _ = self.sender.try_send(RecorderMsg::Flush);
    }

    pub fn shutdown(&self) {
        let _ = self.sender.try_send(RecorderMsg::Shutdown);
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped_count.load(Ordering::Relaxed)
    }
}

pub fn spawn_parquet_recorder(
    config: ParquetConfig,
) -> Option<(ParquetRecorderHandle, thread::JoinHandle<()>)> {
    if !config.enabled {
        return None;
    }

    let capacity = config.queue_capacity.max(1);
    let (sender, receiver) = mpsc::sync_channel(capacity);
    let dropped_count = Arc::new(AtomicU64::new(0));
    let snapshot_seq = Arc::new(AtomicU64::new(0));

    let handle = ParquetRecorderHandle {
        sender,
        dropped_count: Arc::clone(&dropped_count),
        snapshot_seq: Arc::clone(&snapshot_seq),
    };

    let base_dir = config.dir.clone();
    let batch_size = config.batch_size.max(1);
    let flush_interval = Duration::from_millis(config.flush_interval_ms.max(1));
    let heartbeat_interval = Duration::from_secs(config.heartbeat_interval_secs.max(1));
    let compression = parse_compression(&config.compression);

    tracing::info!(
        "Parquet recorder started dir={} batch_size={} flush_ms={} queue_cap={} compression={} heartbeat_secs={}",
        base_dir,
        batch_size,
        flush_interval.as_millis(),
        capacity,
        config.compression,
        heartbeat_interval.as_secs()
    );

    let join = match thread::Builder::new()
        .name("parquet-recorder".to_string())
        .spawn(move || {
            let mut recorder = ParquetRecorder::new(base_dir, batch_size, compression);
            let mut last_flush = Instant::now();
            let mut last_heartbeat = Instant::now();
            let mut last_dropped = 0u64;

            loop {
                let flush_due = flush_interval.saturating_sub(last_flush.elapsed());
                let heartbeat_due = heartbeat_interval.saturating_sub(last_heartbeat.elapsed());
                let wait_for = flush_due.min(heartbeat_due);

                match receiver.recv_timeout(wait_for) {
                    Ok(RecorderMsg::Data(row)) => {
                        recorder.buffer.push(convert_row(row));
                        if recorder.buffer.len() >= recorder.batch_size {
                            if let Err(err) = recorder.flush() {
                                tracing::warn!("Parquet flush failed: {}", err);
                            }
                            last_flush = Instant::now();
                        }
                    }
                    Ok(RecorderMsg::Flush) => {
                        if let Err(err) = recorder.flush() {
                            tracing::warn!("Parquet flush failed: {}", err);
                        }
                        last_flush = Instant::now();
                    }
                    Ok(RecorderMsg::Shutdown) => {
                        let _ = recorder.flush();
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        let _ = recorder.flush();
                        break;
                    }
                }

                if last_flush.elapsed() >= flush_interval {
                    if let Err(err) = recorder.flush() {
                        tracing::warn!("Parquet flush failed: {}", err);
                    }
                    last_flush = Instant::now();
                }

                if last_heartbeat.elapsed() >= heartbeat_interval {
                    let total = dropped_count.load(Ordering::Relaxed);
                    let delta = total.saturating_sub(last_dropped);
                    if delta > 0 {
                        tracing::warn!("Parquet recorder dropped {} snapshots", delta);
                        last_dropped = total;
                    }
                    last_heartbeat = Instant::now();
                }
            }
        }) {
        Ok(join) => join,
        Err(err) => {
            tracing::error!("Failed to spawn parquet recorder: {}", err);
            return None;
        }
    };

    Some((handle, join))
}

struct ParquetRecorder {
    buffer: Vec<SnapshotRow>,
    batch_size: usize,
    base_dir: String,
    part_counters: HashMap<String, u64>,
    compression: ParquetCompression,
}

impl ParquetRecorder {
    pub fn new(
        base_dir: impl Into<String>,
        batch_size: usize,
        compression: ParquetCompression,
    ) -> Self {
        Self {
            buffer: Vec::with_capacity(batch_size),
            batch_size,
            base_dir: base_dir.into(),
            part_counters: HashMap::new(),
            compression,
        }
    }

    pub fn flush(&mut self) -> PolarsResult<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        let base_ts = self
            .buffer
            .first()
            .map(|row| row.local_recv_ts_us)
            .unwrap_or_else(now_ts_us);
        let dir_path = resolve_partition_dir(&self.base_dir, base_ts);
        std::fs::create_dir_all(&dir_path)?;
        let part = self.next_part(&dir_path);

        let mut df = build_dataframe(&self.buffer)?;
        let filename = format!("{}/part-{:06}.parquet", dir_path, part);
        let file = File::create(&filename)?;

        ParquetWriter::new(file)
            .with_compression(self.compression)
            .finish(&mut df)?;

        self.buffer.clear();
        Ok(())
    }

    fn next_part(&mut self, dir_path: &str) -> u64 {
        let entry = self.part_counters.entry(dir_path.to_string()).or_insert(0);
        let current = *entry;
        *entry = entry.saturating_add(1);
        current
    }
}

fn build_dataframe(rows: &[SnapshotRow]) -> PolarsResult<DataFrame> {
    let snapshot_ids: Vec<u64> = rows.iter().map(|row| row.snapshot_id).collect();
    let market_ids: Vec<String> = rows.iter().map(|row| row.market_id.clone()).collect();
    let pairs: Vec<String> = rows.iter().map(|row| row.pair.clone()).collect();
    let outcomes: Vec<String> = rows.iter().map(|row| row.outcome.clone()).collect();
    let sources: Vec<String> = rows.iter().map(|row| row.source.clone()).collect();
    let exchange_ts: Vec<Option<i64>> = rows.iter().map(|row| row.exchange_ts_us).collect();
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

fn build_level_list_series<F>(name: &str, rows: &[SnapshotRow], extractor: F) -> PolarsResult<Series>
where
    F: Fn(&SnapshotRow) -> &Vec<Level>,
{
    let mut structs: Vec<Series> = Vec::with_capacity(rows.len());
    for row in rows {
        structs.push(build_level_struct_series(extractor(row))?);
    }

    let list_chunked: ListChunked = structs.into_iter().collect();
    Ok(list_chunked.into_series().with_name(PlSmallStr::from(name)))
}

fn build_level_struct_series(levels: &[Level]) -> PolarsResult<Series> {
    let prices: Vec<f64> = levels.iter().map(|level| level.p).collect();
    let sizes: Vec<f64> = levels.iter().map(|level| level.s).collect();

    let price_series = Series::new("p".into(), prices);
    let size_series = Series::new("s".into(), sizes);
    let fields = vec![price_series, size_series];

    StructChunked::from_series(PlSmallStr::from("item"), levels.len(), fields.iter())
        .map(Into::into)
}

fn convert_row(raw: RawSnapshotRow) -> SnapshotRow {
    let local_write_ts_us = now_ts_us();
    let bids = parse_levels(&raw.bids);
    let asks = parse_levels(&raw.asks);
    let best_bid = bids.first().map(|level| level.p).unwrap_or(0.0);
    let best_ask = asks.first().map(|level| level.p).unwrap_or(0.0);
    let spread = if best_bid > 0.0 && best_ask > 0.0 {
        best_ask - best_bid
    } else {
        0.0
    };

    SnapshotRow {
        snapshot_id: raw.snapshot_id,
        market_id: raw.market_id,
        pair: raw.pair,
        outcome: raw.outcome,
        source: raw.source,
        exchange_ts_us: raw.exchange_ts_us,
        local_recv_ts_us: raw.local_recv_ts_us,
        local_write_ts_us,
        best_bid,
        best_ask,
        spread,
        bids,
        asks,
    }
}

fn parse_levels(levels: &[(String, String)]) -> Vec<Level> {
    let mut parsed = Vec::with_capacity(levels.len());
    for (price, size) in levels {
        let price = price.parse::<f64>();
        let size = size.parse::<f64>();
        if let (Ok(p), Ok(s)) = (price, size) {
            parsed.push(Level { p, s });
        }
    }
    parsed
}

fn resolve_partition_dir(base_dir: &str, ts_us: i64) -> String {
    let timestamp = DateTime::<Utc>::from_timestamp_micros(ts_us).unwrap_or_else(Utc::now);
    format!(
        "{}/date={}/hour={:02}",
        base_dir,
        timestamp.format("%Y-%m-%d"),
        timestamp.hour()
    )
}

fn parse_compression(raw: &str) -> ParquetCompression {
    match raw.trim().to_lowercase().as_str() {
        "snappy" => ParquetCompression::Snappy,
        "none" | "uncompressed" => ParquetCompression::Uncompressed,
        _ => ParquetCompression::Zstd(None),
    }
}

fn make_snapshot_id(local_recv_ts_us: i64, seq: u64) -> u64 {
    let ts = if local_recv_ts_us <= 0 {
        0
    } else {
        local_recv_ts_us as u64
    };
    (ts << SNAPSHOT_ID_SHIFT) | (seq & SNAPSHOT_ID_MASK)
}

fn now_ts_us() -> i64 {
    Utc::now().timestamp_micros()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_parquet_round_trip() -> PolarsResult<()> {
        let base_ts_us = 1_700_000_000_000_000i64;
        let base_dir = std::env::temp_dir().join("orderbook_snapshot_roundtrip");
        let base_dir_str = base_dir.to_string_lossy().to_string();
        let mut recorder = ParquetRecorder::new(base_dir_str.clone(), 2, ParquetCompression::Zstd(None));

        let row1 = SnapshotRow {
            snapshot_id: 1,
            market_id: "m1".to_string(),
            pair: "YES/NO".to_string(),
            outcome: "YES".to_string(),
            source: "rest".to_string(),
            exchange_ts_us: Some(1_000),
            local_recv_ts_us: base_ts_us,
            local_write_ts_us: base_ts_us + 100,
            best_bid: 0.5,
            best_ask: 0.6,
            spread: 0.1,
            bids: vec![Level { p: 0.5, s: 10.0 }, Level { p: 0.4, s: 20.0 }],
            asks: vec![Level { p: 0.6, s: 10.0 }],
        };

        let row2 = SnapshotRow {
            snapshot_id: 2,
            market_id: "m2".to_string(),
            pair: "YES/NO".to_string(),
            outcome: "NO".to_string(),
            source: "rest".to_string(),
            exchange_ts_us: None,
            local_recv_ts_us: base_ts_us + 1_000,
            local_write_ts_us: base_ts_us + 1_100,
            best_bid: 0.3,
            best_ask: 0.7,
            spread: 0.4,
            bids: vec![],
            asks: vec![Level { p: 0.7, s: 100.0 }],
        };

        recorder.buffer.push(row1);
        recorder.buffer.push(row2);
        recorder.flush()?;

        let expected_dt = DateTime::<Utc>::from_timestamp_micros(base_ts_us).unwrap();
        let expected_dir = format!(
            "{}/date={}/hour={:02}",
            base_dir_str,
            expected_dt.format("%Y-%m-%d"),
            expected_dt.hour()
        );
        let filename = format!("{}/part-000000.parquet", expected_dir);
        assert!(Path::new(&filename).exists());

        let file = File::open(&filename)?;
        let df = ParquetReader::new(file).finish()?;

        assert_eq!(df.height(), 2);

        let market_ids = df.column("market_id")?.str()?;
        assert_eq!(market_ids.get(0), Some("m1"));
        assert_eq!(market_ids.get(1), Some("m2"));

        let bids = df.column("bids")?.list()?;
        let row1_bids = bids.get_as_series(0).expect("row1 bids missing");
        let row2_bids = bids.get_as_series(1).expect("row2 bids missing");
        assert_eq!(row1_bids.len(), 2);
        assert_eq!(row2_bids.len(), 0);

        let _ = std::fs::remove_file(&filename);
        let _ = std::fs::remove_dir_all(&base_dir);
        Ok(())
    }
}
