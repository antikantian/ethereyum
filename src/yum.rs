use std::{u64, vec};
use std::collections::HashMap;

use ethereum_models::objects::*;
use ethereum_models::types::{H160, H256, U256};
use futures::{future, AndThen, Async, Then, Poll, IntoFuture};
use serde::de::{DeserializeOwned};
use serde::ser::Serialize;
use serde_json::{self, Value};

use error::{Error, ErrorKind};
use client::{Client, YumBatchFuture, YumFuture};

type Op1<T> = Box<Fn(Value) -> Result<T, Error>>;

fn de<T: DeserializeOwned>(v: Value) -> Result<T, Error> {
    serde_json::from_value(v).map_err(Into::into)
}

fn de_u64(v: Value) -> Result<u64, Error> {
    de::<U256>(v).map(|u256| u256.low_u64())
}

fn ser<T: Serialize>(t: &T) -> Value {
    serde_json::to_value(&t).expect("Serialize is serializable")
}

pub struct YumClient {
    client: Client
}

impl YumClient {
    pub fn new(host: &str, connections: u32) -> Result<Self, Error> {
        Client::new(host, connections).map(|c| YumClient { client: c })
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
            "eth_getBlockByHash",
            vec![ser(&BlockNumber::Number(block)), ser(&with_tx)],
            de::<Option<Block>>
        )
    }

    pub fn get_code(&self, address: &H160, block: &BlockNumber) -> YumFuture<String> {
        self.client.request("eth_getCode", vec![ser(&address), ser(&block)], de::<String>)
    }

    pub fn get_logs(&self, from: &BlockNumber, to: &BlockNumber, address: &H160, topic: &H256)
        -> YumFuture<Vec<Log>>
    {
        let mut params = HashMap::new();
        params.insert("topics".to_string(), vec![address.clone()]);

        self.client.request("eth_getLogs", vec![ser(&params)], de::<Vec<Log>>)
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

}


