use reth_primitives::{
    sign_message, Address, Bytes as RethBytes, Signature, Transaction as RethTransaction,
    TransactionKind, TransactionSigned, TxEip1559 as RethTxEip1559, H256,
};
use revm::primitives::{KECCAK_EMPTY, U256};
use sov_modules_api::default_context::DefaultContext;
use sov_modules_api::default_signature::private_key::DefaultPrivateKey;
use sov_modules_api::{Context, Module, PrivateKey, Spec};
use sov_state::{ProverStorage, WorkingSet};

use crate::call::CallMessage;
use crate::dev_signer::DevSigner;
use crate::evm::test_helpers::SimpleStorageContract;
use crate::evm::transaction::EvmTransaction;
use crate::evm::EthAddress;
use crate::{AccountData, Evm, EvmConfig};

type C = DefaultContext;

// ETEHRES
//pub fn secret_key_to_address(secret_key: &SigningKey) -> Address {

fn create_messages(contract_addr: EthAddress, set_arg: u32) -> Vec<CallMessage> {
    let ds: DevSigner = todo!();

    let mut transactions = Vec::default();
    let contract = SimpleStorageContract::new();

    // Contract creation.
    {
        let signed_tx = ds
            .sign_default_transaction(TransactionKind::Create, contract.byte_code().to_vec(), 0)
            .unwrap();

        transactions.push(CallMessage {
            tx: signed_tx.try_into().unwrap(),
        });
    }

    // Update contract state.
    {
        let signed_tx = ds
            .sign_default_transaction(
                TransactionKind::Call(contract_addr.into()),
                hex::decode(hex::encode(&contract.set_call_data(set_arg))).unwrap(),
                1,
            )
            .unwrap();

        transactions.push(CallMessage {
            tx: signed_tx.try_into().unwrap(),
        });
    }

    transactions
}

#[test]
fn evm_test() {
    use sov_modules_api::PublicKey;
    let tmpdir = tempfile::tempdir().unwrap();
    let working_set = &mut WorkingSet::new(ProverStorage::with_path(tmpdir.path()).unwrap());

    let priv_key = DefaultPrivateKey::generate();

    let sender = priv_key.pub_key();
    let sender_addr = sender.to_address::<<C as Spec>::Address>();
    let sender_context = C::new(sender_addr);
    let caller = [0; 20];

    let evm = Evm::<C>::default();

    let data = AccountData {
        address: caller,
        balance: U256::from(1000000000).to_le_bytes(),
        code_hash: KECCAK_EMPTY.to_fixed_bytes(),
        code: vec![],
        nonce: 0,
    };

    let config = EvmConfig { data: vec![data] };

    evm.genesis(&config, working_set).unwrap();

    let contract_addr = hex::decode("bd770416a3345f91e4b34576cb804a576fa48eb1")
        .unwrap()
        .try_into()
        .unwrap();

    let set_arg = 999;

    for tx in create_messages(contract_addr, set_arg) {
        evm.call(tx, &sender_context, working_set).unwrap();
    }

    let db_account = evm.accounts.get(&contract_addr, working_set).unwrap();
    let storage_key = &[0; 32];
    let storage_value = db_account.storage.get(storage_key, working_set).unwrap();

    assert_eq!(set_arg.to_le_bytes(), storage_value[0..4])
}
