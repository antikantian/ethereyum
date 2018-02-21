use std::{u64, vec};
use std::collections::{HashMap};
use std::time::{Duration, Instant};

use ethereum_models::objects::*;
use ethereum_models::types::{H160, H256, U256};
use futures::{future, AndThen, Async, Then, Poll, IntoFuture};
use serde::de::{DeserializeOwned};
use serde::ser::Serialize;
use serde_json::{self, Value};

use error::{Error, ErrorKind};
use client::{BlockStream, Client, YumBatchFuture, YumFuture};

type Op1<T> = Box<Fn(Value) -> Result<T, Error> + Send + Sync>;

fn de<T: DeserializeOwned>(v: Value) -> Result<T, Error> {
    serde_json::from_value(v).map_err(Into::into)
}

fn de_u64(v: Value) -> Result<u64, Error> {
    de::<U256>(v).map(|u256| u256.low_u64())
}

fn ser<T: Serialize>(t: &T) -> Value {
    serde_json::to_value(&t).expect("Serialize is serializable")
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogParams {
    from_block: U256,
    to_block: U256,
    address: H160,
    topics: Vec<H256>
}

pub struct YumClient {
    client: Client
}

impl YumClient {
    pub fn new(host: &str, connections: u32) -> Result<Self, Error> {
        Client::new(host, connections)
            .and_then(|c| {
                let t0 = Instant::now();
                loop {
                    if t0.elapsed().as_secs() >= 10 {
                        return Err(ErrorKind::SocketTimeout(10_u64).into());
                    }

                    if c.is_connected() {
                        break;
                    } else {
                        continue
                    }
                }
                Ok(c)
            })
            .map(|c| YumClient { client: c })
    }

    pub fn accounts(&self) -> YumFuture<Vec<H160>> {
        self.client.request("eth_accounts", Vec::new(), de::<Vec<H160>>)
    }

    pub fn address_is_contract(&self, address: &H160, block: &BlockNumber)
        -> YumFuture<bool>
    {
        self.client.request("eth_getCode", vec![ser(&address), ser(&block)], |v| {
            de::<String>(v).map(|code| code.len() > 2)
        })
    }

    pub fn block_number(&self) -> YumFuture<u64> {
        self.client.request("eth_blockNumber", Vec::new(), de_u64)
    }

    pub fn classify_addresses(&self, addresses: Vec<H160>) -> YumBatchFuture<AddressType> {
        let mut requests: Vec<(&str, Vec<Value>, Op1<AddressType>)> = Vec::new();

        for address in addresses {
            let op = Box::new(move |v: Value| {
                de::<String>(v)
                    .map(|code| {
                        if code.len() > 2 {
                            AddressType::Contract(address.clone())
                        } else {
                            AddressType::Address(address.clone())
                        }
                    })
            });
            requests.push(
                ("eth_getCode", vec![ser(&address)], op)
            );
        }
        self.client.batch_request(requests)
    }

    pub fn coinbase(&self) -> YumFuture<H160> {
        self.client.request("eth_coinbase", Vec::new(), de::<H160>)
    }

    pub fn estimate_gas(&self, call: &TransactionCall) -> YumFuture<U256> {
        self.client.request("eth_estimateGas", vec![ser(&call)], de::<U256>)
    }

    pub fn gas_price(&self) -> YumFuture<U256> {
        self.client.request("eth_gasPrice", Vec::new(), de::<U256>)
    }

    pub fn get_balance(&self, addr: &H160, num: &BlockNumber) -> YumFuture<U256> {
        self.client.request("eth_getBalance", vec![ser(&addr), ser(&num)], de::<U256>)
    }

    pub fn get_block_by_hash(&self, block: &H256, with_tx: bool) -> YumFuture<Option<Block>> {
        self.client.request(
            "eth_getBlockByHash", vec![ser(&block), ser(&with_tx)], de::<Option<Block>>
        )
    }

    pub fn get_block_by_number(&self, block: u64, with_tx: bool) -> YumFuture<Option<Block>>
    {
        self.client.request(
            "eth_getBlockByNumber",
            vec![ser(&U256::from(block)), ser(&with_tx)],
            de::<Option<Block>>
        )
    }

    pub fn _get_blocks(&self, blocks: Vec<BlockNumber>, with_tx: bool)
        -> YumBatchFuture<Option<Block>>
    {
        let mut requests: Vec<(&str, Vec<Value>, Op1<Option<Block>>)> = Vec::new();

        for block in blocks {
            let op = Box::new(|v: Value| { de::<Option<Block>>(v) });
            let b = match block {
                BlockNumber::Number(n) => serde_json::to_value(&U256::from(n)).unwrap(),
                BlockNumber::Name(n) => serde_json::to_value(&n).unwrap()
            };

            requests.push(("eth_getBlockByNumber", vec![b, ser(&with_tx)], op))
        }
        self.client.batch_request(requests)
    }

    pub fn get_blocks(&self, blocks: &[u64], with_tx: bool) -> YumBatchFuture<Option<Block>> {
        self._get_blocks(
            blocks
                .into_iter()
                .map(|b| BlockNumber::Number(*b))
                .collect::<Vec<BlockNumber>>(),
            with_tx
        )
    }

    pub fn get_block_range(&self, from: u64, to: u64, with_tx: bool)
        -> YumBatchFuture<Option<Block>>
    {
        // Why is inclusive range syntax experimental in Rust?  Shouldn't that be
        // in Rust stable?
        let blocks = (from..to + 1)
            .into_iter()
            .map(|n| BlockNumber::Number(n))
            .collect::<Vec<BlockNumber>>();

        self._get_blocks(blocks, with_tx)
    }

    pub fn get_block_stream(&self, from: u64, to: u64, with_tx: bool) -> BlockStream {
        BlockStream::new(&self, from, to, with_tx)
    }

    pub fn get_code(&self, address: &H160, block: &BlockNumber) -> YumFuture<String> {
        self.client.request("eth_getCode", vec![ser(&address), ser(&block)], de::<String>)
    }

    pub fn get_logs(&self, from: u64, to: u64, address: &H160, topic: &H256)
        -> YumFuture<Vec<Log>>
    {
        let log_params: Value = serde_json::to_value(LogParams {
            from_block: U256::from(from),
            to_block: U256::from(to),
            address: address.clone(),
            topics: vec![topic.clone()]
        }).expect("Serialization won't fail");

        self.client.request("eth_getLogs", vec![log_params], de::<Vec<Log>>)
    }

    pub fn get_logs_n_topics(&self, from: u64, to: u64, address: &H160, topics: Vec<H256>)
        -> YumBatchFuture<Vec<Log>>
    {
        let mut requests: Vec<(&str, Vec<Value>, Op1<Vec<Log>>)> = Vec::new();

        for topic in topics {
            let op = Box::new(|v: Value| de::<Vec<Log>>(v));
            let log_params: Value = serde_json::to_value(LogParams {
                from_block: U256::from(from),
                to_block: U256::from(to),
                address: address.clone(),
                topics: vec![topic.clone()]
            }).expect("Serialization won't fail");
            requests.push(
                ("eth_getLogs", vec![log_params], op)
            );
        }
        self.client.batch_request(requests)
    }

    pub fn get_num_transactions_sent(&self, address: &H160, block: &BlockNumber)
        -> YumFuture<u64>
    {
        self.client.request("eth_getTransactionCount", vec![ser(&address), ser(&block)], de_u64)
    }

    pub fn get_transaction(&self, tx_hash: &H256) -> YumFuture<Option<Transaction>> {
        self.client.request(
            "eth_getTransactionByHash", vec![ser(&tx_hash)], de::<Option<Transaction>>
        )
    }

    pub fn get_transactions(&self, txns: Vec<H256>) -> YumBatchFuture<Option<Transaction>> {
        let mut requests: Vec<(&str, Vec<Value>, Op1<Option<Transaction>>)> = Vec::new();

        for tx in txns {
            let op = Box::new(|v: Value| de::<Option<Transaction>>(v));
            requests.push(
                ("eth_getTransactionByHash", vec![ser(&tx)], op)
            );
        }
        self.client.batch_request(requests)
    }

    pub fn get_transaction_receipt(&self, tx: &H256)
        -> YumFuture<Option<TransactionReceipt>>
    {
        self.client.request(
            "eth_getTransactionReceipt", vec![ser(&tx)], de::<Option<TransactionReceipt>>
        )
    }

    pub fn get_transaction_receipts(&self, txns: &[H256])
        -> YumBatchFuture<Option<TransactionReceipt>>
    {
        let mut requests: Vec<(&str, Vec<Value>, Op1<Option<TransactionReceipt>>)> = Vec::new();
        for tx in txns {
            let op = Box::new(|v: Value| de::<Option<TransactionReceipt>>(v));
            requests.push(
                ("eth_getTransactionReceipt", vec![ser(&tx)], op)
            );
        }
        self.client.batch_request(requests)
    }

    #[cfg(feature = "parity")]
    pub fn trace_transaction(&self, tx_hash: &H256) -> YumFuture<Vec<ParityTrace>> {
        self.client.request(
            "trace_transaction", vec![ser(&tx_hash)], de::<Vec<ParityTrace>>
        )
    }

    #[cfg(feature = "parity")]
    pub fn trace_transactions(&self, txns: &[H256]) -> YumBatchFuture<Vec<ParityTrace>> {
        let mut requests: Vec<(&str, Vec<Value>, Op1<Vec<ParityTrace>>)> = Vec::new();

        for tx in txns {
            let op = Box::new(|v: Value| de::<Vec<ParityTrace>>(v));
            requests.push(
                ("trace_transaction", vec![ser(&tx)], op)
            );
        }
        self.client.batch_request(requests)
    }

}
