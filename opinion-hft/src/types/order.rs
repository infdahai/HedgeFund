use alloy_primitives::{Address, U256};
use serde::{Deserialize, Serialize};

fn price_to_fraction(price: f64) -> (u128, u128) {
    let price_str = format!("{:.6}", price);
    let parts: Vec<&str> = price_str.split('.').collect();

    if parts.len() == 1 {
        let num = parts[0].parse::<u128>().unwrap();
        return (num, 1);
    }

    let int_part = parts[0].parse::<u128>().unwrap_or(0);
    let dec_part = parts[1].parse::<u128>().unwrap_or(0);
    let dec_len = parts[1].len() as u32;

    let numerator = int_part * 10u128.pow(dec_len) + dec_part;
    let denominator = 10u128.pow(dec_len);

    let gcd = gcd(numerator, denominator);
    (numerator / gcd, denominator / gcd)
}

fn gcd(a: u128, b: u128) -> u128 {
    if b == 0 { a } else { gcd(b, a % b) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderSide {
    #[serde(rename = "0")]
    Buy,
    #[serde(rename = "1")]
    Sell,
}

impl OrderSide {
    pub fn as_u8(&self) -> u8 {
        match self {
            OrderSide::Buy => 0,
            OrderSide::Sell => 1,
        }
    }

    pub fn opposite(&self) -> Self {
        match self {
            OrderSide::Buy => OrderSide::Sell,
            OrderSide::Sell => OrderSide::Buy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SignatureType {
    #[default]
    Eoa = 0,
    PolyProxy = 1,
    PolyGnosisSafe = 2,
}

impl SignatureType {
    pub fn as_u8(&self) -> u8 {
        match self {
            SignatureType::Eoa => 0,
            SignatureType::PolyProxy => 1,
            SignatureType::PolyGnosisSafe => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub salt: U256,
    pub maker: Address,
    pub signer: Address,
    pub taker: Address,
    pub token_id: U256,
    pub maker_amount: U256,
    pub taker_amount: U256,
    pub expiration: U256,
    pub nonce: U256,
    pub fee_rate_bps: U256,
    pub side: OrderSide,
    pub signature_type: SignatureType,
}

impl Order {
    pub fn new(
        maker: Address,
        signer: Address,
        token_id: U256,
        maker_amount: U256,
        taker_amount: U256,
        side: OrderSide,
    ) -> Self {
        Self {
            salt: U256::from(rand::random::<u128>()),
            maker,
            signer,
            taker: Address::ZERO,
            token_id,
            maker_amount,
            taker_amount,
            expiration: U256::ZERO,
            nonce: U256::ZERO,
            fee_rate_bps: U256::ZERO,
            side,
            signature_type: SignatureType::default(),
        }
    }

    pub fn with_expiration(mut self, expiration: u64) -> Self {
        self.expiration = U256::from(expiration);
        self
    }

    pub fn with_nonce(mut self, nonce: u64) -> Self {
        self.nonce = U256::from(nonce);
        self
    }

    pub fn with_fee_rate_bps(mut self, fee_rate_bps: u64) -> Self {
        self.fee_rate_bps = U256::from(fee_rate_bps);
        self
    }

    pub fn with_salt(mut self, salt: U256) -> Self {
        self.salt = salt;
        self
    }

    pub fn with_signature_type(mut self, signature_type: SignatureType) -> Self {
        self.signature_type = signature_type;
        self
    }
}

#[derive(Debug, Clone)]
pub struct OrderIntent {
    pub market_id: String,
    pub token_id: U256,
    pub side: OrderSide,
    pub price: f64,
    pub quantity: U256,
    pub priority: super::OrderPriority,
}

impl OrderIntent {
    pub fn new(
        market_id: impl Into<String>,
        token_id: U256,
        side: OrderSide,
        price: f64,
        quantity: U256,
    ) -> Self {
        Self {
            market_id: market_id.into(),
            token_id,
            side,
            price,
            quantity,
            priority: super::OrderPriority::default(),
        }
    }

    pub fn with_priority(mut self, priority: super::OrderPriority) -> Self {
        self.priority = priority;
        self
    }

    // price: 0.1 - 0.9 (1 decimal place)
    // maker_amount/taker_amount calculation depends on side:
    // BUY: pay USDC to receive shares -> maker_amount = quantity * price, taker_amount = quantity
    // SELL: give shares to receive USDC -> maker_amount = quantity, taker_amount = quantity * price
    //
    // Uses fraction-based arithmetic to ensure exact price matching.
    // The SDK uses Fraction to represent prices exactly, then scales amounts by k
    // where amounts = k * (price_num, price_denom). This guarantees:
    //   - BUY: maker_amount / taker_amount = price_num / price_denom
    //   - SELL: taker_amount / maker_amount = price_num / price_denom
    pub fn calculate_amounts(&self) -> (U256, U256) {
        let quantity_u128 = self.quantity.try_into().unwrap_or(u128::MAX);

        let (price_num, price_denom) = price_to_fraction(self.price);

        match self.side {
            OrderSide::Buy => {
                let k = quantity_u128.div_ceil(price_denom).max(1);
                let taker_amount = U256::from(k * price_denom);
                let maker_amount = U256::from(k * price_num);

                (maker_amount, taker_amount)
            }
            OrderSide::Sell => {
                let k = quantity_u128.div_ceil(price_denom).max(1);
                let maker_amount = U256::from(k * price_denom);
                let taker_amount = U256::from(k * price_num);

                (maker_amount, taker_amount)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    pub r: U256,
    pub s: U256,
    pub v: u8,
}

impl Signature {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(65);
        bytes.extend_from_slice(&self.r.to_be_bytes::<32>());
        bytes.extend_from_slice(&self.s.to_be_bytes::<32>());
        bytes.push(self.v);
        bytes
    }

    pub fn to_hex(&self) -> String {
        format!("0x{}", hex::encode(self.to_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_side_as_u8() {
        assert_eq!(OrderSide::Buy.as_u8(), 0);
        assert_eq!(OrderSide::Sell.as_u8(), 1);
    }

    #[test]
    fn test_order_side_opposite() {
        assert_eq!(OrderSide::Buy.opposite(), OrderSide::Sell);
        assert_eq!(OrderSide::Sell.opposite(), OrderSide::Buy);
    }

    #[test]
    fn test_signature_type_as_u8() {
        assert_eq!(SignatureType::Eoa.as_u8(), 0);
        assert_eq!(SignatureType::PolyProxy.as_u8(), 1);
        assert_eq!(SignatureType::PolyGnosisSafe.as_u8(), 2);
    }

    #[test]
    fn test_order_intent_calculate_amounts_buy() {
        let intent = OrderIntent::new(
            "test-market",
            U256::from(1),
            OrderSide::Buy,
            0.5,
            U256::from(1000),
        );

        let (maker_amount, taker_amount) = intent.calculate_amounts();
        assert_eq!(maker_amount, U256::from(500));
        assert_eq!(taker_amount, U256::from(1000));
    }

    #[test]
    fn test_order_intent_calculate_amounts_sell() {
        let intent = OrderIntent::new(
            "test-market",
            U256::from(1),
            OrderSide::Sell,
            0.5,
            U256::from(1000),
        );

        let (maker_amount, taker_amount) = intent.calculate_amounts();
        assert_eq!(maker_amount, U256::from(1000));
        assert_eq!(taker_amount, U256::from(500));
    }

    #[test]
    fn test_price_to_fraction_simple() {
        assert_eq!(price_to_fraction(0.5), (1, 2));
        assert_eq!(price_to_fraction(0.25), (1, 4));
        assert_eq!(price_to_fraction(0.75), (3, 4));
    }

    #[test]
    fn test_price_to_fraction_edge_cases() {
        assert_eq!(price_to_fraction(0.1), (1, 10));
        assert_eq!(price_to_fraction(0.9), (9, 10));
    }

    #[test]
    fn test_exact_price_matching_buy() {
        for price in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9] {
            let intent = OrderIntent::new(
                "test-market",
                U256::from(1),
                OrderSide::Buy,
                price,
                U256::from(1000000),
            );

            let (maker_amount, taker_amount) = intent.calculate_amounts();

            let maker: f64 = maker_amount.into();
            let taker: f64 = taker_amount.into();

            let calculated_price = maker / taker;

            let price_diff = (calculated_price - price).abs();
            assert!(
                price_diff < 1e-6,
                "Price mismatch: input={}, calculated={}, diff={}",
                price,
                calculated_price,
                price_diff
            );
        }
    }

    #[test]
    fn test_exact_price_matching_sell() {
        for price in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9] {
            let intent = OrderIntent::new(
                "test-market",
                U256::from(1),
                OrderSide::Sell,
                price,
                U256::from(1000000),
            );

            let (maker_amount, taker_amount) = intent.calculate_amounts();

            let maker: f64 = maker_amount.into();
            let taker: f64 = taker_amount.into();

            let calculated_price = taker / maker;

            let price_diff = (calculated_price - price).abs();
            assert!(
                price_diff < 1e-6,
                "Price mismatch: input={}, calculated={}, diff={}",
                price,
                calculated_price,
                price_diff
            );
        }
    }

    #[test]
    fn test_gcd() {
        assert_eq!(gcd(12, 18), 6);
        assert_eq!(gcd(17, 23), 1);
        assert_eq!(gcd(1000, 100), 100);
        assert_eq!(gcd(0, 5), 5);
    }

    #[test]
    fn test_signature_to_hex() {
        let sig = Signature {
            r: U256::from(1),
            s: U256::from(2),
            v: 27,
        };

        let hex = sig.to_hex();
        assert!(hex.starts_with("0x"));
        assert_eq!(hex.len(), 132); // 0x + 64 + 64 + 2
    }
}
