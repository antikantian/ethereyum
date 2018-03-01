use std::{str, u8};
use std::str::FromStr;
use std::sync::Arc;

use ethereum_models::objects::{Block, BlockNumber};
use ethereum_models::types::{H160, U256};
use fixed_hash::clean_0x;
use futures::sync::mpsc;
use serde::de::DeserializeOwned;
use serde_json::Value;
use yum::{de, de_u64, de_usize, ser};

use super::OpSet;
use client::{Client, Pubsub};
use error::Error;

pub trait PubsubOps: OpSet {
    fn get_client(&self) -> Arc<Client>;

    fn subscribe<T>(&self, params: Vec<Value>, op: fn(Value) -> Result<T, Error>) -> Pubsub<T>
        where T: DeserializeOwned
    {
        let (tx, rx) = mpsc::unbounded();
        let id_fut = self.request("parity_subscribe", params, de_usize);
        Pubsub::new(id_fut, self.get_client().clone(), rx, Some(tx), op)
    }

    fn new_block_numbers(&self) -> Pubsub<u64> {
        let params = vec![ser(&"eth_blockNumber".to_string()), ser::<Vec<Value>>(&vec![])];
        self.subscribe(params, de_u64)
    }

    fn new_blocks_with_tx(&self) -> Pubsub<Block> {
        let params = vec![
            ser(&"eth_getBlockByNumber".to_string()),
            ser::<Vec<Value>>(&vec![ser(&BlockNumber::Name("latest")), ser(&true)])
        ];
        self.subscribe(params, de::<Block>)
    }

}