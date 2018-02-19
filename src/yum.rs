use std::u64;
use std::collections::HashMap;

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

    pub fn address_is_contract(&self, address: &H160, block: &BlockNumber)
        -> YumResult<bool, Error>
    {
        self.client.execute_request("eth_getCode", vec![ser(&address), ser(&block)])
            .and_then(|result| {
                result.map(|v| {
                    serde_json::from_value::<String>(v)
                        .map_err(Into::into)
                        .map(|code| code.len() > 2)
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

    pub fn estimate_gas(&self, call: &TransactionCall) -> YumResult<U256, Error> {
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

    pub fn get_balance(&self, addr: &H160, num: &BlockNumber) -> YumResult<U256, Error> {
        self.client.execute_request("eth_getBalance", vec![ser(&addr), ser(&num)])
            .and_then(|result| {
                result.map(|v| {
                    serde_json::from_value::<U256>(v).map_err(Into::into)
                })
            })
    }

    pub fn get_block_by_hash(&self, block: &H256, with_tx: bool)
        -> YumResult<Option<Block>, Error>
    {
        self.client.execute_request("eth_getBlockByHash", vec![ser(&block), ser(&with_tx)])
            .and_then(|result| {
                result.map(|v| {
                    serde_json::from_value::<Option<Block>>(v).map_err(Into::into)
                })
            })
    }

    pub fn get_block_by_number(&self, block: u64, with_tx: bool)
        -> YumResult<Option<Block>, Error>
    {
        self.client.execute_request(
            "eth_getBlockByHash", vec![ser(&BlockNumber::Number(block)), ser(&with_tx)]
        )
            .and_then(|result| {
                result.map(|v| {
                    serde_json::from_value::<Option<Block>>(v).map_err(Into::into)
                })
            })
    }

    pub fn get_code(&self, address: &H160, block: &BlockNumber) -> YumResult<String, Error> {
        self.client.execute_request("eth_getCode", vec![ser(&address), ser(&block)])
            .and_then(|result| {
                result.map(|v| {
                    serde_json::from_value::<String>(v).map_err(Into::into)
                })
            })
    }

    pub fn get_logs(&self, from: &BlockNumber, to: &BlockNumber, address: &H160, topic: &H256)
        -> YumResult<Vec<Log>, Error>
    {
        let mut params = HashMap::new();
        params.insert("topics".to_string(), vec![address.clone()]);

        self.client.execute_request("eth_getLogs", vec![ser(&params)])
            .and_then(|result| {
                result.map(|v| {
                    serde_json::from_value::<Vec<Log>>(v).map_err(Into::into)
                })
            })
    }

    pub fn get_num_transactions_sent(&self, address: &H160, block: &BlockNumber)
                                     -> YumResult<u64, Error>
    {
        self.client.execute_request("eth_getTransactionCount", vec![ser(&address), ser(&block)])
            .and_then(|result| {
                result.map(|v| {
                    serde_json::from_value::<U256>(v)
                        .map_err(Into::into)
                        .map(|n| n.low_u64())
                })
            })
    }

    pub fn get_transaction(&self, tx_hash: &H256) -> YumResult<Option<Transaction>, Error> {
        self.client.execute_request("eth_getTransactionByHash", vec![ser(&tx_hash)])
            .and_then(|result| {
                result.map(|v| {
                    serde_json::from_value::<Option<Transaction>>(v).map_err(Into::into)
                })
            })
    }

    pub fn get_transaction_receipt(&self, tx_hash: &H256)
        -> YumResult<Option<TransactionReceipt>, Error>
    {
        self.client.execute_request("eth_getTransactionReceipt", vec![ser(&tx_hash)])
            .and_then(|result| {
                result.map(|v| {
                    serde_json::from_value::<Option<TransactionReceipt>>(v).map_err(Into::into)
                })
            })
    }

    #[cfg(feature = "parity")]
    pub fn trace_transaction(&self, tx_hash: &H256) -> YumResult<Vec<ParityTrace>, Error> {
        self.client.execute_request("trace_transaction", vec![ser(&tx_hash)])
            .and_then(|result| {
                result.map(|v| {
                    serde_json::from_value::<Vec<ParityTrace>>(v)
                        .map_err(Into::into)
                })
            })
    }


}

fn ser<T: Serialize>(t: &T) -> Value {
    serde_json::to_value(&t).expect("Serialize is serializable")
}
