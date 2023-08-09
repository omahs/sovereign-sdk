use reth_primitives::{
    Signature as RethSignature, TransactionSigned as RethTransactionSigned,
    TransactionSignedEcRecovered as RethTransactionSignedEcRecovered,
    TransactionSignedNoHash as RethTransactionSignedNoHash,
};

use super::{Bytes32, EthAddress};

#[derive(borsh::BorshDeserialize, borsh::BorshSerialize, Debug, PartialEq, Clone)]
pub(crate) struct BlockEnv {
    pub(crate) number: u64,
    pub(crate) coinbase: EthAddress,
    pub(crate) timestamp: Bytes32,
    /// Prevrandao is used after Paris (aka TheMerge) instead of the difficulty value.
    pub(crate) prevrandao: Option<Bytes32>,
    /// basefee is added in EIP1559 London upgrade
    pub(crate) basefee: Bytes32,
    pub(crate) gas_limit: Bytes32,
}

impl Default for BlockEnv {
    fn default() -> Self {
        Self {
            number: Default::default(),
            coinbase: Default::default(),
            timestamp: Default::default(),
            prevrandao: Some(Default::default()),
            basefee: Default::default(),
            gas_limit: [u8::MAX; 32],
        }
    }
}

#[cfg_attr(
    feature = "native",
    derive(serde::Serialize),
    derive(serde::Deserialize),
    derive(schemars::JsonSchema)
)]
#[derive(borsh::BorshDeserialize, borsh::BorshSerialize, Debug, PartialEq, Clone)]
pub struct AccessListItem {
    pub address: EthAddress,
    pub storage_keys: Vec<Bytes32>,
}

#[cfg_attr(
    feature = "native",
    derive(serde::Serialize),
    derive(serde::Deserialize),
    derive(schemars::JsonSchema)
)]
#[derive(borsh::BorshDeserialize, borsh::BorshSerialize, Debug, PartialEq, Clone)]
pub struct EvmTransaction {
    //    pub sender: EthAddress,
    pub data: Vec<u8>,
    pub gas_limit: u64,
    pub gas_price: u128,
    pub max_priority_fee_per_gas: u128,
    pub max_fee_per_gas: u128,
    pub to: Option<EthAddress>,
    pub value: u128,
    pub nonce: u64,
    pub access_lists: Vec<AccessListItem>,
    pub chain_id: u64,
    pub sig: Signature,
}

#[cfg_attr(
    feature = "native",
    derive(serde::Serialize),
    derive(serde::Deserialize),
    derive(schemars::JsonSchema)
)]
#[derive(borsh::BorshDeserialize, borsh::BorshSerialize, Debug, PartialEq, Clone)]
pub struct EvmTransactionWithSender {
    pub sender: EthAddress,
    pub transaction: EvmTransaction,

    pub hash: Bytes32,
}

#[cfg_attr(
    feature = "native",
    derive(serde::Serialize),
    derive(serde::Deserialize),
    derive(schemars::JsonSchema)
)]
#[derive(borsh::BorshDeserialize, borsh::BorshSerialize, Debug, PartialEq, Clone)]
pub struct Signature {
    /// The R field of the signature; the point on the curve.
    pub r: [u8; 32],
    /// The S field of the signature; the point on the curve.
    pub s: [u8; 32],
    /// yParity: Signature Y parity; formally Ty
    pub odd_y_parity: bool,
}

#[cfg_attr(
    feature = "native",
    derive(serde::Serialize),
    derive(serde::Deserialize),
    derive(schemars::JsonSchema)
)]
#[derive(borsh::BorshDeserialize, borsh::BorshSerialize, Debug, PartialEq, Clone)]
pub struct RawEvmTransaction {
    pub tx: Vec<u8>,
}

pub struct EvmTransactionSignedEcRecovered {
    pub tx: RethTransactionSignedEcRecovered,
}
