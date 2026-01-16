pub mod market;
pub mod order;
pub mod position;

pub use market::Market;
pub use order::{Order, OrderIntent, OrderSide, SignatureType};
pub use position::Position;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum OrderPriority {
    Low = 1,
    #[default]
    Medium = 2,
    High = 3,
    Critical = 4,
}
