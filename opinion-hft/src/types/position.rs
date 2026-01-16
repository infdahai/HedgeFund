use alloy_primitives::{Address, U256};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Position {
    pub token_id: U256,
    pub balance: U256,
    pub avg_entry_price: Option<f64>,
}

impl Position {
    pub fn new(token_id: U256, balance: U256) -> Self {
        Self {
            token_id,
            balance,
            avg_entry_price: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.balance.is_zero()
    }
}

#[derive(Debug, Clone, Default)]
pub struct PositionSnapshot {
    pub account: Address,
    pub positions: Vec<Position>,
    pub timestamp: u64,
}

impl PositionSnapshot {
    pub fn get_balance(&self, token_id: &U256) -> U256 {
        self.positions
            .iter()
            .find(|p| &p.token_id == token_id)
            .map(|p| p.balance)
            .unwrap_or(U256::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_is_empty() {
        let empty = Position::new(U256::from(1), U256::ZERO);
        assert!(empty.is_empty());

        let non_empty = Position::new(U256::from(1), U256::from(100));
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_snapshot_get_balance() {
        let snapshot = PositionSnapshot {
            account: Address::ZERO,
            positions: vec![
                Position::new(U256::from(1), U256::from(100)),
                Position::new(U256::from(2), U256::from(200)),
            ],
            timestamp: 0,
        };

        assert_eq!(snapshot.get_balance(&U256::from(1)), U256::from(100));
        assert_eq!(snapshot.get_balance(&U256::from(2)), U256::from(200));
        assert_eq!(snapshot.get_balance(&U256::from(3)), U256::ZERO);
    }
}
