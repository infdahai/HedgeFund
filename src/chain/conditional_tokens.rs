use alloy::sol;

sol! {
    #[sol(rpc)]
    interface IConditionalTokens {
        function balanceOf(address account, uint256 id) external view returns (uint256);
        function balanceOfBatch(address[] calldata accounts, uint256[] calldata ids) external view returns (uint256[] memory);
        function mergePositions(address collateralToken, bytes32 parentCollectionId, bytes32 conditionId, uint256[] calldata partition, uint256 amount) external;
    }
}

pub use IConditionalTokens::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interface_compiles() {
        let _ = IConditionalTokens::balanceOfCall {
            account: alloy_primitives::Address::ZERO,
            id: alloy_primitives::U256::ZERO,
        };
    }
}
