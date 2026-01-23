use chrono::{DateTime, Timelike, Utc};
use polars::io::parquet::write::BatchedWriter;
use polars::prelude::*;
use polars_parquet::parquet::metadata::KeyValue;
use polars_parquet::read::read_metadata;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::ParquetConfig;
use crate::data::rest::models::TokenOrderbook;

const SNAPSHOT_ID_SHIFT: u32 = 12;
const SNAPSHOT_ID_MASK: u64 = (1 << SNAPSHOT_ID_SHIFT) - 1;
const PARQUET_SCHEMA_VERSION: &str = "v1.0.0";
const ORDERBOOK_DATASET_DIR: &str = "orderbook_snapshots_v1";
const SCAN_STATS_DATASET_DIR: &str = "scan_stats_v1";

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

#[derive(Debug, Clone)]
pub struct ScanStatsRow {
    pub market_id: String,
    pub scan_ts_us: i64,
    pub duration_us: u64,
    pub markets_scanned: u64,
    pub yesno_checked: u64,
    pub yesno_found: u64,
    pub yesno_missing_ask: u64,
    pub yesno_cost_ge_one: u64,
    pub yesno_low_liquidity: u64,
    pub yesno_below_min_notional: u64,
    pub yesno_below_min_profit: u64,
    pub yesno_best_overpay: f64,
    pub mint_sell_checked: u64,
    pub mint_sell_found: u64,
    pub mint_sell_missing_bid: u64,
    pub mint_sell_revenue_le_one: u64,
    pub mint_sell_low_liquidity: u64,
    pub mint_sell_below_min_notional: u64,
    pub mint_sell_below_min_profit: u64,
    pub mint_sell_best_shortfall: f64,
    pub neg_risk_checked: u64,
    pub neg_risk_found: u64,
    pub neg_risk_missing_ask: u64,
    pub neg_risk_cost_ge_one: u64,
    pub neg_risk_low_liquidity: u64,
    pub neg_risk_below_min_notional: u64,
    pub neg_risk_below_min_profit: u64,
    pub neg_risk_best_overpay: f64,
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
    ScanStats(ScanStatsRow),
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

    pub fn record_scan_stats(&self, stats: ScanStatsRow) {
        match self.sender.try_send(RecorderMsg::ScanStats(stats)) {
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
    let rotate_size_bytes = config.rotate_size_mb.saturating_mul(1024 * 1024);
    let metadata = build_parquet_metadata(&config);
    let (orderbook_dir, scan_stats_dir) = resolve_dataset_dirs(&base_dir);

    tracing::info!(
        "Parquet recorder started orderbook_dir={} scan_stats_dir={} batch_size={} flush_ms={} rotate_mb={} queue_cap={} compression={} heartbeat_secs={}",
        orderbook_dir,
        scan_stats_dir,
        batch_size,
        flush_interval.as_millis(),
        config.rotate_size_mb,
        capacity,
        config.compression,
        heartbeat_interval.as_secs()
    );

    let join = match thread::Builder::new()
        .name("parquet-recorder".to_string())
        .spawn(move || {
            let mut recorder = ParquetRecorder::new(
                orderbook_dir,
                scan_stats_dir,
                batch_size,
                compression,
                rotate_size_bytes,
                metadata,
            );
            let mut last_flush = Instant::now();
            let mut last_heartbeat = Instant::now();
            let mut last_dropped = 0u64;

            loop {
                let flush_due = flush_interval.saturating_sub(last_flush.elapsed());
                let heartbeat_due = heartbeat_interval.saturating_sub(last_heartbeat.elapsed());
                let wait_for = flush_due.min(heartbeat_due);

                match receiver.recv_timeout(wait_for) {
                    Ok(RecorderMsg::Data(row)) => {
                        recorder.orderbook_buffer.push(convert_row(row));
                        if recorder.orderbook_buffer.len() >= recorder.batch_size {
                            if let Err(err) = recorder.flush() {
                                tracing::warn!("Parquet flush failed: {}", err);
                            }
                            last_flush = Instant::now();
                        }
                    }
                    Ok(RecorderMsg::ScanStats(row)) => {
                        recorder.scan_stats_buffer.push(row);
                        if recorder.scan_stats_buffer.len() >= recorder.batch_size {
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
                        let _ = recorder.shutdown();
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        let _ = recorder.shutdown();
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

struct ParquetMetadata {
    schema_version: String,
    git_commit: String,
    config_hash: String,
}

struct DatasetWriter {
    current_dir: Option<String>,
    current_path: Option<String>,
    current_temp_path: Option<String>,
    current_schema_hash: Option<String>,
    current_size_bytes: u64,
    writer: Option<BatchedWriter<File>>,
}

impl DatasetWriter {
    fn new() -> Self {
        Self {
            current_dir: None,
            current_path: None,
            current_temp_path: None,
            current_schema_hash: None,
            current_size_bytes: 0,
            writer: None,
        }
    }

    fn open_writer(
        &mut self,
        dir_path: &str,
        part: u64,
        compression: ParquetCompression,
        df: &DataFrame,
    ) -> PolarsResult<()> {
        let filename = format!("{}/part-{:06}.parquet", dir_path, part);
        let temp_filename = format!("{}.tmp", filename);
        let file = File::create(&temp_filename)?;
        let writer = ParquetWriter::new(file)
            .with_compression(compression)
            .batched(&df.schema())?;

        self.current_dir = Some(dir_path.to_string());
        self.current_path = Some(filename);
        self.current_temp_path = Some(temp_filename);
        self.current_schema_hash = Some(schema_fingerprint(df));
        self.current_size_bytes = 0;
        self.writer = Some(writer);
        Ok(())
    }

    fn write_batch(&mut self, df: &DataFrame) -> PolarsResult<()> {
        if let Some(writer) = &mut self.writer {
            writer.write_batch(df)?;
        }
        Ok(())
    }

    fn refresh_size(&mut self) {
        let Some(path) = &self.current_temp_path else {
            return;
        };
        if let Ok(metadata) = std::fs::metadata(path) {
            self.current_size_bytes = metadata.len();
        }
    }

    fn finish(&mut self, metadata: &ParquetMetadata, mark_success: bool) -> PolarsResult<()> {
        let Some(writer) = self.writer.take() else {
            self.current_dir = None;
            self.current_path = None;
            self.current_temp_path = None;
            self.current_schema_hash = None;
            self.current_size_bytes = 0;
            return Ok(());
        };
        let schema_hash = self
            .current_schema_hash
            .take()
            .unwrap_or_else(|| "unknown".to_string());
        let key_values = build_kv_metadata(metadata, &schema_hash);
        let size = {
            let mut guard = writer
                .get_writer()
                .lock()
                .map_err(|_| PolarsError::ComputeError("Parquet writer lock poisoned".into()))?;
            guard
                .end(Some(key_values))
                .map_err(|err| PolarsError::ComputeError(err.to_string().into()))?
        };

        let temp_path = self
            .current_temp_path
            .clone()
            .unwrap_or_else(|| "".to_string());
        let final_path = self.current_path.clone().unwrap_or_else(|| "".to_string());
        if !temp_path.is_empty() && !final_path.is_empty() {
            validate_parquet_footer(&temp_path)?;
            let target = Path::new(&final_path);
            if target.exists() {
                return Err(PolarsError::ComputeError(
                    format!("parquet target already exists: {}", final_path).into(),
                ));
            }
            std::fs::rename(&temp_path, &final_path)?;
            if mark_success {
                let success_path = format!("{}/_SUCCESS", self.current_dir.as_deref().unwrap());
                let _ = File::create(success_path);
            }
        }

        self.current_dir = None;
        self.current_path = None;
        self.current_temp_path = None;
        self.current_schema_hash = None;
        self.current_size_bytes = size;
        Ok(())
    }
}

struct ParquetRecorder {
    orderbook_buffer: Vec<SnapshotRow>,
    scan_stats_buffer: Vec<ScanStatsRow>,
    batch_size: usize,
    orderbook_dir: String,
    scan_stats_dir: String,
    part_counters: HashMap<String, u64>,
    compression: ParquetCompression,
    rotate_size_bytes: u64,
    metadata: ParquetMetadata,
    orderbook_writer: DatasetWriter,
    scan_stats_writer: DatasetWriter,
}

impl ParquetRecorder {
    pub fn new(
        orderbook_dir: String,
        scan_stats_dir: String,
        batch_size: usize,
        compression: ParquetCompression,
        rotate_size_bytes: u64,
        metadata: ParquetMetadata,
    ) -> Self {
        Self {
            orderbook_buffer: Vec::with_capacity(batch_size),
            scan_stats_buffer: Vec::with_capacity(batch_size),
            batch_size,
            orderbook_dir,
            scan_stats_dir,
            part_counters: HashMap::new(),
            compression,
            rotate_size_bytes,
            metadata,
            orderbook_writer: DatasetWriter::new(),
            scan_stats_writer: DatasetWriter::new(),
        }
    }

    pub fn flush(&mut self) -> PolarsResult<()> {
        self.flush_orderbooks()?;
        self.flush_scan_stats()?;
        Ok(())
    }

    pub fn shutdown(&mut self) -> PolarsResult<()> {
        self.flush()?;
        self.orderbook_writer.finish(&self.metadata, true)?;
        self.scan_stats_writer.finish(&self.metadata, true)?;
        Ok(())
    }

    fn flush_orderbooks(&mut self) -> PolarsResult<()> {
        if self.orderbook_buffer.is_empty() {
            return Ok(());
        }

        let base_ts = self
            .orderbook_buffer
            .first()
            .map(|row| row.local_recv_ts_us)
            .unwrap_or_else(now_ts_us);
        let dir_path = resolve_partition_dir(&self.orderbook_dir, base_ts);
        std::fs::create_dir_all(&dir_path)?;

        let mut df = build_dataframe(&self.orderbook_buffer)?;
        write_dataframe(
            &mut self.orderbook_writer,
            &mut self.part_counters,
            &dir_path,
            &mut df,
            self.compression,
            self.rotate_size_bytes,
            &self.metadata,
        )?;
        self.orderbook_buffer.clear();
        Ok(())
    }

    fn flush_scan_stats(&mut self) -> PolarsResult<()> {
        if self.scan_stats_buffer.is_empty() {
            return Ok(());
        }

        let base_ts = self
            .scan_stats_buffer
            .first()
            .map(|row| row.scan_ts_us)
            .unwrap_or_else(now_ts_us);
        let dir_path = resolve_partition_dir(&self.scan_stats_dir, base_ts);
        std::fs::create_dir_all(&dir_path)?;

        let mut df = build_scan_stats_dataframe(&self.scan_stats_buffer)?;
        write_dataframe(
            &mut self.scan_stats_writer,
            &mut self.part_counters,
            &dir_path,
            &mut df,
            self.compression,
            self.rotate_size_bytes,
            &self.metadata,
        )?;
        self.scan_stats_buffer.clear();
        Ok(())
    }
}

fn write_dataframe(
    dataset: &mut DatasetWriter,
    part_counters: &mut HashMap<String, u64>,
    dir_path: &str,
    df: &mut DataFrame,
    compression: ParquetCompression,
    rotate_size_bytes: u64,
    metadata: &ParquetMetadata,
) -> PolarsResult<()> {
    if dataset.writer.is_none() || dataset.current_dir.as_deref() != Some(dir_path) {
        let mark_success = dataset
            .current_dir
            .as_deref()
            .is_some_and(|dir| dir != dir_path);
        dataset.finish(metadata, mark_success)?;
        let part = next_part(part_counters, dir_path);
        dataset.open_writer(dir_path, part, compression, df)?;
    }

    dataset.write_batch(df)?;
    dataset.refresh_size();

    if rotate_size_bytes > 0 && dataset.current_size_bytes >= rotate_size_bytes {
        dataset.finish(metadata, false)?;
    }

    Ok(())
}

fn next_part(part_counters: &mut HashMap<String, u64>, dir_path: &str) -> u64 {
    let entry = part_counters
        .entry(dir_path.to_string())
        .or_insert_with(|| scan_max_part_number(dir_path));
    let current = *entry;
    *entry = entry.saturating_add(1);
    current
}

fn resolve_dataset_dirs(base_dir: &str) -> (String, String) {
    let base_path = Path::new(base_dir);
    let suffix = base_path.file_name().and_then(|name| name.to_str());
    let root = match suffix {
        Some(ORDERBOOK_DATASET_DIR) | Some(SCAN_STATS_DATASET_DIR) => {
            base_path.parent().unwrap_or(Path::new("."))
        }
        _ => base_path,
    };
    let orderbook_dir = root.join(ORDERBOOK_DATASET_DIR);
    let scan_stats_dir = root.join(SCAN_STATS_DATASET_DIR);
    (
        orderbook_dir.to_string_lossy().to_string(),
        scan_stats_dir.to_string_lossy().to_string(),
    )
}

fn build_parquet_metadata(config: &ParquetConfig) -> ParquetMetadata {
    ParquetMetadata {
        schema_version: PARQUET_SCHEMA_VERSION.to_string(),
        git_commit: git_commit().to_string(),
        config_hash: config_hash(config),
    }
}

fn git_commit() -> &'static str {
    option_env!("GIT_COMMIT").unwrap_or("unknown")
}

fn config_hash(config: &ParquetConfig) -> String {
    let mut hasher = DefaultHasher::new();
    config.enabled.hash(&mut hasher);
    config.dir.hash(&mut hasher);
    config.batch_size.hash(&mut hasher);
    config.flush_interval_ms.hash(&mut hasher);
    config.rotate_size_mb.hash(&mut hasher);
    config.queue_capacity.hash(&mut hasher);
    config.compression.hash(&mut hasher);
    config.heartbeat_interval_secs.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn build_kv_metadata(metadata: &ParquetMetadata, schema_hash: &str) -> Vec<KeyValue> {
    vec![
        KeyValue::new(
            "schema_version".to_string(),
            metadata.schema_version.clone(),
        ),
        KeyValue::new("schema_hash".to_string(), schema_hash.to_string()),
        KeyValue::new("git_commit".to_string(), metadata.git_commit.clone()),
        KeyValue::new("config_hash".to_string(), metadata.config_hash.clone()),
    ]
}

fn schema_fingerprint(df: &DataFrame) -> String {
    let mut hasher = DefaultHasher::new();
    for (name, dtype) in df.schema().iter() {
        name.as_str().hash(&mut hasher);
        format!("{:?}", dtype).hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn scan_max_part_number(dir_path: &str) -> u64 {
    let entries = match std::fs::read_dir(dir_path) {
        Ok(entries) => entries,
        Err(_) => return 0,
    };
    let mut max_part: Option<u64> = None;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let file_name = match file_name.to_str() {
            Some(name) => name,
            None => continue,
        };
        let part = match parse_part_number(file_name) {
            Some(part) => part,
            None => continue,
        };
        max_part = Some(max_part.map_or(part, |current| current.max(part)));
    }
    max_part.map_or(0, |value| value.saturating_add(1))
}

fn parse_part_number(file_name: &str) -> Option<u64> {
    let suffix = file_name.strip_prefix("part-")?;
    let suffix = suffix.strip_suffix(".parquet")?;
    suffix.parse::<u64>().ok()
}

fn validate_parquet_footer(path: &str) -> PolarsResult<()> {
    let mut file = File::open(path)?;
    read_metadata(&mut file)
        .map(|_| ())
        .map_err(|err| PolarsError::ComputeError(err.to_string().into()))
}

fn build_scan_stats_dataframe(rows: &[ScanStatsRow]) -> PolarsResult<DataFrame> {
    let market_ids: Vec<String> = rows.iter().map(|row| row.market_id.clone()).collect();
    let scan_ts_us: Vec<i64> = rows.iter().map(|row| row.scan_ts_us).collect();
    let duration_us: Vec<u64> = rows.iter().map(|row| row.duration_us).collect();
    let markets_scanned: Vec<u64> = rows.iter().map(|row| row.markets_scanned).collect();
    let yesno_checked: Vec<u64> = rows.iter().map(|row| row.yesno_checked).collect();
    let yesno_found: Vec<u64> = rows.iter().map(|row| row.yesno_found).collect();
    let yesno_missing_ask: Vec<u64> = rows.iter().map(|row| row.yesno_missing_ask).collect();
    let yesno_cost_ge_one: Vec<u64> = rows.iter().map(|row| row.yesno_cost_ge_one).collect();
    let yesno_low_liquidity: Vec<u64> = rows.iter().map(|row| row.yesno_low_liquidity).collect();
    let yesno_below_min_notional: Vec<u64> = rows
        .iter()
        .map(|row| row.yesno_below_min_notional)
        .collect();
    let yesno_below_min_profit: Vec<u64> =
        rows.iter().map(|row| row.yesno_below_min_profit).collect();
    let yesno_best_overpay: Vec<f64> = rows.iter().map(|row| row.yesno_best_overpay).collect();
    let mint_sell_checked: Vec<u64> = rows.iter().map(|row| row.mint_sell_checked).collect();
    let mint_sell_found: Vec<u64> = rows.iter().map(|row| row.mint_sell_found).collect();
    let mint_sell_missing_bid: Vec<u64> =
        rows.iter().map(|row| row.mint_sell_missing_bid).collect();
    let mint_sell_revenue_le_one: Vec<u64> = rows
        .iter()
        .map(|row| row.mint_sell_revenue_le_one)
        .collect();
    let mint_sell_low_liquidity: Vec<u64> =
        rows.iter().map(|row| row.mint_sell_low_liquidity).collect();
    let mint_sell_below_min_notional: Vec<u64> = rows
        .iter()
        .map(|row| row.mint_sell_below_min_notional)
        .collect();
    let mint_sell_below_min_profit: Vec<u64> = rows
        .iter()
        .map(|row| row.mint_sell_below_min_profit)
        .collect();
    let mint_sell_best_shortfall: Vec<f64> = rows
        .iter()
        .map(|row| row.mint_sell_best_shortfall)
        .collect();
    let neg_risk_checked: Vec<u64> = rows.iter().map(|row| row.neg_risk_checked).collect();
    let neg_risk_found: Vec<u64> = rows.iter().map(|row| row.neg_risk_found).collect();
    let neg_risk_missing_ask: Vec<u64> = rows.iter().map(|row| row.neg_risk_missing_ask).collect();
    let neg_risk_cost_ge_one: Vec<u64> = rows.iter().map(|row| row.neg_risk_cost_ge_one).collect();
    let neg_risk_low_liquidity: Vec<u64> =
        rows.iter().map(|row| row.neg_risk_low_liquidity).collect();
    let neg_risk_below_min_notional: Vec<u64> = rows
        .iter()
        .map(|row| row.neg_risk_below_min_notional)
        .collect();
    let neg_risk_below_min_profit: Vec<u64> = rows
        .iter()
        .map(|row| row.neg_risk_below_min_profit)
        .collect();
    let neg_risk_best_overpay: Vec<f64> =
        rows.iter().map(|row| row.neg_risk_best_overpay).collect();

    DataFrame::new(vec![
        Series::new("market_id".into(), market_ids).into(),
        Series::new("scan_ts_us".into(), scan_ts_us).into(),
        Series::new("duration_us".into(), duration_us).into(),
        Series::new("markets_scanned".into(), markets_scanned).into(),
        Series::new("yesno_checked".into(), yesno_checked).into(),
        Series::new("yesno_found".into(), yesno_found).into(),
        Series::new("yesno_missing_ask".into(), yesno_missing_ask).into(),
        Series::new("yesno_cost_ge_one".into(), yesno_cost_ge_one).into(),
        Series::new("yesno_low_liquidity".into(), yesno_low_liquidity).into(),
        Series::new("yesno_below_min_notional".into(), yesno_below_min_notional).into(),
        Series::new("yesno_below_min_profit".into(), yesno_below_min_profit).into(),
        Series::new("yesno_best_overpay".into(), yesno_best_overpay).into(),
        Series::new("mint_sell_checked".into(), mint_sell_checked).into(),
        Series::new("mint_sell_found".into(), mint_sell_found).into(),
        Series::new("mint_sell_missing_bid".into(), mint_sell_missing_bid).into(),
        Series::new("mint_sell_revenue_le_one".into(), mint_sell_revenue_le_one).into(),
        Series::new("mint_sell_low_liquidity".into(), mint_sell_low_liquidity).into(),
        Series::new(
            "mint_sell_below_min_notional".into(),
            mint_sell_below_min_notional,
        )
        .into(),
        Series::new(
            "mint_sell_below_min_profit".into(),
            mint_sell_below_min_profit,
        )
        .into(),
        Series::new("mint_sell_best_shortfall".into(), mint_sell_best_shortfall).into(),
        Series::new("neg_risk_checked".into(), neg_risk_checked).into(),
        Series::new("neg_risk_found".into(), neg_risk_found).into(),
        Series::new("neg_risk_missing_ask".into(), neg_risk_missing_ask).into(),
        Series::new("neg_risk_cost_ge_one".into(), neg_risk_cost_ge_one).into(),
        Series::new("neg_risk_low_liquidity".into(), neg_risk_low_liquidity).into(),
        Series::new(
            "neg_risk_below_min_notional".into(),
            neg_risk_below_min_notional,
        )
        .into(),
        Series::new(
            "neg_risk_below_min_profit".into(),
            neg_risk_below_min_profit,
        )
        .into(),
        Series::new("neg_risk_best_overpay".into(), neg_risk_best_overpay).into(),
    ])
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

fn build_level_list_series<F>(
    name: &str,
    rows: &[SnapshotRow],
    extractor: F,
) -> PolarsResult<Series>
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
    let fields = [price_series, size_series];

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
        let (orderbook_dir, scan_stats_dir) = resolve_dataset_dirs(&base_dir_str);
        let metadata = build_parquet_metadata(&ParquetConfig::default());
        let mut recorder = ParquetRecorder::new(
            orderbook_dir.clone(),
            scan_stats_dir,
            2,
            ParquetCompression::Zstd(None),
            0,
            metadata,
        );

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

        recorder.orderbook_buffer.push(row1);
        recorder.orderbook_buffer.push(row2);
        recorder.flush()?;
        recorder.shutdown()?;

        let expected_dt = DateTime::<Utc>::from_timestamp_micros(base_ts_us).unwrap();
        let expected_dir = format!(
            "{}/date={}/hour={:02}",
            orderbook_dir,
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

    #[test]
    fn test_scan_stats_round_trip() -> PolarsResult<()> {
        let base_ts_us = 1_700_000_100_000_000i64;
        let base_dir = std::env::temp_dir().join("scan_stats_roundtrip");
        let base_dir_str = base_dir.to_string_lossy().to_string();
        let (orderbook_dir, scan_stats_dir) = resolve_dataset_dirs(&base_dir_str);
        let metadata = build_parquet_metadata(&ParquetConfig::default());
        let mut recorder = ParquetRecorder::new(
            orderbook_dir,
            scan_stats_dir.clone(),
            2,
            ParquetCompression::Zstd(None),
            0,
            metadata,
        );

        let row1 = ScanStatsRow {
            market_id: "ALL".to_string(),
            scan_ts_us: base_ts_us,
            duration_us: 500,
            markets_scanned: 10,
            yesno_checked: 10,
            yesno_found: 1,
            yesno_missing_ask: 2,
            yesno_cost_ge_one: 3,
            yesno_low_liquidity: 4,
            yesno_below_min_notional: 5,
            yesno_below_min_profit: 6,
            yesno_best_overpay: 0.012,
            mint_sell_checked: 8,
            mint_sell_found: 2,
            mint_sell_missing_bid: 1,
            mint_sell_revenue_le_one: 1,
            mint_sell_low_liquidity: 1,
            mint_sell_below_min_notional: 1,
            mint_sell_below_min_profit: 1,
            mint_sell_best_shortfall: 0.034,
            neg_risk_checked: 7,
            neg_risk_found: 1,
            neg_risk_missing_ask: 1,
            neg_risk_cost_ge_one: 2,
            neg_risk_low_liquidity: 3,
            neg_risk_below_min_notional: 4,
            neg_risk_below_min_profit: 5,
            neg_risk_best_overpay: 0.056,
        };
        let row2 = ScanStatsRow {
            market_id: "ALL".to_string(),
            scan_ts_us: base_ts_us + 1_000_000,
            duration_us: 800,
            markets_scanned: 12,
            yesno_checked: 12,
            yesno_found: 0,
            yesno_missing_ask: 1,
            yesno_cost_ge_one: 2,
            yesno_low_liquidity: 3,
            yesno_below_min_notional: 4,
            yesno_below_min_profit: 5,
            yesno_best_overpay: f64::INFINITY,
            mint_sell_checked: 9,
            mint_sell_found: 0,
            mint_sell_missing_bid: 2,
            mint_sell_revenue_le_one: 2,
            mint_sell_low_liquidity: 2,
            mint_sell_below_min_notional: 2,
            mint_sell_below_min_profit: 2,
            mint_sell_best_shortfall: f64::INFINITY,
            neg_risk_checked: 8,
            neg_risk_found: 0,
            neg_risk_missing_ask: 2,
            neg_risk_cost_ge_one: 2,
            neg_risk_low_liquidity: 2,
            neg_risk_below_min_notional: 2,
            neg_risk_below_min_profit: 2,
            neg_risk_best_overpay: f64::INFINITY,
        };

        recorder.scan_stats_buffer.push(row1);
        recorder.scan_stats_buffer.push(row2);
        recorder.flush()?;
        recorder.shutdown()?;

        let expected_dt = DateTime::<Utc>::from_timestamp_micros(base_ts_us).unwrap();
        let expected_dir = format!(
            "{}/date={}/hour={:02}",
            scan_stats_dir,
            expected_dt.format("%Y-%m-%d"),
            expected_dt.hour()
        );
        let filename = format!("{}/part-000000.parquet", expected_dir);
        assert!(Path::new(&filename).exists());

        let file = File::open(&filename)?;
        let df = ParquetReader::new(file).finish()?;
        assert_eq!(df.height(), 2);
        assert!(df.column("yesno_best_overpay").is_ok());
        assert!(df.column("mint_sell_best_shortfall").is_ok());
        assert!(df.column("neg_risk_best_overpay").is_ok());

        let _ = std::fs::remove_file(&filename);
        let _ = std::fs::remove_dir_all(&base_dir);
        Ok(())
    }
}
