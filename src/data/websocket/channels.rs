pub const MARKET_DATA: &str = "market.data";
pub const MARKET_TRADE: &str = "market.trade";
pub const TRADE_ORDER_UPDATE: &str = "trade.order.update";
pub const TRADE_RECORD_NEW: &str = "trade.record.new";

pub fn market_data_channel(pair: &str) -> String {
    format!("{}.{}", MARKET_DATA, pair)
}

pub fn market_trade_channel(pair: &str) -> String {
    format!("{}.{}", MARKET_TRADE, pair)
}
