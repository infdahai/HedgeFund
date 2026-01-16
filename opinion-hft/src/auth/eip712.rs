use alloy_primitives::{keccak256, Address, B256, U256};
use k256::ecdsa::{signature::hazmat::PrehashSigner, RecoveryId, SigningKey};

use crate::config::{
    CHAIN_ID, CONDITIONAL_TOKENS_ADDRESS, EIP712_DOMAIN_NAME, EIP712_DOMAIN_VERSION,
};
use crate::error::{Error, Result};
use crate::types::order::{Order, Signature};

use super::signer::OrderSigner;

pub struct Eip712Signer {
    domain_separator: B256,
    order_type_hash: B256,
    signing_key: SigningKey,
    address: Address,
}

impl Eip712Signer {
    pub fn new(private_key: &[u8; 32]) -> Result<Self> {
        let signing_key = SigningKey::from_bytes(private_key.into())
            .map_err(|e| Error::InvalidPrivateKey(e.to_string()))?;

        let verifying_key = signing_key.verifying_key();
        let public_key_bytes = verifying_key.to_encoded_point(false);
        let public_key_hash = keccak256(&public_key_bytes.as_bytes()[1..]);
        let address = Address::from_slice(&public_key_hash[12..]);

        let domain_separator = Self::compute_domain_separator();
        let order_type_hash = Self::compute_order_type_hash();

        Ok(Self {
            domain_separator,
            order_type_hash,
            signing_key,
            address,
        })
    }

    pub fn from_hex(private_key_hex: &str) -> Result<Self> {
        let hex_str = private_key_hex
            .strip_prefix("0x")
            .unwrap_or(private_key_hex);
        let bytes = hex::decode(hex_str)
            .map_err(|e| Error::InvalidPrivateKey(format!("invalid hex: {}", e)))?;

        if bytes.len() != 32 {
            return Err(Error::InvalidPrivateKey(format!(
                "expected 32 bytes, got {}",
                bytes.len()
            )));
        }

        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Self::new(&arr)
    }

    fn compute_domain_separator() -> B256 {
        let domain_type_hash = keccak256(
            b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
        );

        let name_hash = keccak256(EIP712_DOMAIN_NAME.as_bytes());
        let version_hash = keccak256(EIP712_DOMAIN_VERSION.as_bytes());
        let chain_id = U256::from(CHAIN_ID);
        let verifying_contract: Address = CONDITIONAL_TOKENS_ADDRESS.parse().unwrap();

        let mut encoded = Vec::with_capacity(160);
        encoded.extend_from_slice(domain_type_hash.as_slice());
        encoded.extend_from_slice(name_hash.as_slice());
        encoded.extend_from_slice(version_hash.as_slice());
        encoded.extend_from_slice(&chain_id.to_be_bytes::<32>());
        encoded.extend_from_slice(verifying_contract.into_word().as_slice());

        keccak256(&encoded)
    }

    fn compute_order_type_hash() -> B256 {
        keccak256(
            b"Order(uint256 salt,address maker,address signer,address taker,uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint256 expiration,uint256 nonce,uint256 feeRateBps,uint8 side,uint8 signatureType)"
        )
    }

    fn hash_order_struct(&self, order: &Order) -> B256 {
        let mut encoded = Vec::with_capacity(416);

        encoded.extend_from_slice(self.order_type_hash.as_slice());
        encoded.extend_from_slice(&order.salt.to_be_bytes::<32>());
        encoded.extend_from_slice(order.maker.into_word().as_slice());
        encoded.extend_from_slice(order.signer.into_word().as_slice());
        encoded.extend_from_slice(order.taker.into_word().as_slice());
        encoded.extend_from_slice(&order.token_id.to_be_bytes::<32>());
        encoded.extend_from_slice(&order.maker_amount.to_be_bytes::<32>());
        encoded.extend_from_slice(&order.taker_amount.to_be_bytes::<32>());
        encoded.extend_from_slice(&order.expiration.to_be_bytes::<32>());
        encoded.extend_from_slice(&order.nonce.to_be_bytes::<32>());
        encoded.extend_from_slice(&order.fee_rate_bps.to_be_bytes::<32>());

        let mut side_padded = [0u8; 32];
        side_padded[31] = order.side.as_u8();
        encoded.extend_from_slice(&side_padded);

        let mut sig_type_padded = [0u8; 32];
        sig_type_padded[31] = order.signature_type.as_u8();
        encoded.extend_from_slice(&sig_type_padded);

        keccak256(&encoded)
    }

    pub fn domain_separator(&self) -> B256 {
        self.domain_separator
    }

    pub fn order_type_hash(&self) -> B256 {
        self.order_type_hash
    }
}

impl OrderSigner for Eip712Signer {
    fn sign_order(&self, order: &Order) -> Result<Signature> {
        let struct_hash = self.hash_order_struct(order);

        // EIP-712: \x19\x01 ‖ domainSeparator ‖ structHash
        let mut message = Vec::with_capacity(66);
        message.extend_from_slice(&[0x19, 0x01]);
        message.extend_from_slice(self.domain_separator.as_slice());
        message.extend_from_slice(struct_hash.as_slice());

        let digest = keccak256(&message);

        let (sig, recovery_id) = self
            .signing_key
            .sign_prehash(digest.as_slice())
            .map_err(|e| Error::SigningError(e.to_string()))?;

        let r_bytes: [u8; 32] = sig.r().to_bytes().into();
        let s_bytes: [u8; 32] = sig.s().to_bytes().into();

        Ok(Signature {
            r: U256::from_be_bytes(r_bytes),
            s: U256::from_be_bytes(s_bytes),
            v: recovery_id_to_v(recovery_id),
        })
    }

    fn address(&self) -> Address {
        self.address
    }
}

fn recovery_id_to_v(recovery_id: RecoveryId) -> u8 {
    recovery_id.to_byte() + 27
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::order::OrderSide;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    fn test_signer() -> Eip712Signer {
        Eip712Signer::from_hex(TEST_PRIVATE_KEY).unwrap()
    }

    #[test]
    fn test_signer_creation() {
        let signer = test_signer();
        assert!(!signer.address().is_zero());
    }

    #[test]
    fn test_domain_separator_consistency() {
        let signer1 = test_signer();
        let signer2 = test_signer();
        assert_eq!(signer1.domain_separator(), signer2.domain_separator());
    }

    #[test]
    fn test_order_type_hash_consistency() {
        let signer1 = test_signer();
        let signer2 = test_signer();
        assert_eq!(signer1.order_type_hash(), signer2.order_type_hash());
    }

    #[test]
    fn test_sign_order() {
        let signer = test_signer();
        let order = Order::new(
            signer.address(),
            signer.address(),
            U256::from(12345),
            U256::from(1000),
            U256::from(500),
            OrderSide::Buy,
        )
        .with_salt(U256::from(1))
        .with_expiration(1700000000)
        .with_nonce(0);

        let signature = signer.sign_order(&order).unwrap();

        assert!(!signature.r.is_zero());
        assert!(!signature.s.is_zero());
        assert!(signature.v == 27 || signature.v == 28);
    }

    #[test]
    fn test_signature_deterministic() {
        let signer = test_signer();
        let order = Order::new(
            signer.address(),
            signer.address(),
            U256::from(12345),
            U256::from(1000),
            U256::from(500),
            OrderSide::Buy,
        )
        .with_salt(U256::from(42))
        .with_expiration(1700000000);

        let sig1 = signer.sign_order(&order).unwrap();
        let sig2 = signer.sign_order(&order).unwrap();

        assert_eq!(sig1.r, sig2.r);
        assert_eq!(sig1.s, sig2.s);
        assert_eq!(sig1.v, sig2.v);
    }

    #[test]
    fn test_different_orders_different_signatures() {
        let signer = test_signer();

        let order1 = Order::new(
            signer.address(),
            signer.address(),
            U256::from(12345),
            U256::from(1000),
            U256::from(500),
            OrderSide::Buy,
        )
        .with_salt(U256::from(1));

        let order2 = Order::new(
            signer.address(),
            signer.address(),
            U256::from(12345),
            U256::from(1000),
            U256::from(500),
            OrderSide::Buy,
        )
        .with_salt(U256::from(2));

        let sig1 = signer.sign_order(&order1).unwrap();
        let sig2 = signer.sign_order(&order2).unwrap();

        assert_ne!(sig1.r, sig2.r);
    }

    #[test]
    fn test_invalid_private_key_length() {
        let result = Eip712Signer::from_hex("0x1234");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_hex() {
        let result = Eip712Signer::from_hex("0xzzzz");
        assert!(result.is_err());
    }

    #[test]
    fn test_signature_to_bytes() {
        let signer = test_signer();
        let order = Order::new(
            signer.address(),
            signer.address(),
            U256::from(12345),
            U256::from(1000),
            U256::from(500),
            OrderSide::Buy,
        )
        .with_salt(U256::from(1));

        let signature = signer.sign_order(&order).unwrap();
        let bytes = signature.to_bytes();

        assert_eq!(bytes.len(), 65);
    }

    #[test]
    fn test_domain_name_has_space() {
        assert!(
            EIP712_DOMAIN_NAME.contains(' '),
            "Domain name must contain space: 'OPINION CTF Exchange'"
        );
        assert_eq!(EIP712_DOMAIN_NAME, "OPINION CTF Exchange");
    }
}
