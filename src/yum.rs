use std::u64;
use std::marker::PhantomData;

use ethereum_models::types::U256;
use fixed_hash::clean_0x;
use futures::{future, Async, Poll, Future};
use futures::sync::oneshot;
use futures::future::lazy;
use rpc;
use serde::de::{DeserializeOwned, Deserializer};
use serde_json::{self, Value};

use error::{Error, ErrorKind};
use tokio::executor::current_thread;
use client::{Client, ClientResponse, YumResult};


pub struct YumClient {
    client: Client
}

impl YumClient {
    pub fn new(host: &str, connections: u32) -> Result<Self, Error> {
        Client::new(host, connections).map(|c| YumClient { client: c })
    }
}

impl YumClient {
    pub fn block_number(&self) -> YumResult<u64> {
        let x = self.client.execute_request("eth_blockNumber", vec![]);

    }
}



#[cfg(test)]
mod tests {
    use ethereum_models::types::U256;
    use futures::Future;
    use super::YumClient;
    use super::YumResult;

    #[test]
    fn gets_block_number() {
        let mut client = YumClient::new("ws://localhost:8546", 1)
            .expect("Client connection required");

        let result = client.block_number().wait();
    }
}