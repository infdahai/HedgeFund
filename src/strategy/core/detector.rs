//! Arbitrage strategy module for Opinion.trade prediction markets.
//!
//! This module implements arbitrage detection and execution for:
//! - Yes/No binary market arbitrage
//! - Multi-outcome (Neg Risk) arbitrage for categorical markets
//! - Mint-Sell arbitrage (Risk-free yield)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::fees::{fee_amount, fee_rate, FeeConfig};
use super::{MarketInfo, OrderbookSide, TradeSide};

fn best_price(levels: &[OrderbookSide]) -> Option<f64> {
    levels.first().map(|level| level.price)
}

fn best_size(levels: &[OrderbookSide]) -> f64 {
    levels.first().map(|level| level.size).unwrap_or(0.0)
}

/// Type of arbitrage opportunity detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArbitrageType {
    /// Yes + No prices sum to less than 1.0 in a binary market
    YesNo,
    /// All outcomes sum to less than 1.0 in a categorical market
    NegRisk,
    /// Abnormal spread between bid and ask
    SpreadAnomaly,
    /// Mint pairs and sell into bids (Sum(Bids) > 1.0)
    MintSell,
}

/// Represents a detected arbitrage opportunity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    /// Unique identifier for this opportunity
    pub id: String,
    /// Type of arbitrage
    pub arb_type: ArbitrageType,
    /// Market ID
    pub market_id: String,
    /// Unit cost (sum of prices) before fees
    pub total_cost: f64,
    /// Total spend including fees for max_size
    pub total_spend: f64,
    /// Gross profit rate before fees (unit payout vs unit cost)
    pub gross_profit_rate: f64,
    /// Net profit in absolute terms for max_size
    pub net_profit_abs: f64,
    /// Net profit rate after estimated fees
    pub net_profit_rate: f64,
    /// Maximum executable size (limited by liquidity)
    pub max_size: f64,
    /// Individual legs of the arbitrage
    pub legs: Vec<ArbitrageLeg>,
    /// Timestamp when detected
    pub detected_at: u64,
    /// Whether this opportunity is still valid
    pub is_valid: bool,
}

/// A single leg (order) in an arbitrage trade.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageLeg {
    /// Trading pair (e.g., "BTC-YES" or "BTC-NO")
    pub pair: String,
    /// Token ID (ERC1155 ID)
    pub token_id: String,
    /// Side to trade (always Buy for standard arb)
    pub side: TradeSide,
    /// Target price
    pub price: f64,
    /// Target size
    pub size: f64,
    /// Available liquidity at this price
    pub available_liquidity: f64,
}

/// Configuration for arbitrage detection.
#[derive(Debug, Clone)]
pub struct ArbitrageConfig {
    /// Minimum profit rate to consider (after fees)
    pub min_profit_rate: f64,
    /// Minimum liquidity required per leg
    pub min_liquidity: f64,
    /// Minimum notional per order (USDT)
    pub min_order_notional: f64,
    /// Minimum fee per order (USDT)
    pub min_fee: f64,
    /// Base topic fee rate (usually 0.02 for 2%)
    pub topic_fee_rate: f64,
    /// Referral discount (e.g., 0.10 for 10% rebate)
    pub referral_discount: f64,
    /// Maximum slippage tolerance
    pub max_slippage: f64,
}

impl Default for ArbitrageConfig {
    fn default() -> Self {
        Self {
            min_profit_rate: 0.005,   // 0.5% minimum profit
            min_liquidity: 100.0,     // Minimum 100 units
            min_order_notional: 1.30, // API minimum order notional
            min_fee: 0.50,            // API minimum fee per order
            topic_fee_rate: 0.02,     // 2% max fee base
            referral_discount: 0.10,  // 10% rebate
            max_slippage: 0.01,       // 1% max slippage
        }
    }
}

impl ArbitrageConfig {
    pub fn fee_config(&self) -> FeeConfig {
        FeeConfig {
            min_fee: self.min_fee,
            topic_fee_rate: self.topic_fee_rate,
            referral_discount: self.referral_discount,
        }
    }

    /// Calculate fee rate for a given price based on Opinion's symmetric curve:
    /// Fee = topic_rate * price * (1 - price)
    /// This peaks at p=0.5 (max uncertainty) and tapers to 0 at edges.
    pub fn calculate_fee_rate(&self, price: f64) -> f64 {
        fee_rate(price, self.fee_config())
    }
}

#[derive(Debug, Clone)]
pub struct ArbScanStats {
    pub yesno_checked: u64,
    pub yesno_missing_ask: u64,
    pub yesno_cost_ge_one: u64,
    pub yesno_low_liquidity: u64,
    pub yesno_below_min_notional: u64,
    pub yesno_below_min_profit: u64,
    pub yesno_found: u64,
    pub yesno_best_overpay: f64,
    pub mint_sell_checked: u64,
    pub mint_sell_missing_bid: u64,
    pub mint_sell_revenue_le_one: u64,
    pub mint_sell_low_liquidity: u64,
    pub mint_sell_below_min_notional: u64,
    pub mint_sell_below_min_profit: u64,
    pub mint_sell_found: u64,
    pub mint_sell_best_shortfall: f64,
    pub neg_risk_checked: u64,
    pub neg_risk_missing_ask: u64,
    pub neg_risk_cost_ge_one: u64,
    pub neg_risk_low_liquidity: u64,
    pub neg_risk_below_min_notional: u64,
    pub neg_risk_below_min_profit: u64,
    pub neg_risk_found: u64,
    pub neg_risk_best_overpay: f64,
}

impl ArbScanStats {
    pub fn snapshot(&self) -> Self {
        self.clone()
    }

    pub fn delta(&self, previous: &Self) -> Self {
        Self {
            yesno_checked: self.yesno_checked.saturating_sub(previous.yesno_checked),
            yesno_missing_ask: self
                .yesno_missing_ask
                .saturating_sub(previous.yesno_missing_ask),
            yesno_cost_ge_one: self
                .yesno_cost_ge_one
                .saturating_sub(previous.yesno_cost_ge_one),
            yesno_low_liquidity: self
                .yesno_low_liquidity
                .saturating_sub(previous.yesno_low_liquidity),
            yesno_below_min_notional: self
                .yesno_below_min_notional
                .saturating_sub(previous.yesno_below_min_notional),
            yesno_below_min_profit: self
                .yesno_below_min_profit
                .saturating_sub(previous.yesno_below_min_profit),
            yesno_found: self.yesno_found.saturating_sub(previous.yesno_found),
            yesno_best_overpay: if self.yesno_best_overpay < previous.yesno_best_overpay {
                self.yesno_best_overpay
            } else {
                f64::INFINITY
            },
            mint_sell_checked: self
                .mint_sell_checked
                .saturating_sub(previous.mint_sell_checked),
            mint_sell_missing_bid: self
                .mint_sell_missing_bid
                .saturating_sub(previous.mint_sell_missing_bid),
            mint_sell_revenue_le_one: self
                .mint_sell_revenue_le_one
                .saturating_sub(previous.mint_sell_revenue_le_one),
            mint_sell_low_liquidity: self
                .mint_sell_low_liquidity
                .saturating_sub(previous.mint_sell_low_liquidity),
            mint_sell_below_min_notional: self
                .mint_sell_below_min_notional
                .saturating_sub(previous.mint_sell_below_min_notional),
            mint_sell_below_min_profit: self
                .mint_sell_below_min_profit
                .saturating_sub(previous.mint_sell_below_min_profit),
            mint_sell_found: self
                .mint_sell_found
                .saturating_sub(previous.mint_sell_found),
            mint_sell_best_shortfall: if self.mint_sell_best_shortfall
                < previous.mint_sell_best_shortfall
            {
                self.mint_sell_best_shortfall
            } else {
                f64::INFINITY
            },
            neg_risk_checked: self
                .neg_risk_checked
                .saturating_sub(previous.neg_risk_checked),
            neg_risk_missing_ask: self
                .neg_risk_missing_ask
                .saturating_sub(previous.neg_risk_missing_ask),
            neg_risk_cost_ge_one: self
                .neg_risk_cost_ge_one
                .saturating_sub(previous.neg_risk_cost_ge_one),
            neg_risk_low_liquidity: self
                .neg_risk_low_liquidity
                .saturating_sub(previous.neg_risk_low_liquidity),
            neg_risk_below_min_notional: self
                .neg_risk_below_min_notional
                .saturating_sub(previous.neg_risk_below_min_notional),
            neg_risk_below_min_profit: self
                .neg_risk_below_min_profit
                .saturating_sub(previous.neg_risk_below_min_profit),
            neg_risk_found: self.neg_risk_found.saturating_sub(previous.neg_risk_found),
            neg_risk_best_overpay: if self.neg_risk_best_overpay < previous.neg_risk_best_overpay {
                self.neg_risk_best_overpay
            } else {
                f64::INFINITY
            },
        }
    }

    pub fn total_checked(&self) -> u64 {
        self.yesno_checked + self.mint_sell_checked + self.neg_risk_checked
    }

    pub fn total_found(&self) -> u64 {
        self.yesno_found + self.mint_sell_found + self.neg_risk_found
    }

    pub fn summary(&self) -> String {
        let yesno_overpay = if self.yesno_best_overpay.is_finite() {
            format!("{:.6}", self.yesno_best_overpay)
        } else {
            "n/a".to_string()
        };
        let mint_shortfall = if self.mint_sell_best_shortfall.is_finite() {
            format!("{:.6}", self.mint_sell_best_shortfall)
        } else {
            "n/a".to_string()
        };
        let neg_risk_overpay = if self.neg_risk_best_overpay.is_finite() {
            format!("{:.6}", self.neg_risk_best_overpay)
        } else {
            "n/a".to_string()
        };
        format!(
            "yesno: checked={}, found={}, no_ask={}, cost>=1={}, low_liq={}, min_notional={}, min_profit={}, best_overpay={}; \
mint_sell: checked={}, found={}, no_bid={}, revenue<=1={}, low_liq={}, min_notional={}, min_profit={}, best_shortfall={}; \
neg_risk: checked={}, found={}, no_ask={}, cost>=1={}, low_liq={}, min_notional={}, min_profit={}, best_overpay={}",
            self.yesno_checked,
            self.yesno_found,
            self.yesno_missing_ask,
            self.yesno_cost_ge_one,
            self.yesno_low_liquidity,
            self.yesno_below_min_notional,
            self.yesno_below_min_profit,
            yesno_overpay,
            self.mint_sell_checked,
            self.mint_sell_found,
            self.mint_sell_missing_bid,
            self.mint_sell_revenue_le_one,
            self.mint_sell_low_liquidity,
            self.mint_sell_below_min_notional,
            self.mint_sell_below_min_profit,
            mint_shortfall,
            self.neg_risk_checked,
            self.neg_risk_found,
            self.neg_risk_missing_ask,
            self.neg_risk_cost_ge_one,
            self.neg_risk_low_liquidity,
            self.neg_risk_below_min_notional,
            self.neg_risk_below_min_profit,
            neg_risk_overpay
        )
    }
}

impl Default for ArbScanStats {
    fn default() -> Self {
        Self {
            yesno_checked: 0,
            yesno_missing_ask: 0,
            yesno_cost_ge_one: 0,
            yesno_low_liquidity: 0,
            yesno_below_min_notional: 0,
            yesno_below_min_profit: 0,
            yesno_found: 0,
            yesno_best_overpay: f64::INFINITY,
            mint_sell_checked: 0,
            mint_sell_missing_bid: 0,
            mint_sell_revenue_le_one: 0,
            mint_sell_low_liquidity: 0,
            mint_sell_below_min_notional: 0,
            mint_sell_below_min_profit: 0,
            mint_sell_found: 0,
            mint_sell_best_shortfall: f64::INFINITY,
            neg_risk_checked: 0,
            neg_risk_missing_ask: 0,
            neg_risk_cost_ge_one: 0,
            neg_risk_low_liquidity: 0,
            neg_risk_below_min_notional: 0,
            neg_risk_below_min_profit: 0,
            neg_risk_found: 0,
            neg_risk_best_overpay: f64::INFINITY,
        }
    }
}

/// Arbitrage detector for Opinion.trade markets.
pub struct ArbitrageDetector {
    config: ArbitrageConfig,
    opportunity_counter: u64,
    stats: ArbScanStats,
}

impl ArbitrageDetector {
    /// Create a new arbitrage detector with the given configuration.
    pub fn new(config: ArbitrageConfig) -> Self {
        Self {
            config,
            opportunity_counter: 0,
            stats: ArbScanStats::default(),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(ArbitrageConfig::default())
    }

    pub fn drain_stats(&mut self) -> ArbScanStats {
        std::mem::take(&mut self.stats)
    }

    pub fn stats_snapshot(&self) -> ArbScanStats {
        self.stats.snapshot()
    }

    /// Detect Yes/No arbitrage opportunity in a binary market.
    ///
    /// Returns Some if `yes_ask + no_ask < 1.0` with sufficient profit margin.
    pub fn detect_yesno_arbitrage(
        &mut self,
        market: &MarketInfo,
        yes_bids: &[OrderbookSide],
        yes_asks: &[OrderbookSide],
        no_bids: &[OrderbookSide],
        no_asks: &[OrderbookSide],
        detected_at: u64,
    ) -> Option<ArbitrageOpportunity> {
        let _ = (yes_bids, no_bids);
        self.stats.yesno_checked += 1;

        let yes_ask = match best_price(yes_asks) {
            Some(price) => price,
            None => {
                self.stats.yesno_missing_ask += 1;
                return None;
            }
        };
        let no_ask = match best_price(no_asks) {
            Some(price) => price,
            None => {
                self.stats.yesno_missing_ask += 1;
                return None;
            }
        };

        let total_cost_unit = yes_ask + no_ask;

        // No opportunity if cost >= 1.0
        if total_cost_unit >= 1.0 {
            self.stats.yesno_cost_ge_one += 1;
            let overpay = total_cost_unit - 1.0;
            if overpay < self.stats.yesno_best_overpay {
                self.stats.yesno_best_overpay = overpay;
            }
            return None;
        }

        // Determine max executable size (limited by smaller liquidity)
        let yes_liquidity = best_size(yes_asks);
        let no_liquidity = best_size(no_asks);
        let max_size = yes_liquidity.min(no_liquidity);

        // Check minimum liquidity
        if max_size < self.config.min_liquidity {
            self.stats.yesno_low_liquidity += 1;
            return None;
        }

        let min_size_yes = self.config.min_order_notional / yes_ask;
        let min_size_no = self.config.min_order_notional / no_ask;
        let min_required_size = min_size_yes.max(min_size_no);
        if max_size < min_required_size {
            self.stats.yesno_below_min_notional += 1;
            return None;
        }

        // Calculate Fees
        // Fee = Rate * Notional. Notional = Price * Size.
        let fee_config = self.config.fee_config();

        let yes_cost = yes_ask * max_size;
        let no_cost = no_ask * max_size;

        // Apply minimum fee floor per order
        let yes_fee = fee_amount(yes_ask, max_size, fee_config);
        let no_fee = fee_amount(no_ask, max_size, fee_config);

        let total_spend = yes_cost + no_cost + yes_fee + no_fee;
        let total_payout = 1.0 * max_size; // Winning side pays 1.0, losing pays 0.0. You hold both, so 1.0 * size.

        let net_profit_abs = total_payout - total_spend;
        let net_profit_rate = if total_spend > 0.0 {
            net_profit_abs / total_spend
        } else {
            0.0
        };

        // Check minimum profit threshold
        if net_profit_rate < self.config.min_profit_rate {
            self.stats.yesno_below_min_profit += 1;
            return None;
        }

        self.opportunity_counter += 1;
        let id = format!("arb-yesno-{}-{}", market.id, self.opportunity_counter);
        self.stats.yesno_found += 1;
        let gross_profit_rate = if total_cost_unit > 0.0 {
            (1.0 - total_cost_unit) / total_cost_unit
        } else {
            0.0
        };

        Some(ArbitrageOpportunity {
            id,
            arb_type: ArbitrageType::YesNo,
            market_id: market.id.clone(),
            total_cost: total_cost_unit,
            total_spend,
            gross_profit_rate,
            net_profit_abs,
            net_profit_rate,
            max_size,
            legs: vec![
                ArbitrageLeg {
                    pair: market.yes_pair.clone(),
                    token_id: market.yes_token_id.clone(),
                    side: TradeSide::Buy,
                    price: yes_ask,
                    size: max_size,
                    available_liquidity: yes_liquidity,
                },
                ArbitrageLeg {
                    pair: market.no_pair.clone(),
                    token_id: market.no_token_id.clone(),
                    side: TradeSide::Buy,
                    price: no_ask,
                    size: max_size,
                    available_liquidity: no_liquidity,
                },
            ],
            detected_at,
            is_valid: true,
        })
    }

    /// Detect Neg Risk arbitrage in a categorical (multi-outcome) market.
    ///
    /// Returns Some if sum of all outcome asks < 1.0 with sufficient profit margin.
    pub fn detect_neg_risk_arbitrage(
        &mut self,
        market_id: &str,
        outcomes: &[OutcomeOrderbook],
        detected_at: u64,
    ) -> Option<ArbitrageOpportunity> {
        if outcomes.is_empty() {
            return None;
        }

        self.stats.neg_risk_checked += 1;

        // Calculate total cost to buy all outcomes
        let mut total_cost = 0.0;
        let mut min_liquidity = f64::MAX;
        let mut min_required_size = 0.0_f64;
        let mut legs = Vec::new();

        for outcome in outcomes {
            let ask = match best_price(&outcome.asks) {
                Some(price) => price,
                None => {
                    self.stats.neg_risk_missing_ask += 1;
                    return None;
                }
            };
            let liquidity = best_size(&outcome.asks);

            if liquidity < self.config.min_liquidity {
                self.stats.neg_risk_low_liquidity += 1;
                return None; // Insufficient liquidity in one leg
            }

            total_cost += ask;
            min_liquidity = min_liquidity.min(liquidity);
            min_required_size = min_required_size.max(self.config.min_order_notional / ask);

            legs.push(ArbitrageLeg {
                pair: outcome.pair.clone(),
                token_id: outcome.token_id.clone(),
                side: TradeSide::Buy,
                price: ask,
                size: 0.0, // Will be set after determining max_size
                available_liquidity: liquidity,
            });
        }

        // No opportunity if cost >= 1.0
        if total_cost >= 1.0 {
            self.stats.neg_risk_cost_ge_one += 1;
            let overpay = total_cost - 1.0;
            if overpay < self.stats.neg_risk_best_overpay {
                self.stats.neg_risk_best_overpay = overpay;
            }
            return None;
        }

        if min_liquidity < min_required_size {
            self.stats.neg_risk_below_min_notional += 1;
            return None;
        }

        let exec_size = min_liquidity;
        let gross_profit_rate = if total_cost > 0.0 {
            (1.0 - total_cost) / total_cost
        } else {
            0.0
        };

        // Calculate fees for all legs based on their specific prices
        let mut total_spend = 0.0;
        let fee_config = self.config.fee_config();
        for leg in &legs {
            let cost = leg.price * exec_size;
            let fee = fee_amount(leg.price, exec_size, fee_config);
            total_spend += cost + fee;
        }

        let total_payout = 1.0 * exec_size;
        let net_profit_abs = total_payout - total_spend;
        let net_profit_rate = if total_spend > 0.0 {
            net_profit_abs / total_spend
        } else {
            0.0
        };

        if net_profit_rate < self.config.min_profit_rate {
            self.stats.neg_risk_below_min_profit += 1;
            return None;
        }

        // Set size for all legs
        for leg in &mut legs {
            leg.size = exec_size;
        }

        self.opportunity_counter += 1;
        let id = format!("arb-negrisk-{}-{}", market_id, self.opportunity_counter);
        self.stats.neg_risk_found += 1;

        Some(ArbitrageOpportunity {
            id,
            arb_type: ArbitrageType::NegRisk,
            market_id: market_id.to_string(),
            total_cost,
            total_spend,
            gross_profit_rate,
            net_profit_abs,
            net_profit_rate,
            max_size: exec_size,
            legs,
            detected_at,
            is_valid: true,
        })
    }

    /// Detect abnormal spread opportunity.
    ///
    /// Returns Some if spread is abnormally wide, suggesting market making opportunity.
    pub fn detect_spread_anomaly(
        &mut self,
        market: &MarketInfo,
        pair: &str,
        bids: &[OrderbookSide],
        asks: &[OrderbookSide],
        normal_spread: f64,
        detected_at: u64,
    ) -> Option<ArbitrageOpportunity> {
        let bid = best_price(bids)?;
        let ask = best_price(asks)?;
        let spread = ask - bid;
        let mid_price = (ask + bid) / 2.0;
        let spread_pct = spread / mid_price;

        // Only trigger if spread is significantly wider than normal
        if spread_pct <= normal_spread * 2.0 {
            return None;
        }

        let bid_price = bid + 0.1;
        let ask_price = ask - 0.1;

        let exec_size = best_size(bids);
        if exec_size <= 0.0 {
            return None;
        }

        // Potential profit from placing orders inside the spread
        let spread_captured = ask_price - bid_price;
        let gross_profit_rate = spread_captured / bid_price;

        // Calculate fees for both legs
        let bid_cost = bid_price * exec_size;
        let ask_revenue = ask_price * exec_size;
        let fee_config = self.config.fee_config();
        let bid_fee = fee_amount(bid_price, exec_size, fee_config);
        let ask_fee = fee_amount(ask_price, exec_size, fee_config);

        let total_spend = bid_cost + bid_fee + ask_fee;
        let net_profit_abs = (ask_revenue - bid_cost) - (bid_fee + ask_fee);
        let net_profit_rate = if total_spend > 0.0 {
            net_profit_abs / total_spend
        } else {
            0.0
        };

        if net_profit_rate < self.config.min_profit_rate {
            return None;
        }

        self.opportunity_counter += 1;
        let id = format!("arb-spread-{}-{}", market.id, self.opportunity_counter);

        // Determine token ID based on pair
        let token_id = if pair == market.yes_pair {
            market.yes_token_id.clone()
        } else if pair == market.no_pair {
            market.no_token_id.clone()
        } else {
            "0".to_string()
        };

        Some(ArbitrageOpportunity {
            id,
            arb_type: ArbitrageType::SpreadAnomaly,
            market_id: market.id.clone(),
            total_cost: 0.0, // Not applicable for spread arb
            total_spend,
            gross_profit_rate,
            net_profit_abs,
            net_profit_rate,
            max_size: exec_size,
            legs: vec![
                ArbitrageLeg {
                    pair: pair.to_string(),
                    token_id: token_id.clone(),
                    side: TradeSide::Buy,
                    price: bid_price, // Place bid slightly higher
                    size: 0.0,
                    available_liquidity: 0.0,
                },
                ArbitrageLeg {
                    pair: pair.to_string(),
                    token_id,
                    side: TradeSide::Sell,
                    price: ask_price, // Place ask slightly lower
                    size: 0.0,
                    available_liquidity: 0.0,
                },
            ],
            detected_at,
            is_valid: true,
        })
    }

    /// Detect Mint-Sell arbitrage (Risk-free yield).
    ///
    /// Returns Some if `yes_bid + no_bid > 1.0`.
    /// Strategy: Mint YES+NO for $1.0, sell both into bids.
    pub fn detect_mint_sell_arbitrage(
        &mut self,
        market: &MarketInfo,
        yes_bids: &[OrderbookSide],
        yes_asks: &[OrderbookSide],
        no_bids: &[OrderbookSide],
        no_asks: &[OrderbookSide],
        detected_at: u64,
    ) -> Option<ArbitrageOpportunity> {
        let _ = (yes_asks, no_asks);
        self.stats.mint_sell_checked += 1;

        let yes_bid = match best_price(yes_bids) {
            Some(price) => price,
            None => {
                self.stats.mint_sell_missing_bid += 1;
                return None;
            }
        };
        let no_bid = match best_price(no_bids) {
            Some(price) => price,
            None => {
                self.stats.mint_sell_missing_bid += 1;
                return None;
            }
        };

        let total_revenue = yes_bid + no_bid;

        // No opportunity if revenue <= 1.0
        if total_revenue <= 1.0 {
            self.stats.mint_sell_revenue_le_one += 1;
            let shortfall = 1.0 - total_revenue;
            if shortfall < self.stats.mint_sell_best_shortfall {
                self.stats.mint_sell_best_shortfall = shortfall;
            }
            return None;
        }

        // Determine max executable size (limited by bid liquidity)
        let yes_liquidity = best_size(yes_bids);
        let no_liquidity = best_size(no_bids);
        let max_size = yes_liquidity.min(no_liquidity);

        if max_size < self.config.min_liquidity {
            self.stats.mint_sell_low_liquidity += 1;
            return None;
        }

        let min_size_yes = self.config.min_order_notional / yes_bid;
        let min_size_no = self.config.min_order_notional / no_bid;
        let min_required_size = min_size_yes.max(min_size_no);
        if max_size < min_required_size {
            self.stats.mint_sell_below_min_notional += 1;
            return None;
        }

        // Cost is always 1.0 (Minting)
        // Fees: Taker fee on both Sells.
        let yes_revenue = yes_bid * max_size;
        let no_revenue = no_bid * max_size;
        let fee_config = self.config.fee_config();
        let yes_fee = fee_amount(yes_bid, max_size, fee_config);
        let no_fee = fee_amount(no_bid, max_size, fee_config);

        let total_spend = max_size + yes_fee + no_fee;
        let total_payout = yes_revenue + no_revenue;
        let net_profit_abs = total_payout - total_spend;
        let net_profit_rate = if total_spend > 0.0 {
            net_profit_abs / total_spend
        } else {
            0.0
        };
        let gross_profit_rate = (total_revenue - 1.0) / 1.0;

        if net_profit_rate < self.config.min_profit_rate {
            self.stats.mint_sell_below_min_profit += 1;
            return None;
        }

        self.opportunity_counter += 1;
        let id = format!("arb-mintsell-{}-{}", market.id, self.opportunity_counter);
        self.stats.mint_sell_found += 1;

        Some(ArbitrageOpportunity {
            id,
            arb_type: ArbitrageType::MintSell,
            market_id: market.id.clone(),
            total_cost: 1.0,
            total_spend,
            gross_profit_rate,
            net_profit_abs,
            net_profit_rate,
            max_size,
            legs: vec![
                ArbitrageLeg {
                    pair: market.yes_pair.clone(),
                    token_id: market.yes_token_id.clone(),
                    side: TradeSide::Sell,
                    price: yes_bid,
                    size: max_size,
                    available_liquidity: yes_liquidity,
                },
                ArbitrageLeg {
                    pair: market.no_pair.clone(),
                    token_id: market.no_token_id.clone(),
                    side: TradeSide::Sell,
                    price: no_bid,
                    size: max_size,
                    available_liquidity: no_liquidity,
                },
            ],
            detected_at,
            is_valid: true,
        })
    }
}

/// Orderbook for a specific outcome in a categorical market.
#[derive(Debug, Clone)]
pub struct OutcomeOrderbook {
    /// Outcome identifier
    pub outcome_id: String,
    /// Token ID
    pub token_id: String,
    /// Trading pair for this outcome
    pub pair: String,
    /// Orderbook bids
    pub bids: Vec<OrderbookSide>,
    /// Orderbook asks
    pub asks: Vec<OrderbookSide>,
}

impl ArbitrageOpportunity {
    /// Calculate expected profit in absolute terms for a given size.
    pub fn expected_profit(&self, size: f64) -> f64 {
        let actual_size = size.min(self.max_size);
        if self.max_size <= 0.0 {
            return 0.0;
        }
        let profit_per_unit = self.net_profit_abs / self.max_size;
        profit_per_unit * actual_size
    }

    /// Check if opportunity is still profitable with current prices.
    pub fn validate(&self, current_prices: &HashMap<String, f64>) -> bool {
        let current_cost: f64 = self
            .legs
            .iter()
            .filter_map(|leg| current_prices.get(&leg.pair))
            .sum();

        match self.arb_type {
            ArbitrageType::YesNo | ArbitrageType::NegRisk => current_cost < 1.0,
            ArbitrageType::SpreadAnomaly => true, // Spread opportunities are time-sensitive
            ArbitrageType::MintSell => current_cost > 1.0, // Revenue > Cost(1.0)
        }
    }

    /// Get a summary string for logging.
    pub fn summary(&self) -> String {
        format!(
            "[{}] {} | Market: {} | Cost: {:.4} | Profit: {:.2}% | Size: {:.2}",
            self.id,
            match self.arb_type {
                ArbitrageType::YesNo => "YES/NO",
                ArbitrageType::NegRisk => "NEG-RISK",
                ArbitrageType::SpreadAnomaly => "SPREAD",
                ArbitrageType::MintSell => "MINT-SELL",
            },
            self.market_id,
            self.total_cost,
            self.net_profit_rate * 100.0,
            self.max_size
        )
    }
}
