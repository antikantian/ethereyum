use std::u64;

use ethereum_models::objects::*;
use ethereum_models::types::{H160, H256, U256};
use fixed_hash::clean_0x;
use futures::{future, AndThen, Async, Poll, Future};
use futures::sync::oneshot;
use futures::future::lazy;
use rpc;
use serde::de::{DeserializeOwned, Deserializer};
use serde::ser::Serialize;
use serde_json::{self, Value};

use error::{Error, ErrorKind};
use client::{Client, ClientResponse};

pub type YumResult<T, E> = AndThen<ClientResponse, Result<Result<T, E>, Error>,
    fn(Result<Value, Error>) -> Result<Result<T, E>, Error>>;

pub struct YumClient {
    client: Client
}

impl YumClient {
    pub fn new(host: &str, connections: u32) -> Result<Self, Error> {
        Client::new(host, connections).map(|c| YumClient { client: c })
    }

    pub fn accounts(&self) -> YumResult<Vec<H160>, Error> {
        self.client.execute_request("eth_accounts", Vec::new()).and_then(|result| {
            result.map(|v| {
                serde_json::from_value::<Vec<H160>>(v).map_err(Into::into)
            })
        })
    }

    pub fn block_number(&self) -> YumResult<u64, Error> {
        self.client.execute_request("eth_blockNumber", Vec::new()).and_then(|result| {
            result.map(|v| {
                serde_json::from_value::<U256>(v).map_err(Into::into)
                    .map(|u| u.low_u64())
            })
        })
    }

    pub fn coinbase(&self) -> YumResult<H160, Error> {
        self.client.execute_request("eth_coinbase", Vec::new()).and_then(|result| {
            result.map(|v| {
                serde_json::from_value::<H160>(v).map_err(Into::into)
            })
        })
    }

    pub fn estimate_gas(&self, call: TransactionCall) -> YumResult<U256, Error> {
        self.client.execute_request("eth_estimateGas", vec![ser(&call)]).and_then(|result| {
            result.map(|v| {
                serde_json::from_value::<U256>(v).map_err(Into::into)
            })
        })
    }

    pub fn gas_price(&self) -> YumResult<U256, Error> {
        self.client.execute_request("eth_gasPrice", Vec::new()).and_then(|result| {
            result.map(|v| {
                serde_json::from_value::<U256>(v).map_err(Into::into)
            })
        })
    }

    pub fn get_balance(&self, addr: H160, num: BlockNumber) -> YumResult<U256, Error> {
        self.client.execute_request("eth_getBalance", vec![ser(&addr), ser(&num)])
            .and_then(|result| {
                result.map(|v| {
                    serde_json::from_value::<U256>(v).map_err(Into::into)
                })
            })
    }

    pub fn get_block_by_hash(&self, block: H256, with_tx: bool) -> YumResult<Option<Block>, Error> {
        self.client.execute_request("eth_getBlockByHash", vec![ser(&block), ser(&with_tx)])
            .and_then(|result| {
                result.map(|v| {
                    serde_json::from_value::<Option<Block>>(v).map_err(Into::into)
                })
            })
    }

    pub fn get_block_by_number(&self, block: u64, with_tx: bool) -> YumResult<Option<Block>, Error> {
        self.client.execute_request("eth_getBlockByHash", vec![ser(&BlockNumber::Number(block)), ser(&with_tx)])
            .and_then(|result| {
                result.map(|v| {
                    serde_json::from_value::<Option<Block>>(v).map_err(Into::into)
                })
            })
    }

}

fn ser<T: Serialize>(t: &T) -> Value {
    serde_json::to_value(&t).expect("Serialize is serializable")
}