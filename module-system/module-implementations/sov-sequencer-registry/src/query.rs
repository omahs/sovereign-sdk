#![allow(missing_docs)]

use jsonrpsee::core::RpcResult;
use sov_modules_api::macros::rpc_gen;
use sov_modules_api::Context;
use sov_state::WorkingSet;

use crate::{DaAddress, SequencerRegistry};

/// The response type to the `getSequencerDddress` RPC method.
#[cfg_attr(
    feature = "native",
    derive(serde::Deserialize, serde::Serialize, Clone)
)]
#[derive(Debug, Eq, PartialEq)]
pub struct SequencerAddressResponse<C: Context> {
    /// The rollup address of the requested sequencer.
    pub address: Option<C::Address>,
}

#[rpc_gen(client, server, namespace = "sequencer")]
impl<C: Context> SequencerRegistry<C> {
    /// Returns the rollup address of the sequencer with the given DA address.
    ///
    /// The response only contains data if the sequencer is registered.
    #[rpc_method(name = "getSequencerAddress")]
    pub fn sequencer_address(
        &self,
        da_address: DaAddress,
        working_set: &mut WorkingSet<C::Storage>,
    ) -> RpcResult<SequencerAddressResponse<C>> {
        Ok(SequencerAddressResponse {
            address: self.allowed_sequencers.get(&da_address, working_set),
        })
    }
}
