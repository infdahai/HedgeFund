use chrono::Utc;
use rusqlite::{params, Connection, Result as SqlResult};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};

use crate::config::DataCollectionConfig;
use crate::data::rest::models::TokenOrderbook;
use crate::execution::confirmation::ConfirmedOrder;

#[derive(Debug, Clone)]
pub struct RecorderHandle {
    tx: mpsc::Sender<RecordEvent>,
}

impl RecorderHandle {
    pub async fn record_orderbook(&self, market_id: &str, pair: &str, ob: &TokenOrderbook) {
        let _ = self
            .tx
            .send(RecordEvent::OrderbookSnapshot {
                market_id: market_id.to_string(),
                pair: pair.to_string(),
                snapshot_ts: ob.timestamp,
                bids: ob
                    .bids
                    .iter()
                    .map(|b| (b.price.clone(), b.size.clone()))
                    .collect(),
                asks: ob
                    .asks
                    .iter()
                    .map(|a| (a.price.clone(), a.size.clone()))
                    .collect(),
            })
            .await;
    }

    pub async fn record_confirmed_order(&self, confirmed: &ConfirmedOrder) {
        let _ = self
            .tx
            .send(RecordEvent::ConfirmedOrder(ConfirmedOrderRecord {
                order_id: confirmed.order_id.clone(),
                market_id: confirmed.market_id.clone(),
                token_id: confirmed.token_id.to_string(),
                side: format!("{:?}", confirmed.side),
                expected_price: confirmed.expected_price,
                actual_fill: confirmed.actual_fill.to_string(),
                position_before: confirmed.position_before.to_string(),
                position_after: confirmed.position_after.to_string(),
                confirmation_time_ms: confirmed.confirmation_time.as_millis() as u64,
                timestamp_ms: Utc::now().timestamp_millis(),
            }))
            .await;
    }
}

#[derive(Debug)]
pub struct ConfirmedOrderRecord {
    pub order_id: String,
    pub market_id: String,
    pub token_id: String,
    pub side: String,
    pub expected_price: f64,
    pub actual_fill: String,
    pub position_before: String,
    pub position_after: String,
    pub confirmation_time_ms: u64,
    pub timestamp_ms: i64,
}

#[derive(Debug)]
pub enum RecordEvent {
    OrderbookSnapshot {
        market_id: String,
        pair: String,
        snapshot_ts: i64,
        bids: Vec<(String, String)>,
        asks: Vec<(String, String)>,
    },
    ConfirmedOrder(ConfirmedOrderRecord),
}

pub struct Recorder {
    config: DataCollectionConfig,
    conn: Connection,
    current_db_date: Option<String>,
}

fn build_daily_db_path(base_path: &str, date: &str) -> PathBuf {
    let trimmed = base_path.trim();
    let has_trailing_sep = trimmed.ends_with('/') || trimmed.ends_with('\\');
    let base_path = if has_trailing_sep || trimmed.is_empty() {
        PathBuf::from(trimmed).join("opinion-hft.sqlite")
    } else {
        PathBuf::from(trimmed)
    };

    let dir = base_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let file_name = base_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("opinion-hft.sqlite");
    let file_path = Path::new(file_name);
    let stem = file_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("opinion-hft");
    let dated_name = match file_path.extension().and_then(|value| value.to_str()) {
        Some(ext) => format!("{stem}-{date}.{ext}"),
        None => format!("{stem}-{date}"),
    };

    dir.join(dated_name)
}

impl Recorder {
    pub fn new(config: DataCollectionConfig) -> SqlResult<Self> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let db_path = build_daily_db_path(&config.db_path, &today);
        let db_dir = db_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        std::fs::create_dir_all(&db_dir).ok();

        let conn = Connection::open(&db_path)?;
        let current_db_date = Some(today.clone());
        let recorder = Self {
            config,
            conn,
            current_db_date,
        };
        recorder.init_schema()?;
        recorder.apply_pragmas()?;
        Ok(recorder)
    }

    fn apply_pragmas(&self) -> SqlResult<()> {
        if self.config.wal {
            self.conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        }
        self.conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
        Ok(())
    }

    fn init_schema(&self) -> SqlResult<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS orderbook_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                market_id TEXT NOT NULL,
                pair TEXT NOT NULL,
                snapshot_ts INTEGER NOT NULL,
                captured_at_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS orderbook_levels (
                snapshot_id INTEGER NOT NULL,
                side TEXT NOT NULL,
                price TEXT NOT NULL,
                size TEXT NOT NULL,
                level INTEGER NOT NULL,
                FOREIGN KEY(snapshot_id) REFERENCES orderbook_snapshots(id)
            );

            CREATE TABLE IF NOT EXISTS confirmed_orders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                order_id TEXT NOT NULL,
                market_id TEXT NOT NULL,
                token_id TEXT NOT NULL,
                side TEXT NOT NULL,
                expected_price REAL NOT NULL,
                actual_fill TEXT NOT NULL,
                position_before TEXT NOT NULL,
                position_after TEXT NOT NULL,
                confirmation_time_ms INTEGER NOT NULL,
                timestamp_ms INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_orderbook_snapshots_market_time
                ON orderbook_snapshots(market_id, captured_at_ms);

            CREATE INDEX IF NOT EXISTS idx_confirmed_orders_time
                ON confirmed_orders(timestamp_ms);
            ",
        )?;
        Ok(())
    }

    fn insert_orderbook_snapshot(
        &mut self,
        market_id: &str,
        pair: &str,
        snapshot_ts: i64,
        bids: &[(String, String)],
        asks: &[(String, String)],
    ) -> SqlResult<()> {
        self.maybe_rotate_db()?;

        let captured_at_ms = Utc::now().timestamp_millis();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO orderbook_snapshots (market_id, pair, snapshot_ts, captured_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            params![market_id, pair, snapshot_ts, captured_at_ms],
        )?;
        let snapshot_id = tx.last_insert_rowid();

        let mut level = 0;
        for (price, size) in bids.iter().take(self.config.orderbook_depth) {
            tx.execute(
                "INSERT INTO orderbook_levels (snapshot_id, side, price, size, level)
                 VALUES (?1, 'bid', ?2, ?3, ?4)",
                params![snapshot_id, price, size, level],
            )?;
            level += 1;
        }

        for (price, size) in asks.iter().take(self.config.orderbook_depth) {
            tx.execute(
                "INSERT INTO orderbook_levels (snapshot_id, side, price, size, level)
                 VALUES (?1, 'ask', ?2, ?3, ?4)",
                params![snapshot_id, price, size, level],
            )?;
            level += 1;
        }

        tx.commit()?;
        Ok(())
    }

    fn insert_confirmed_order(&mut self, record: &ConfirmedOrderRecord) -> SqlResult<()> {
        self.maybe_rotate_db()?;

        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO confirmed_orders
            (order_id, market_id, token_id, side, expected_price, actual_fill, position_before, position_after, confirmation_time_ms, timestamp_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                &record.order_id,
                &record.market_id,
                &record.token_id,
                &record.side,
                record.expected_price,
                &record.actual_fill,
                &record.position_before,
                &record.position_after,
                record.confirmation_time_ms,
                record.timestamp_ms
            ],
        )?;
        Ok(())
    }

    fn maybe_rotate_db(&mut self) -> SqlResult<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        if self.current_db_date.as_deref() == Some(&today) {
            return Ok(());
        }

        let new_db_path = build_daily_db_path(&self.config.db_path, &today);
        let db_dir = new_db_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        std::fs::create_dir_all(&db_dir).ok();

        self.conn = Connection::open(&new_db_path)?;
        self.current_db_date = Some(today.clone());
        self.init_schema()?;
        self.apply_pragmas()?;

        tracing::info!("Rotated to new daily database: {}", new_db_path.display());

        Ok(())
    }
}

async fn flush_events(recorder: &Arc<Mutex<Recorder>>, buffer: &mut Vec<RecordEvent>) {
    let mut recorder = recorder.lock().await;
    for event in buffer.drain(..) {
        match event {
            RecordEvent::OrderbookSnapshot {
                market_id,
                pair,
                snapshot_ts,
                bids,
                asks,
            } => {
                if let Err(err) =
                    recorder.insert_orderbook_snapshot(&market_id, &pair, snapshot_ts, &bids, &asks)
                {
                    tracing::error!("Failed to insert orderbook snapshot: {}", err);
                }
            }
            RecordEvent::ConfirmedOrder(record) => {
                if let Err(err) = recorder.insert_confirmed_order(&record) {
                    tracing::error!("Failed to insert confirmed order: {}", err);
                }
            }
        }
    }
}

pub async fn spawn_recorder(
    config: DataCollectionConfig,
) -> Option<(RecorderHandle, tokio::task::JoinHandle<()>)> {
    if !config.enabled {
        return None;
    }

    let (tx, mut rx) = mpsc::channel::<RecordEvent>(config.queue_capacity);
    let handle = RecorderHandle { tx };

    let recorder = match Recorder::new(config.clone()) {
        Ok(recorder) => recorder,
        Err(err) => {
            tracing::error!("Failed to initialize recorder: {}", err);
            return None;
        }
    };

    let recorder = Arc::new(Mutex::new(recorder));

    let join = tokio::spawn(async move {
        let mut tick = interval(Duration::from_millis(config.flush_interval_ms));
        let mut buffer: Vec<RecordEvent> = Vec::with_capacity(config.batch_size);
        loop {
            tokio::select! {
                maybe_event = rx.recv() => {
                    if let Some(event) = maybe_event {
                        buffer.push(event);
                        if buffer.len() >= config.batch_size {
                            flush_events(&recorder, &mut buffer).await;
                        }
                    } else {
                        break;
                    }
                }
                _ = tick.tick() => {
                    if !buffer.is_empty() {
                        flush_events(&recorder, &mut buffer).await;
                    }
                }
            }
        }

        if !buffer.is_empty() {
            flush_events(&recorder, &mut buffer).await;
        }
    });

    Some((handle, join))
}
