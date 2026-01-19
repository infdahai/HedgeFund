use chrono::Utc;
use rusqlite::{params, Connection, Result as SqlResult};
use std::path::Path;
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
            .send(RecordEvent::ConfirmedOrder {
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
            })
            .await;
    }
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
    ConfirmedOrder {
        order_id: String,
        market_id: String,
        token_id: String,
        side: String,
        expected_price: f64,
        actual_fill: String,
        position_before: String,
        position_after: String,
        confirmation_time_ms: u64,
        timestamp_ms: i64,
    },
}

pub struct Recorder {
    config: DataCollectionConfig,
    conn: Connection,
}

impl Recorder {
    pub fn new(config: DataCollectionConfig) -> SqlResult<Self> {
        let db_path = Path::new(&config.db_path);
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(db_path)?;
        let recorder = Self { config, conn };
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

        let mut level = 0;
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

    fn insert_confirmed_order(
        &mut self,
        order_id: &str,
        market_id: &str,
        token_id: &str,
        side: &str,
        expected_price: f64,
        actual_fill: &str,
        position_before: &str,
        position_after: &str,
        confirmation_time_ms: u64,
        timestamp_ms: i64,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO confirmed_orders
            (order_id, market_id, token_id, side, expected_price, actual_fill, position_before, position_after, confirmation_time_ms, timestamp_ms)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                order_id,
                market_id,
                token_id,
                side,
                expected_price,
                actual_fill,
                position_before,
                position_after,
                confirmation_time_ms,
                timestamp_ms
            ],
        )?;
        Ok(())
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

async fn flush_events(recorder: &Arc<Mutex<Recorder>>, buffer: &mut Vec<RecordEvent>) {
    let events: Vec<RecordEvent> = buffer.drain(..).collect();
    if events.is_empty() {
        return;
    }

    let recorder = recorder.lock().await;
    for event in events {
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
            RecordEvent::ConfirmedOrder {
                order_id,
                market_id,
                token_id,
                side,
                expected_price,
                actual_fill,
                position_before,
                position_after,
                confirmation_time_ms,
                timestamp_ms,
            } => {
                if let Err(err) = recorder.insert_confirmed_order(
                    &order_id,
                    &market_id,
                    &token_id,
                    &side,
                    expected_price,
                    &actual_fill,
                    &position_before,
                    &position_after,
                    confirmation_time_ms,
                    timestamp_ms,
                ) {
                    tracing::error!("Failed to insert confirmed order: {}", err);
                }
            }
        }
    }
}
