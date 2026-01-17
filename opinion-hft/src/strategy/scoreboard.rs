use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::Serialize;
use tracing::info;

#[derive(Debug, Clone, Serialize)]
pub struct StrategyStats {
    pub name: String,
    pub signals: u64,
    pub fills: u64,
    pub profit: f64,
    pub weight: f64,
}

#[derive(Clone)]
pub struct Scoreboard {
    stats: Arc<DashMap<String, Mutex<StrategyStats>>>,
}

impl Default for Scoreboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Scoreboard {
    pub fn new() -> Self {
        Self {
            stats: Arc::new(DashMap::new()),
        }
    }

    pub async fn register_signal(&self, strategy_name: &str) {
        let entry = self.stats.entry(strategy_name.to_string()).or_insert_with(|| {
            Mutex::new(StrategyStats {
                name: strategy_name.to_string(),
                signals: 0,
                fills: 0,
                profit: 0.0,
                weight: 1.0,
            })
        });
        let mut stats = entry.lock().await;
        stats.signals += 1;
    }

    pub async fn register_fill(&self, strategy_name: &str, profit: f64) {
        if let Some(entry) = self.stats.get(strategy_name) {
            let mut stats = entry.lock().await;
            stats.fills += 1;
            stats.profit += profit;
            
            if profit > 0.0 {
                stats.weight = (stats.weight * 1.05).min(2.0);
            } else if profit < 0.0 {
                stats.weight = (stats.weight * 0.95).max(0.1);
            }

            info!("Strategy Update: {} -> Profit: {}, New Weight: {:.2}", strategy_name, stats.profit, stats.weight);
        }
    }

    pub async fn get_weight(&self, strategy_name: &str) -> f64 {
        if let Some(entry) = self.stats.get(strategy_name) {
            entry.lock().await.weight
        } else {
            1.0
        }
    }
}
