use crate::strategy::core::TradeSide;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct MicroPosition {
    pub market_id: String,
    pub pair: String,
    pub side: TradeSide, // Buy (long) or Sell (short/prediction NO)
    pub entry_price: f64,
    pub entry_obi: f64, // OBI at entry
    pub quantity: f64,
    pub notional: f64, // entry_price * quantity
    pub entry_time: Instant,
    pub order_id: String,

    // Dynamic tracking
    pub peak_price: f64,   // Highest price seen (for trailing logic if needed)
    pub trough_price: f64, // Lowest price seen
    pub current_pnl: f64,  // Unrealized P&L
}

impl MicroPosition {
    pub fn new(
        market_id: String,
        pair: String,
        side: TradeSide,
        entry_price: f64,
        entry_obi: f64,
        quantity: f64,
        order_id: String,
    ) -> Self {
        Self {
            notional: entry_price * quantity,
            peak_price: entry_price,
            trough_price: entry_price,
            current_pnl: 0.0,
            market_id,
            pair,
            side,
            entry_price,
            entry_obi,
            quantity,
            entry_time: Instant::now(),
            order_id,
        }
    }

    pub fn update_price(&mut self, current_mid: f64) {
        if current_mid > self.peak_price {
            self.peak_price = current_mid;
        }
        if current_mid < self.trough_price {
            self.trough_price = current_mid;
        }

        // PnL = (current - entry) * qty * direction
        let price_delta = current_mid - self.entry_price;
        self.current_pnl = match self.side {
            TradeSide::Buy => price_delta * self.quantity,
            TradeSide::Sell => -price_delta * self.quantity,
        };
    }

    pub fn holding_duration(&self) -> Duration {
        self.entry_time.elapsed()
    }

    pub fn return_pct(&self) -> f64 {
        if self.notional > 0.0 {
            self.current_pnl / self.notional
        } else {
            0.0
        }
    }
}
