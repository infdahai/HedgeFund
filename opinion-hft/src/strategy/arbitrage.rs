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
    /// Total cost to execute (should be < 1.0 for profit)
    pub total_cost: f64,
    /// Gross profit rate before fees (1.0 - total_cost)
    pub gross_profit_rate: f64,
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
        
        // Opinion has a $0.50 minimum fee, but for rate calculation we use the curve first.
        // The actual fee cost check needs the amount, which we do in detection.
        base_fee * (1.0 - self.referral_discount)
    }
}

/// Arbitrage detector for Opinion.trade markets.
pub struct ArbitrageDetector {
    config: ArbitrageConfig,
    opportunity_counter: u64,
}

impl ArbitrageDetector {
    /// Create a new arbitrage detector with the given configuration.
    pub fn new(config: ArbitrageConfig) -> Self {
        Self {
            config,
            opportunity_counter: 0,
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(ArbitrageConfig::default())
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
        let yes_ask = yes_orderbook.best_ask()?;
        let no_ask = no_orderbook.best_ask()?;

        let total_cost_unit = yes_ask + no_ask;
        
        // No opportunity if cost >= 1.0
        if total_cost_unit >= 1.0 {
            return None;
        }

        // Determine max executable size (limited by smaller liquidity)
        let yes_liquidity = yes_orderbook.asks.first().map(|a| a.size).unwrap_or(0.0);
        let no_liquidity = no_orderbook.asks.first().map(|a| a.size).unwrap_or(0.0);
        let max_size = yes_liquidity.min(no_liquidity);

        // Check minimum liquidity
        if max_size < self.config.min_liquidity {
            return None;
        }

        // Calculate Fees
        // Fee = Rate * Notional. Notional = Price * Size.
        let yes_fee_rate = self.config.calculate_fee_rate(yes_ask);
        let no_fee_rate = self.config.calculate_fee_rate(no_ask);
        
        let yes_cost = yes_ask * max_size;
        let no_cost = no_ask * max_size;
        
        // Apply minimum fee floor ($0.50) floor per order
        // Assuming 1 unit = 1 USDT for simplicity in hft context
        let yes_fee = (yes_cost * yes_fee_rate).max(0.5);
        let no_fee = (no_cost * no_fee_rate).max(0.5);
        
        let total_spend = yes_cost + no_cost + yes_fee + no_fee;
        let total_payout = 1.0 * max_size; // Winning side pays 1.0, losing pays 0.0. You hold both, so 1.0 * size.
        
        let net_profit_abs = total_payout - total_spend;
        let net_profit_rate = net_profit_abs / total_spend;

        // Check minimum profit threshold
        if net_profit_rate < self.config.min_profit_rate {
            return None;
        }

        self.opportunity_counter += 1;
        let id = format!("arb-yesno-{}-{}", market.id, self.opportunity_counter);

        Some(ArbitrageOpportunity {
            id,
            arb_type: ArbitrageType::YesNo,
            market_id: market.id.clone(),
            total_cost: total_cost_unit,
            gross_profit_rate: 1.0 - total_cost_unit,
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

        // Calculate total cost to buy all outcomes
        let mut total_cost = 0.0;
        let mut min_liquidity = f64::MAX;
        let mut legs = Vec::new();

        for outcome in outcomes {
            let ask = outcome.orderbook.best_ask()?;
            let liquidity = outcome.orderbook.asks.first().map(|a| a.size).unwrap_or(0.0);

            if liquidity < self.config.min_liquidity {
                return None; // Insufficient liquidity in one leg
            }

            total_cost += ask;
            min_liquidity = min_liquidity.min(liquidity);

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
            return None;
        }

        let gross_profit = 1.0 - total_cost;
        
        // Calculate fees for all legs based on their specific prices
        let total_fees: f64 = legs.iter().map(|leg| {
            let fee_rate = self.config.calculate_fee_rate(leg.price);
            // Simplified for rate check: fee_cost = price * fee_rate
            leg.price * fee_rate
        }).sum();

        let net_profit = gross_profit - total_fees;

        if net_profit < self.config.min_profit_rate {
            return None;
        }

        // Set size for all legs
        for leg in &mut legs {
            leg.size = min_liquidity;
        }

        self.opportunity_counter += 1;
        let id = format!("arb-negrisk-{}-{}", market_id, self.opportunity_counter);

        Some(ArbitrageOpportunity {
            id,
            arb_type: ArbitrageType::NegRisk,
            market_id: market_id.to_string(),
            total_cost,
            gross_profit_rate: gross_profit,
            net_profit_rate: net_profit,
            max_size: min_liquidity,
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

        // Potential profit from placing orders inside the spread
        let spread_captured = ask_price - bid_price;
        
        // Calculate fees for both legs
        let bid_fee = bid_price * self.config.calculate_fee_rate(bid_price);
        let ask_fee = ask_price * self.config.calculate_fee_rate(ask_price);
        
        let net_profit = spread_captured - bid_fee - ask_fee;

        if net_profit < self.config.min_profit_rate {
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
            gross_profit_rate: spread_captured,
            net_profit_rate: net_profit,
            max_size: orderbook.bids.first().map(|b| b.size).unwrap_or(0.0),
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
        let yes_bid = yes_orderbook.best_bid()?;
        let no_bid = no_orderbook.best_bid()?;

        let total_revenue = yes_bid + no_bid;
        
        // No opportunity if revenue <= 1.0
        if total_revenue <= 1.0 {
            return None;
        }

        // Determine max executable size (limited by bid liquidity)
        let yes_liquidity = yes_orderbook.bids.first().map(|b| b.size).unwrap_or(0.0);
        let no_liquidity = no_orderbook.bids.first().map(|b| b.size).unwrap_or(0.0);
        let max_size = yes_liquidity.min(no_liquidity);

        if max_size < self.config.min_liquidity {
            return None;
        }

        // Cost is always 1.0 (Minting)
        // Fees: Taker fee on both Sells.
        let yes_fee = self.config.calculate_fee_rate(yes_bid) * yes_bid;
        let no_fee = self.config.calculate_fee_rate(no_bid) * no_bid;
        
        // Net Revenue = (Yes_Bid - Fee) + (No_Bid - Fee)
        let net_revenue = (yes_bid - yes_fee) + (no_bid - no_fee);
        let net_profit = net_revenue - 1.0;

        if net_profit < self.config.min_profit_rate {
            return None;
        }

        self.opportunity_counter += 1;
        let id = format!("arb-mintsell-{}-{}", market.id, self.opportunity_counter);

        Some(ArbitrageOpportunity {
            id,
            arb_type: ArbitrageType::MintSell,
            market_id: market.id.clone(),
            total_cost: 1.0,
            gross_profit_rate: total_revenue - 1.0,
            net_profit_rate: net_profit,
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
        actual_size * self.net_profit_rate
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
            gross_profit_rate: 0.03,
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
