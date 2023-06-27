use proptest::{prelude::any_with, prop_compose, proptest, strategy::Strategy};
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use sov_db::ledger_db::{LedgerDB, SlotCommit};
use sov_rollup_interface::{services::da::SlotData, stf::fuzzing::BatchReceiptStrategyArgs};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::SocketAddr;

#[cfg(test)]
use sov_rollup_interface::mocks::{TestBlock, TestBlockHeader, TestHash};

use sov_rollup_interface::stf::{BatchReceipt, Event, TransactionReceipt};
use tendermint::crypto::Sha256;
use tokio::sync::oneshot;

use crate::{config::RpcConfig, ledger_rpc};

const QUERIES_FILE: &str = "/tmp/queries.json";
const SLOTS_FILE: &str = "/tmp/slots.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestExpect {
    data: String,
    expected: String,
}

async fn query_test_helper(
    test_queries: Vec<TestExpect>,
    rpc_config: RpcConfig,
) -> anyhow::Result<()> {
    let (addr, port) = (rpc_config.bind_host, rpc_config.bind_port);
    let client = reqwest::Client::new();
    let url_str = format!("http://{addr}:{port}");

    for query in test_queries {
        let res = client
            .post(url_str.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(query.data)
            .send()
            .await
            .unwrap();

        if !res.status().is_success() {
            anyhow::bail!("Bad response: {}", res.status());
        }
        let contents = res.text().await.unwrap();
        if contents != query.expected {
            anyhow::bail!("mismatched content");
        }
        // assert_eq!((&contents), query.expected.as_str());
    }
    Ok(())
}

fn populate_ledger(ledger_db: &mut LedgerDB, slots: Vec<SlotCommit<TestBlock, u32, u32>>) {
    for slot in slots {
        ledger_db.commit_slot(slot).unwrap();
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct SerializableSlotCommit {
    slot_data: TestBlock,
    receipts: Vec<BatchReceipt<u32, u32>>,
}

impl SerializableSlotCommit {
    fn produce_slot_commit(&self) -> SlotCommit<TestBlock, u32, u32> {
        let mut slot_commit = SlotCommit::new(self.slot_data.clone());
        for r in &self.receipts {
            slot_commit.add_batch(r.clone());
        }
        slot_commit
    }
}

fn test_helper(test_queries: Vec<TestExpect>, slots: Vec<SlotCommit<TestBlock, u32, u32>>) {
    let mut to_serialize_slots = vec![];
    for s in &slots {
        let slot_data = s.slot_data().clone();
        // let serialized_slot_data = serde_json::to_string(slot_data).unwrap();
        let receipts = s.batch_receipts().clone();
        let serializable = SerializableSlotCommit {
            slot_data,
            receipts,
        };
        to_serialize_slots.push(serializable);
    }

    // Initialize the ledger database, which stores blocks, transactions, events, etc.
    let tmpdir = tempfile::tempdir().unwrap();
    let mut ledger_db = LedgerDB::with_path(tmpdir.path()).unwrap();
    populate_ledger(&mut ledger_db, slots);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap();

    rt.block_on(async {
        let (tx_start, rx_start) = oneshot::channel();
        let (tx_end, rx_end) = oneshot::channel();
        let address = SocketAddr::new("127.0.0.1".parse().unwrap(), 0);
        let ledger_rpc_module = ledger_rpc::get_ledger_rpc::<u32, u32>(ledger_db.clone());

        rt.spawn(async move {
            let server = jsonrpsee::server::ServerBuilder::default()
                .build([address].as_ref())
                .await
                .unwrap();
            let actual_address = server.local_addr().unwrap();
            let server_handle = server.start(ledger_rpc_module).unwrap();
            tx_start.send(actual_address.port()).unwrap();
            rx_end.await.unwrap();
            server_handle.stop().unwrap();
        });

        let bind_port = rx_start.await.unwrap();
        let rpc_config = RpcConfig {
            bind_host: "127.0.0.1".to_string(),
            bind_port,
        };

        if query_test_helper(test_queries.clone(), rpc_config)
            .await
            .is_err()
        {
            {
                let serialized_queries =
                    serde_json::to_string(&test_queries).expect("Failed to serialize");
                let mut file = OpenOptions::new().write(true).create(true).open(QUERIES_FILE).unwrap();
                file.write_all(serialized_queries.as_bytes()).unwrap();
            }
            {
                let serialized_slots = serde_json::to_string(&to_serialize_slots).unwrap();
                let mut file = OpenOptions::new().write(true).create(true).open(SLOTS_FILE).unwrap();
                file.write_all(serialized_slots.as_bytes()).unwrap();
            }
        };
        tx_end.send("drop server").unwrap();
    });
}

fn regular_test_helper(data: String, expected: &str) {
    let mut slots: Vec<SlotCommit<TestBlock, u32, u32>> = vec![SlotCommit::new(TestBlock {
        curr_hash: sha2::Sha256::digest(b"slot_data"),
        header: TestBlockHeader {
            prev_hash: TestHash(sha2::Sha256::digest(b"prev_header")),
        },
    })];

    let batches = vec![
        BatchReceipt {
            batch_hash: ::sha2::Sha256::digest(b"batch_receipt"),
            tx_receipts: vec![
                TransactionReceipt::<u32> {
                    tx_hash: ::sha2::Sha256::digest(b"tx1"),
                    body_to_save: Some(b"tx1 body".to_vec()),
                    events: vec![],
                    receipt: 0,
                },
                TransactionReceipt::<u32> {
                    tx_hash: ::sha2::Sha256::digest(b"tx2"),
                    body_to_save: Some(b"tx2 body".to_vec()),
                    events: vec![
                        Event::new("event1_key", "event1_value"),
                        Event::new("event2_key", "event2_value"),
                    ],
                    receipt: 1,
                },
            ],
            inner: 0,
        },
        BatchReceipt {
            batch_hash: ::sha2::Sha256::digest(b"batch_receipt2"),
            tx_receipts: vec![TransactionReceipt::<u32> {
                tx_hash: ::sha2::Sha256::digest(b"tx1"),
                body_to_save: Some(b"tx1 body".to_vec()),
                events: vec![],
                receipt: 0,
            }],
            inner: 1,
        },
    ];

    for batch in batches {
        slots.get_mut(0).unwrap().add_batch(batch)
    }

    test_helper(
        vec![TestExpect {
            data,
            expected: expected.to_string(),
        }],
        slots,
    )
}

// These tests reproduce the README workflow for the ledger_rpc, ie:
// - It creates and populate a simple ledger with a few transactions
// - It initializes the rpc server
// - It successively calls the different rpc methods registered and tests the answer
// Side note: we need to change the port for each test to avoid concurrent access issues
#[test]
fn test_get_head() {
    let data = r#"{"jsonrpc":"2.0","method":"ledger_getHead","params":[],"id":1}"#.to_string();
    let expected = r#"{"jsonrpc":"2.0","result":{"number":1,"hash":"0xd1231a38586e68d0405dc55ae6775e219f29fff1f7e0c6410d0ac069201e550b","batch_range":{"start":1,"end":3}},"id":1}"#;

    regular_test_helper(data, expected);
}

#[test]
fn test_get_transactions() {
    // Tests for different types of argument
    let data = r#"{"jsonrpc":"2.0","method":"ledger_getTransactions","params":[[{ "batch_id": 1, "offset": 0}]],"id":1}"#.to_string();
    let expected = r#"{"jsonrpc":"2.0","result":[{"hash":"0x709b55bd3da0f5a838125bd0ee20c5bfdd7caba173912d4281cae816b79a201b","event_range":{"start":1,"end":1},"body":[116,120,49,32,98,111,100,121],"custom_receipt":0}],"id":1}"#;
    regular_test_helper(data, expected);

    // Tests for flattened args
    let data =
        r#"{"jsonrpc":"2.0","method":"ledger_getTransactions","params":[1],"id":1}"#.to_string();
    regular_test_helper(data, expected);

    let data =
        r#"{"jsonrpc":"2.0","method":"ledger_getTransactions","params":[[1]],"id":1}"#.to_string();
    regular_test_helper(data, expected);

    let data =
        r#"{"jsonrpc":"2.0","method":"ledger_getTransactions","params":[[1], "Standard"],"id":1}"#
            .to_string();
    regular_test_helper(data, expected);

    let data =
        r#"{"jsonrpc":"2.0","method":"ledger_getTransactions","params":[[1], "Compact"],"id":1}"#
            .to_string();
    regular_test_helper(data, expected);

    let data =
        r#"{"jsonrpc":"2.0","method":"ledger_getTransactions","params":[[1], "Full"],"id":1}"#
            .to_string();
    regular_test_helper(data, expected);

    let data = r#"{"jsonrpc":"2.0","method":"ledger_getTransactions","params":[[{ "batch_id": 1, "offset": 1}]],"id":1}"#
            .to_string();
    let expected = r#"{"jsonrpc":"2.0","result":[{"hash":"0x27ca64c092a959c7edc525ed45e845b1de6a7590d173fd2fad9133c8a779a1e3","event_range":{"start":1,"end":3},"body":[116,120,50,32,98,111,100,121],"custom_receipt":1}],"id":1}"#;
    regular_test_helper(data, expected);
}

#[test]
fn test_get_batches() {
    let data =
        r#"{"jsonrpc":"2.0","method":"ledger_getBatches","params":[[2], "Standard"],"id":1}"#
            .to_string();
    let expected = r#"{"jsonrpc":"2.0","result":[{"hash":"0xf85fe0cb36fdaeca571c896ed476b49bb3c8eff00d935293a8967e1e9a62071e","tx_range":{"start":3,"end":4},"txs":["0x709b55bd3da0f5a838125bd0ee20c5bfdd7caba173912d4281cae816b79a201b"],"custom_receipt":1}],"id":1}"#;
    regular_test_helper(data, expected);

    let data =
        r#"{"jsonrpc":"2.0","method":"ledger_getBatches","params":[[2]],"id":1}"#.to_string();
    regular_test_helper(data, expected);

    let data = r#"{"jsonrpc":"2.0","method":"ledger_getBatches","params":[2],"id":1}"#.to_string();
    regular_test_helper(data, expected);

    let data = r#"{"jsonrpc":"2.0","method":"ledger_getBatches","params":[[1], "Compact"],"id":1}"#
        .to_string();
    let expected = r#"{"jsonrpc":"2.0","result":[{"hash":"0xb5515a80204963f7db40e98af11aedb49a394b1c7e3d8b5b7a33346b8627444f","tx_range":{"start":1,"end":3},"custom_receipt":0}],"id":1}"#;
    regular_test_helper(data, expected);

    let data = r#"{"jsonrpc":"2.0","method":"ledger_getBatches","params":[[1], "Full"],"id":1}"#
        .to_string();
    let expected = r#"{"jsonrpc":"2.0","result":[{"hash":"0xb5515a80204963f7db40e98af11aedb49a394b1c7e3d8b5b7a33346b8627444f","tx_range":{"start":1,"end":3},"txs":[{"hash":"0x709b55bd3da0f5a838125bd0ee20c5bfdd7caba173912d4281cae816b79a201b","event_range":{"start":1,"end":1},"body":[116,120,49,32,98,111,100,121],"custom_receipt":0},{"hash":"0x27ca64c092a959c7edc525ed45e845b1de6a7590d173fd2fad9133c8a779a1e3","event_range":{"start":1,"end":3},"body":[116,120,50,32,98,111,100,121],"custom_receipt":1}],"custom_receipt":0}],"id":1}"#;
    regular_test_helper(data, expected);

    let data = r#"{"jsonrpc":"2.0","method":"ledger_getBatches","params":[[0], "Compact"],"id":1}"#
        .to_string();
    let expected = r#"{"jsonrpc":"2.0","result":[null],"id":1}"#;
    regular_test_helper(data, expected);
}

#[test]
fn test_get_events() {
    let data = r#"{"jsonrpc":"2.0","method":"ledger_getEvents","params":[1],"id":1}"#.to_string();
    let expected = r#"{"jsonrpc":"2.0","result":[{"key":[101,118,101,110,116,49,95,107,101,121],"value":[101,118,101,110,116,49,95,118,97,108,117,101]}],"id":1}"#;
    regular_test_helper(data, expected);

    let data = r#"{"jsonrpc":"2.0","method":"ledger_getEvents","params":[2],"id":1}"#.to_string();
    let expected = r#"{"jsonrpc":"2.0","result":[{"key":[101,118,101,110,116,50,95,107,101,121],"value":[101,118,101,110,116,50,95,118,97,108,117,101]}],"id":1}"#;
    regular_test_helper(data, expected);

    let data = r#"{"jsonrpc":"2.0","method":"ledger_getEvents","params":[3],"id":1}"#.to_string();
    let expected = r#"{"jsonrpc":"2.0","result":[null],"id":1}"#;
    regular_test_helper(data, expected);
}

fn batch_receipt_without_hasher() -> impl Strategy<Value = BatchReceipt<u32, u32>> {
    let mut args = BatchReceiptStrategyArgs {
        hasher: None,
        ..Default::default()
    };
    args.transaction_strategy_args.hasher = None;
    any_with::<BatchReceipt<u32, u32>>(args)
}

prop_compose! {
    fn arb_batches_and_slot_hash(max_batches : usize)
    (slot_hash in proptest::array::uniform32(0_u8..), batches in proptest::collection::vec(batch_receipt_without_hasher(), 1..max_batches)) ->
     (Vec<BatchReceipt<u32, u32>>, [u8;32]){

        (batches, slot_hash)
    }
}

prop_compose! {
    fn arb_slots(max_slots : usize, max_batches: usize)
    (batches_and_hashes in proptest::collection::vec(arb_batches_and_slot_hash(max_batches), 1..max_slots)) -> (Vec<SlotCommit<TestBlock, u32, u32>>, HashMap<usize, (usize, usize)>, usize)
    {
        let mut slots = std::vec::Vec::with_capacity(max_slots);

        let mut total_num_batches = 1;

        let mut prev_hash = TestHash([0;32]);

        let mut curr_tx_id = 1;
        let mut curr_event_id = 1;

        let mut tx_id_to_event_range = HashMap::new();

        for (batches, hash) in batches_and_hashes{
            let mut new_slot = SlotCommit::new(TestBlock {
                curr_hash: hash,
                header: TestBlockHeader {
                    prev_hash,
                },
            });

            total_num_batches += batches.len();

            for batch in batches {
                for tx in &batch.tx_receipts{
                    tx_id_to_event_range.insert(curr_tx_id, (curr_event_id, curr_event_id + tx.events.len()));

                    curr_event_id += tx.events.len();
                    curr_tx_id += 1;
                }

                new_slot.add_batch(batch);
            }


            slots.push(new_slot);

            prev_hash = TestHash(hash);
        }

        (slots, tx_id_to_event_range, total_num_batches)
    }
}

fn format_tx(
    tx_id: usize,
    tx: &TransactionReceipt<u32>,
    tx_id_to_event_range: &HashMap<usize, (usize, usize)>,
) -> String {
    let (event_range_begin, event_range_end) = tx_id_to_event_range.get(&(tx_id)).unwrap();
    let encoding = hex::encode(tx.tx_hash);
    let custom_receipt = tx.receipt;
    match &tx.body_to_save {
        None => format!(
            r#"{{"hash":"0x{encoding}","event_range":{{"start":{event_range_begin},"end":{event_range_end}}},"custom_receipt":{custom_receipt}}}"#
        ),
        Some(body) => {
            let body_formatted: Vec<String> = body.iter().map(|x| x.to_string()).collect();
            let body_str = body_formatted.join(",");
            format!(
                r#"{{"hash":"0x{encoding}","event_range":{{"start":{event_range_begin},"end":{event_range_end}}},"body":[{body_str}],"custom_receipt":{custom_receipt}}}"#
            )
        }
    }
}

fn get_batches_test(
    slots: Vec<SlotCommit<TestBlock, u32, u32>>,
    tx_id_to_event_range: HashMap<usize, (usize, usize)>,
    _total_num_batches: usize,
    random_batch_num: i32,
) {
    let mut curr_batch_num = 1;
    let mut curr_tx_num = 1;

    let random_batch_num_usize = usize::try_from(random_batch_num).unwrap();

    for slot in &slots {
        if curr_batch_num > random_batch_num_usize {
            break;
        }

        if curr_batch_num + slot.batch_receipts().len() > random_batch_num_usize {
            let curr_slot_batches = slot.batch_receipts();

            let batch_index = random_batch_num_usize - curr_batch_num;

            for i in 0..batch_index {
                curr_tx_num += curr_slot_batches.get(i).unwrap().tx_receipts.len();
            }

            let first_tx_num = curr_tx_num;

            let curr_batch = curr_slot_batches.get(batch_index).unwrap();
            let last_tx_num = first_tx_num + curr_batch.tx_receipts.len();

            let batch_hash = hex::encode(curr_batch.batch_hash);
            let batch_receipt = curr_batch.inner;

            let tx_hashes: Vec<String> = curr_batch
                .tx_receipts
                .clone()
                .into_iter()
                .map(|x| {
                    let encoding = hex::encode(x.tx_hash);
                    format!("\"0x{encoding}\"")
                })
                .collect();
            let formatted_hashes = tx_hashes.join(",");

            let mut tx_full_data: Vec<String> = Vec::new();

            for (tx_id, tx) in curr_batch.tx_receipts.clone().into_iter().enumerate() {
                tx_full_data.push(format_tx(curr_tx_num + tx_id, &tx, &tx_id_to_event_range));
            }

            let tx_full_data = tx_full_data.join(",");

            test_helper(
                vec![
                    TestExpect {
                        data: format!(
                            r#"{{"jsonrpc":"2.0","method":"ledger_getBatches","params":[[{random_batch_num}], "Compact"],"id":1}}"#
                        ),
                        expected: format!(
                            r#"{{"jsonrpc":"2.0","result":[{{"hash":"0x{batch_hash}","tx_range":{{"start":{first_tx_num},"end":{last_tx_num}}},"custom_receipt":{batch_receipt}}}],"id":1}}"#
                        ),
                    },
                    TestExpect {
                        data: format!(
                            r#"{{"jsonrpc":"2.0","method":"ledger_getBatches","params":[[{random_batch_num}], "Standard"],"id":1}}"#
                        ),
                        expected: format!(
                            r#"{{"jsonrpc":"2.0","result":[{{"hash":"0x{batch_hash}","tx_range":{{"start":{first_tx_num},"end":{last_tx_num}}},"txs":[{formatted_hashes}],"custom_receipt":{batch_receipt}}}],"id":1}}"#
                        ),
                    },
                    TestExpect {
                        data: format!(
                            r#"{{"jsonrpc":"2.0","method":"ledger_getBatches","params":[[{random_batch_num}]],"id":1}}"#
                        ),
                        expected: format!(
                            r#"{{"jsonrpc":"2.0","result":[{{"hash":"0x{batch_hash}","tx_range":{{"start":{first_tx_num},"end":{last_tx_num}}},"txs":[{formatted_hashes}],"custom_receipt":{batch_receipt}}}],"id":1}}"#
                        ),
                    },
                    TestExpect {
                        data: format!(
                            r#"{{"jsonrpc":"2.0","method":"ledger_getBatches","params":[{random_batch_num}],"id":1}}"#
                        ),
                        expected: format!(
                            r#"{{"jsonrpc":"2.0","result":[{{"hash":"0x{batch_hash}","tx_range":{{"start":{first_tx_num},"end":{last_tx_num}}},"txs":[{formatted_hashes}],"custom_receipt":{batch_receipt}}}],"id":1}}"#
                        ),
                    },
                    // TODO #417: Solve this test
                    TestExpect {
                        data: format!(
                            r#"{{"jsonrpc":"2.0","method":"ledger_getBatches","params":[[{random_batch_num}], "Full"],"id":1}}"#
                        ),
                        expected: format!(
                            r#"{{"jsonrpc":"2.0","result":[{{"hash":"0x{batch_hash}","tx_range":{{"start":{first_tx_num},"end":{last_tx_num}}},"txs":[{tx_full_data}],"custom_receipt":{batch_receipt}}}],"id":1}}"#
                        ),
                    },
                ],
                slots,
            );
            return;
        }

        curr_batch_num += slot.batch_receipts().len();

        for batch in slot.batch_receipts() {
            curr_tx_num += batch.tx_receipts.len();
        }
    }

    let data = format!(
        r#"{{"jsonrpc":"2.0","method":"ledger_getBatches","params":[[{random_batch_num}], "Compact"],"id":1}}"#
    );
    let expected: String = r#"{"jsonrpc":"2.0","result":[null],"id":1}"#.to_string();
    test_helper(vec![TestExpect { data, expected }], slots);
}

proptest!(
    #[test]
    fn proptest_get_head((slots, _, total_num_batches) in arb_slots(10, 10)){
        let num_slots = slots.len();
        let last_slot = slots.last().unwrap();

        let last_slot_hash = hex::encode(last_slot.slot_data().hash());
        let last_slot_num_batches = last_slot.batch_receipts().len();

        let last_slot_start_batch = total_num_batches - last_slot_num_batches;
        let last_slot_end_batch = total_num_batches;

        let data = r#"{"jsonrpc":"2.0","method":"ledger_getHead","params":[],"id":1}"#.to_string();
        let expected = format!("{{\"jsonrpc\":\"2.0\",\"result\":{{\"number\":{num_slots},\"hash\":\"0x{last_slot_hash}\",\"batch_range\":{{\"start\":{last_slot_start_batch},\"end\":{last_slot_end_batch}}}}},\"id\":1}}");
        test_helper(vec![TestExpect{ data, expected }], slots);
    }


    #[test]
    fn proptest_get_batches((slots, tx_id_to_event_range, _total_num_batches) in arb_slots(10, 10), random_batch_num in 1..100){
        get_batches_test(slots, tx_id_to_event_range, _total_num_batches, random_batch_num);
    }

    #[test]
    fn proptest_get_transactions((slots, tx_id_to_event_range, _total_num_batches) in arb_slots(10, 10), random_tx_num in 1..1000){
        let mut curr_tx_num = 1;

        let random_tx_num_usize = usize::try_from(random_tx_num).unwrap();

        for slot in &slots{
            for batch in slot.batch_receipts(){
                if curr_tx_num > random_tx_num_usize {
                    break;
                }

                if curr_tx_num + batch.tx_receipts.len() > random_tx_num_usize {
                    let tx_index = random_tx_num_usize - curr_tx_num;
                    let tx = batch.tx_receipts.get(tx_index).unwrap();

                    let tx_formatted = format_tx(curr_tx_num + tx_index, tx, &tx_id_to_event_range);


                    test_helper(vec![TestExpect{
                        data:
                        format!(r#"{{"jsonrpc":"2.0","method":"ledger_getTransactions","params":[[{random_tx_num}]],"id":1}}"#),
                        expected:
                        format!(r#"{{"jsonrpc":"2.0","result":[{tx_formatted}],"id":1}}"#)},
                        TestExpect{
                        data:
                        format!(r#"{{"jsonrpc":"2.0","method":"ledger_getTransactions","params":[{random_tx_num}],"id":1}}"#),
                        expected:
                        format!(r#"{{"jsonrpc":"2.0","result":[{tx_formatted}],"id":1}}"#)},
                        TestExpect{
                        data:
                        format!(r#"{{"jsonrpc":"2.0","method":"ledger_getTransactions","params":[[{random_tx_num}], "Compact"],"id":1}}"#),
                        expected:
                        format!(r#"{{"jsonrpc":"2.0","result":[{tx_formatted}],"id":1}}"#)},
                        TestExpect{
                        data:
                        format!(r#"{{"jsonrpc":"2.0","method":"ledger_getTransactions","params":[[{random_tx_num}], "Standard"],"id":1}}"#),
                        expected:
                        format!(r#"{{"jsonrpc":"2.0","result":[{tx_formatted}],"id":1}}"#)},
                        TestExpect{
                        data:
                        format!(r#"{{"jsonrpc":"2.0","method":"ledger_getTransactions","params":[[{random_tx_num}], "Full"],"id":1}}"#),
                        expected:
                        format!(r#"{{"jsonrpc":"2.0","result":[{tx_formatted}],"id":1}}"#)},
                        ]
                        , slots);

                    return Ok(());
                }

                curr_tx_num += batch.tx_receipts.len();
            }
        }

        let data = format!(r#"{{"jsonrpc":"2.0","method":"ledger_getTransactions","params":[[{random_tx_num}]],"id":1}}"#);
        let expected : String = r#"{"jsonrpc":"2.0","result":[null],"id":1}"#.to_string();
        test_helper(vec![TestExpect{data, expected}], slots);

    }

    #[test]
    fn proptest_get_events((slots, tx_id_to_event_range, _total_num_batches) in arb_slots(10, 10), random_event_num in 1..10000){
        let mut curr_tx_num = 1;

        let random_event_num_usize = usize::try_from(random_event_num).unwrap();

        for slot in &slots{
            for batch in slot.batch_receipts(){
                for tx in &batch.tx_receipts{
                    let (start_event_range, end_event_range) = tx_id_to_event_range.get(&curr_tx_num).unwrap();
                    if *start_event_range > random_event_num_usize {
                        break;
                    }

                    if random_event_num_usize < *end_event_range {
                        let event_index = random_event_num_usize - *start_event_range;
                        let event : &Event = tx.events.get(event_index).unwrap();

                        let key_str_vec : Vec<String> = event.key().inner().iter().map(|x| x.to_string()).collect();
                        let key_str = key_str_vec.join(",");

                        let value_str_vec : Vec<String> = event.value().inner().iter().map(|x| x.to_string()).collect();
                        let value_str = value_str_vec.join(",");

                        let event_formatted = format!(r#"{{"key":[{key_str}],"value":[{value_str}]}}"#);


                    test_helper(vec![TestExpect{
                        data:
                        format!(r#"{{"jsonrpc":"2.0","method":"ledger_getEvents","params":[{random_event_num_usize}],"id":1}}"#),
                        expected:
                        format!(r#"{{"jsonrpc":"2.0","result":[{event_formatted}],"id":1}}"#)},
                        ]
                        , slots);

                    return Ok(());
                    }
                    curr_tx_num += 1;
                }

            }

        }

        let data = format!(r#"{{"jsonrpc":"2.0","method":"ledger_getEvents","params":[{random_event_num}],"id":1}}"#);
        let expected : String= r#"{"jsonrpc":"2.0","result":[null],"id":1}"#.to_string();
        test_helper(vec![TestExpect{data, expected}], slots);


    }
);

#[test]
// #[ignore = "WHEN DATA IS THERE"]
fn test_get_batches_from_proptest() {
    let queries: Vec<TestExpect> = {
        let mut file = File::open(QUERIES_FILE).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        serde_json::from_str(&contents).unwrap()
    };

    let deserialized_slots: Vec<SerializableSlotCommit> = {
        let mut file = File::open(SLOTS_FILE).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        serde_json::from_str(&contents).unwrap()
    };

    let slots: Vec<SlotCommit<TestBlock, u32, u32>> = deserialized_slots
        .iter()
        .map(|s| s.produce_slot_commit())
        .collect();

    test_helper(queries, slots);
}
