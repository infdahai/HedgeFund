use std::collections::HashMap;
use std::time::Duration;

use super::config::CliConfig;
use super::types::{MarketTick, MicrostructureReport};

pub(crate) fn build_microstructure_reports(
    markets: &HashMap<String, Vec<MarketTick>>,
    config: &CliConfig,
    scan_skip_rates: &HashMap<String, f64>,
    default_scan_skip_rate: f64,
) -> Vec<MicrostructureReport> {
    markets
        .iter()
        .map(|(market_id, ticks)| {
            let scan_skip_rate = scan_skip_rates
                .get(market_id)
                .copied()
                .unwrap_or(default_scan_skip_rate);
            MicroTotals::from_ticks(market_id, ticks, config, scan_skip_rate)
        })
        .collect()
}

#[derive(Default)]
struct MicroTotals {
    total_ticks: u64,
    liquid_ticks: u64,
    spreads: Vec<f64>,
    slippage_rates: Vec<f64>,
    opportunity_ticks: u64,
    first_ts_us: Option<i64>,
    last_ts_us: Option<i64>,
}

impl MicroTotals {
    fn from_ticks(
        market_id: &str,
        ticks: &[MarketTick],
        config: &CliConfig,
        scan_skip_rate: f64,
    ) -> MicrostructureReport {
        let mut totals = MicroTotals::default();
        for tick in ticks {
            totals.total_ticks += 1;
            totals.first_ts_us = Some(
                totals
                    .first_ts_us
                    .map_or(tick.ts_us, |value| value.min(tick.ts_us)),
            );
            totals.last_ts_us = Some(
                totals
                    .last_ts_us
                    .map_or(tick.ts_us, |value| value.max(tick.ts_us)),
            );

            let yes = &tick.yes;
            let no = &tick.no;
            if yes.best_bid > 0.0 && yes.best_ask > 0.0 && no.best_bid > 0.0 && no.best_ask > 0.0 {
                totals.liquid_ticks += 1;
                let spread_yes = (yes.best_ask - yes.best_bid).max(0.0);
                let spread_no = (no.best_ask - no.best_bid).max(0.0);
                totals.spreads.push((spread_yes + spread_no) / 2.0);

                let slippage_yes = slippage_rate_for_size(
                    100.0,
                    yes.best_bid,
                    yes.best_ask,
                    yes.bid_size,
                    yes.ask_size,
                );
                let slippage_no = slippage_rate_for_size(
                    100.0,
                    no.best_bid,
                    no.best_ask,
                    no.bid_size,
                    no.ask_size,
                );
                totals
                    .slippage_rates
                    .push((slippage_yes + slippage_no) / 2.0);
            }

            if yes.best_ask > 0.0 && no.best_ask > 0.0 {
                if yes.best_ask + no.best_ask < 1.0 - config.min_profit_rate {
                    totals.opportunity_ticks += 1;
                }
            }
        }

        totals.finish(market_id.to_string(), scan_skip_rate)
    }

    fn finish(mut self, market_id: String, scan_skip_rate: f64) -> MicrostructureReport {
        let liquidity_pct = if self.total_ticks > 0 {
            self.liquid_ticks as f64 / self.total_ticks as f64 * 100.0
        } else {
            0.0
        };

        let spread_p50 = percentile(&mut self.spreads, 0.50);
        let spread_p95 = percentile(&mut self.spreads, 0.95);
        let slippage_100 = percentile(&mut self.slippage_rates, 0.50);

        let hours = match (self.first_ts_us, self.last_ts_us) {
            (Some(start), Some(end)) if end > start => {
                Duration::from_micros((end - start) as u64).as_secs_f64() / 3600.0
            }
            _ => 0.0,
        };
        let opp_frequency = if hours > 0.0 {
            self.opportunity_ticks as f64 / hours
        } else {
            0.0
        };

        MicrostructureReport {
            market_id,
            liquidity_availability_pct: liquidity_pct,
            scan_skip_rate,
            spread_p50,
            spread_p95,
            slippage_100_shares: slippage_100,
            opportunity_frequency_per_hour: opp_frequency,
        }
    }
}

fn slippage_rate_for_size(
    size: f64,
    best_bid: f64,
    best_ask: f64,
    bid_size: f64,
    ask_size: f64,
) -> f64 {
    if size <= 0.0 || best_bid <= 0.0 || best_ask <= 0.0 {
        return 0.0;
    }
    let spread = (best_ask - best_bid).max(0.0);
    let base = spread / best_ask;
    let depth = ask_size.min(bid_size);
    if depth <= 0.0 {
        return base;
    }
    let impact = (size / depth).max(0.0) * 0.01;
    base + impact
}

fn percentile(values: &mut Vec<f64>, quantile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((values.len() - 1) as f64 * quantile).round() as usize;
    values.get(idx).copied().unwrap_or(0.0)
}
