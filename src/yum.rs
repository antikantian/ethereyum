use std::marker::PhantomData;

use futures::{future, Async, Poll, Future};
use futures::sync::oneshot;
use futures::future::lazy;
use rpc;
use serde::de::{DeserializeOwned, Deserializer};
use serde_json::{self, Value};

use error::{Error, ErrorKind};
use tokio::executor::current_thread;
use client::Client;

pub fn build_request(id: u64, method: &str, params: Vec<Value>) -> rpc::Call {
    rpc::Call::MethodCall(rpc::MethodCall {
        jsonrpc: Some(rpc::Version::V2),
        method: method.to_string(),
        params: Some(rpc::Params::Array(params)),
        id: rpc::Id::Num(id)
    })
}

pub struct YumClient {
    underlying: Client
}

//impl YumClient {
//    pub fn block_number(&self) {
//        self.underlying.execute_request("eth_blockNumber", vec![])
//            .map(|result| {
//                result.map(|)
//            })
//
//
//
//
//    }
//
//
//
//}