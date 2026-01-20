//! Arbitrage strategy module for Opinion.trade prediction markets.
//!
//! This module implements arbitrage detection and execution for:
//! - Yes/No binary market arbitrage
//! - Multi-outcome (Neg Risk) arbitrage for categorical markets
//! - Mint-Sell arbitrage (Risk-free yield)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::strategy::market_scanner::{MarketInfo, MarketOrderbook};

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

/// Trade side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeSide {
    Buy,
    Sell,
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
            min_profit_rate: 0.005, // 0.5% minimum profit
            min_liquidity: 100.0,   // Minimum 100 units
            min_order_notional: 1.30, // API minimum order notional
            min_fee: 0.50,          // API minimum fee per order
            topic_fee_rate: 0.02,   // 2% max fee base
            referral_discount: 0.10,// 10% rebate
            max_slippage: 0.01,     // 1% max slippage
        }
    }
}

impl ArbitrageConfig {
    /// Calculate fee rate for a given price based on Opinion's symmetric curve:
    /// Fee = topic_rate * price * (1 - price)
    /// This peaks at p=0.5 (max uncertainty) and tapers to 0 at edges.
    pub fn calculate_fee_rate(&self, price: f64) -> f64 {
        // Symmetric fee curve: topic_rate * p * (1-p)
        // At p=0.5, fee = topic_rate * 0.25
        // We apply discounts: (1 - referral_discount)
        let base_fee = self.topic_fee_rate * price * (1.0 - price);

        // Minimum fee is applied at the notional level during detection.
        base_fee * (1.0 - self.referral_discount)
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

    /// Detect Yes/No arbitrage opportunity in a binary market.
    ///
    /// Returns Some if `yes_ask + no_ask < 1.0` with sufficient profit margin.
    pub fn detect_yesno_arbitrage(
        &mut self,
        market: &MarketInfo,
        yes_orderbook: &MarketOrderbook,
        no_orderbook: &MarketOrderbook,
    ) -> Option<ArbitrageOpportunity> {
        self.stats.yesno_checked += 1;

        let yes_ask = match yes_orderbook.best_ask() {
            Some(price) => price,
            None => {
                self.stats.yesno_missing_ask += 1;
                return None;
            }
        };
        let no_ask = match no_orderbook.best_ask() {
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
        let yes_liquidity = yes_orderbook.asks.first().map(|a| a.size).unwrap_or(0.0);
        let no_liquidity = no_orderbook.asks.first().map(|a| a.size).unwrap_or(0.0);
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
        let yes_fee_rate = self.config.calculate_fee_rate(yes_ask);
        let no_fee_rate = self.config.calculate_fee_rate(no_ask);
        
        let yes_cost = yes_ask * max_size;
        let no_cost = no_ask * max_size;
        
        // Apply minimum fee floor per order
        let yes_fee = (yes_cost * yes_fee_rate).max(self.config.min_fee);
        let no_fee = (no_cost * no_fee_rate).max(self.config.min_fee);
        
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
            detected_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
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
            let ask = match outcome.orderbook.best_ask() {
                Some(price) => price,
                None => {
                    self.stats.neg_risk_missing_ask += 1;
                    return None;
                }
            };
            let liquidity = outcome.orderbook.asks.first().map(|a| a.size).unwrap_or(0.0);

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
        for leg in &legs {
            let fee_rate = self.config.calculate_fee_rate(leg.price);
            let cost = leg.price * exec_size;
            let fee = (cost * fee_rate).max(self.config.min_fee);
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
            detected_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            is_valid: true,
        })
    }

    /// Detect abnormal spread opportunity.
    ///
    /// Returns Some if spread is abnormally wide, suggesting market making opportunity.
    pub fn detect_spread_anomaly(
        &mut self,
        market: &MarketInfo,
        orderbook: &MarketOrderbook,
        normal_spread: f64,
    ) -> Option<ArbitrageOpportunity> {
        let spread = orderbook.spread()?;
        let mid_price = orderbook.mid_price()?;
        let spread_pct = spread / mid_price;

        // Only trigger if spread is significantly wider than normal
        if spread_pct <= normal_spread * 2.0 {
            return None;
        }

        let bid = orderbook.best_bid()?;
        let ask = orderbook.best_ask()?;

        let bid_price = bid + 0.1;
        let ask_price = ask - 0.1;

        let exec_size = orderbook.bids.first().map(|b| b.size).unwrap_or(0.0);
        if exec_size <= 0.0 {
            return None;
        }

        // Potential profit from placing orders inside the spread
        let spread_captured = ask_price - bid_price;
        let gross_profit_rate = spread_captured / bid_price;

        // Calculate fees for both legs
        let bid_cost = bid_price * exec_size;
        let ask_revenue = ask_price * exec_size;
        let bid_fee = (bid_cost * self.config.calculate_fee_rate(bid_price)).max(self.config.min_fee);
        let ask_fee = (ask_revenue * self.config.calculate_fee_rate(ask_price)).max(self.config.min_fee);

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
        let token_id = if orderbook.pair == market.yes_pair {
            market.yes_token_id.clone()
        } else if orderbook.pair == market.no_pair {
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
                    pair: orderbook.pair.clone(),
                    token_id: token_id.clone(),
                    side: TradeSide::Buy,
                    price: bid_price, // Place bid slightly higher
                    size: 0.0,
                    available_liquidity: 0.0,
                },
                ArbitrageLeg {
                    pair: orderbook.pair.clone(),
                    token_id,
                    side: TradeSide::Sell,
                    price: ask_price, // Place ask slightly lower
                    size: 0.0,
                    available_liquidity: 0.0,
                },
            ],
            detected_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
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
        yes_orderbook: &MarketOrderbook,
        no_orderbook: &MarketOrderbook,
    ) -> Option<ArbitrageOpportunity> {
        self.stats.mint_sell_checked += 1;

        let yes_bid = match yes_orderbook.best_bid() {
            Some(price) => price,
            None => {
                self.stats.mint_sell_missing_bid += 1;
                return None;
            }
        };
        let no_bid = match no_orderbook.best_bid() {
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
        let yes_liquidity = yes_orderbook.bids.first().map(|b| b.size).unwrap_or(0.0);
        let no_liquidity = no_orderbook.bids.first().map(|b| b.size).unwrap_or(0.0);
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
        let yes_fee = (yes_revenue * self.config.calculate_fee_rate(yes_bid)).max(self.config.min_fee);
        let no_fee = (no_revenue * self.config.calculate_fee_rate(no_bid)).max(self.config.min_fee);

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
            detected_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
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
    /// Orderbook data
    pub orderbook: MarketOrderbook,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::market_scanner::OrderbookSide;

    fn mock_market() -> MarketInfo {
        MarketInfo {
            id: "TEST-MARKET".into(),
            question: "Test question?".into(),
            yes_token_id: "1".into(),
            no_token_id: "2".into(),
            yes_pair: "TEST-YES".into(),
            no_pair: "TEST-NO".into(),
            volume_24h: 10000.0,
            liquidity: 5000.0,
            is_active: true,
            market_type: None,
            root_market_id: None,
            parent_title: None,
        }
    }

    fn mock_orderbook(pair: &str, best_ask: f64, liquidity: f64) -> MarketOrderbook {
        MarketOrderbook {
            pair: pair.into(),
            bids: vec![OrderbookSide {
                price: best_ask - 0.02,
                size: liquidity,
            }],
            asks: vec![OrderbookSide {
                price: best_ask,
                size: liquidity,
            }],
        }
    }

    #[test]
    fn test_yesno_arbitrage_detected() {
        let mut detector = ArbitrageDetector::with_defaults();
        let market = mock_market();

        // Yes @ 0.48, No @ 0.49 = 0.97 total (3% gross profit)
        let yes_ob = mock_orderbook("TEST-YES", 0.48, 1000.0);
        let no_ob = mock_orderbook("TEST-NO", 0.49, 1000.0);

        let opportunity = detector.detect_yesno_arbitrage(&market, &yes_ob, &no_ob);

        assert!(opportunity.is_some());
        let opp = opportunity.unwrap();
        assert_eq!(opp.arb_type, ArbitrageType::YesNo);
        assert!((opp.total_cost - 0.97).abs() < 0.0001);
        assert!(opp.gross_profit_rate > 0.02);
        assert!(opp.net_profit_rate > 0.0);
        assert_eq!(opp.legs.len(), 2);
    }

    #[test]
    fn test_mint_sell_arbitrage_detected() {
        let mut detector = ArbitrageDetector::with_defaults();
        let market = mock_market();

        // Yes Bid @ 0.52, No Bid @ 0.51 = 1.03 revenue
        // Mint @ 1.0, Sell @ 1.03 -> 3% gross
        let mut yes_ob = mock_orderbook("TEST-YES", 0.54, 1000.0); // Ask 0.54, Bid 0.52
        let mut no_ob = mock_orderbook("TEST-NO", 0.53, 1000.0); // Ask 0.53, Bid 0.51
        
        // Fix bids for mock
        yes_ob.bids[0].price = 0.52;
        no_ob.bids[0].price = 0.51;

        let opportunity = detector.detect_mint_sell_arbitrage(&market, &yes_ob, &no_ob);

        assert!(opportunity.is_some());
        let opp = opportunity.unwrap();
        assert_eq!(opp.arb_type, ArbitrageType::MintSell);
        assert_eq!(opp.total_cost, 1.0);
        assert!((opp.gross_profit_rate - 0.03).abs() < 0.0001);
        assert_eq!(opp.legs[0].side, TradeSide::Sell);
    }

    #[test]
    fn test_neg_risk_arbitrage_detected() {
        let mut detector = ArbitrageDetector::with_defaults();

        let outcomes = vec![
            OutcomeOrderbook {
                outcome_id: "A".into(),
                token_id: "1".into(),
                pair: "TEST-A".into(),
                orderbook: mock_orderbook("TEST-A", 0.22, 500.0),
            },
            OutcomeOrderbook {
                outcome_id: "B".into(),
                token_id: "2".into(),
                pair: "TEST-B".into(),
                orderbook: mock_orderbook("TEST-B", 0.25, 500.0),
            },
            OutcomeOrderbook {
                outcome_id: "C".into(),
                token_id: "3".into(),
                pair: "TEST-C".into(),
                orderbook: mock_orderbook("TEST-C", 0.28, 500.0),
            },
            OutcomeOrderbook {
                outcome_id: "D".into(),
                token_id: "4".into(),
                pair: "TEST-D".into(),
                orderbook: mock_orderbook("TEST-D", 0.20, 500.0),
            },
        ];

        // Total: 0.22 + 0.25 + 0.28 + 0.20 = 0.95
        let opportunity = detector.detect_neg_risk_arbitrage("MULTI-MARKET", &outcomes);

        assert!(opportunity.is_some());
        let opp = opportunity.unwrap();
        assert_eq!(opp.arb_type, ArbitrageType::NegRisk);
        assert!((opp.total_cost - 0.95).abs() < 0.0001);
        assert_eq!(opp.legs.len(), 4);
    }

    #[test]
    fn test_opportunity_summary() {
        let opp = ArbitrageOpportunity {
            id: "arb-yesno-1-1".into(),
            arb_type: ArbitrageType::YesNo,
            market_id: "BTC-100K".into(),
            total_cost: 0.97,
            total_spend: 0.97,
            gross_profit_rate: 0.03,
            net_profit_abs: 0.025,
            net_profit_rate: 0.026,
            max_size: 500.0,
            legs: vec![],
            detected_at: 0,
            is_valid: true,
        };

        let summary = opp.summary();
        assert!(summary.contains("YES/NO"));
        assert!(summary.contains("BTC-100K"));
        assert!(summary.contains("0.9700"));
    }
}
