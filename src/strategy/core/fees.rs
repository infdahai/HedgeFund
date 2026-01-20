use super::TradeSide;

#[derive(Debug, Clone, Copy)]
pub struct FeeConfig {
    pub min_fee: f64,
    pub topic_fee_rate: f64,
    pub referral_discount: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct FeeQuote {
    pub fee_amount: f64,
    pub net_price: f64,
}

pub fn fee_rate(price: f64, config: FeeConfig) -> f64 {
    let base_fee = config.topic_fee_rate * price * (1.0 - price);
    base_fee * (1.0 - config.referral_discount)
}

pub fn fee_amount(price: f64, size: f64, config: FeeConfig) -> f64 {
    if size <= 0.0 {
        return 0.0;
    }
    let notional = price * size;
    if notional <= 0.0 {
        return 0.0;
    }
    let rate = fee_rate(price, config);
    (notional * rate).max(config.min_fee)
}

pub fn quote_fee(price: f64, size: f64, side: TradeSide, config: FeeConfig) -> FeeQuote {
    let fee = fee_amount(price, size, config);
    let net_price = if size <= 0.0 {
        price
    } else {
        let fee_per_unit = fee / size;
        match side {
            TradeSide::Buy => price + fee_per_unit,
            TradeSide::Sell => price - fee_per_unit,
        }
    };

    FeeQuote {
        fee_amount: fee,
        net_price,
    }
}
