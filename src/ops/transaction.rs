use ethereum_models::objects::{Block, BlockNumber, Transaction, TransactionReceipt};
use ethereum_models::types::{H160, H256, U256};
use serde::de::DeserializeOwned;
use serde_json::{self, Value};
use yum::{Op1, de, de_u64, ser};

use super::OpSet;
use {YumBatchFuture, YumFuture};
use error::Error;

pub trait TransactionOps: OpSet {
    fn get_num_transactions_sent(&self, address: &H160, block: &BlockNumber)
        -> YumFuture<u64>
    {
        self.request("eth_getTransactionCount", vec![ser(&address), ser(&block)], de_u64)
    }

    fn get_transaction(&self, tx_hash: &H256) -> YumFuture<Option<Transaction>> {
        self.request(
            "eth_getTransactionByHash", vec![ser(&tx_hash)], de::<Option<Transaction>>
        )
    }

    fn get_transactions(&self, txns: Vec<H256>) -> YumBatchFuture<Option<Transaction>> {
        let mut requests: Vec<(&str, Vec<Value>, Op1<Option<Transaction>>)> = Vec::new();

        for tx in txns {
            let op = Box::new(|v: Value| de::<Option<Transaction>>(v));
            requests.push(
                ("eth_getTransactionByHash", vec![ser(&tx)], op)
            );
        }
        self.batch_request(requests)
    }

    fn get_transaction_receipt(&self, tx: &H256)
        -> YumFuture<Option<TransactionReceipt>>
    {
        self.request(
            "eth_getTransactionReceipt", vec![ser(&tx)], de::<Option<TransactionReceipt>>
        )
    }

    fn get_transaction_receipts(&self, txns: &[H256])
        -> YumBatchFuture<Option<TransactionReceipt>>
    {
        let mut requests: Vec<(&str, Vec<Value>, Op1<Option<TransactionReceipt>>)> = Vec::new();
        for tx in txns {
            let op = Box::new(|v: Value| de::<Option<TransactionReceipt>>(v));
            requests.push(
                ("eth_getTransactionReceipt", vec![ser(&tx)], op)
            );
        }
        self.batch_request(requests)
    }
}