use super::detector::ArbitrageOpportunity;

#[derive(Debug, Clone, Copy)]
pub struct RiskSnapshot {
    pub current_exposure: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct RiskRules {
    pub max_exposure: f64,
    pub max_trade_notional: f64,
}

impl RiskRules {
    pub fn approve(&self, signal: &ArbitrageOpportunity, snapshot: &RiskSnapshot) -> bool {
        if signal.total_spend > self.max_trade_notional {
            return false;
        }
        snapshot.current_exposure + signal.total_spend <= self.max_exposure
    }
}
