use ethereum_models::objects::{Block, BlockNumber};
use ethereum_models::types::{H256, U256};
use serde::de::DeserializeOwned;
use serde_json::{self, Value};
use yum::{Op1, de, ser};

use {YumBatchFuture, YumFuture};
use error::Error;

pub trait BlockOps {
    fn request<T>(
        &self,
        method: &str,
        params: Vec<Value>,
        de: fn(Value) -> Result<T, Error>) -> YumFuture<T>
        where T: DeserializeOwned;

    fn batch_request<T>(&self, req: Vec<(&str, Vec<Value>, Box<Fn(Value) -> Result<T, Error> + Send + Sync>)>)
        -> YumBatchFuture<T> where T: DeserializeOwned + Send + Sync + 'static;


    fn get_block_by_hash(&self, block: &H256, with_tx: bool) -> YumFuture<Option<Block>> {
        self.request(
            "eth_getBlockByHash", vec![ser(&block), ser(&with_tx)], de::<Option<Block>>
        )
    }

    fn get_block_by_number(&self, block: u64, with_tx: bool) -> YumFuture<Option<Block>>
    {
        self.request(
            "eth_getBlockByNumber",
            vec![ser(&U256::from(block)), ser(&with_tx)],
            de::<Option<Block>>
        )
    }

    fn _get_blocks(&self, blocks: Vec<BlockNumber>, with_tx: bool)
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
        self.batch_request(requests)
    }

    fn get_blocks(&self, blocks: &[u64], with_tx: bool) -> YumBatchFuture<Option<Block>> {
        self._get_blocks(
            blocks
                .into_iter()
                .map(|b| BlockNumber::Number(*b))
                .collect::<Vec<BlockNumber>>(),
            with_tx
        )
    }

    fn get_block_range(&self, from: u64, to: u64, with_tx: bool)
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
}