use crate::error::Result;
use crate::types::order::{Order, Signature};
use alloy_primitives::Address;

pub trait OrderSigner: Send + Sync {
    fn sign_order(&self, order: &Order) -> Result<Signature>;
    fn address(&self) -> Address;
}
