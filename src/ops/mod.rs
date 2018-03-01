mod block;
mod erc20;
mod transaction;

pub use self::block::BlockOps;
pub use self::erc20::TokenOps;
pub use self::transaction::TransactionOps;

use std::str;

use error::{Error, ErrorKind};
use ethereum_models::objects::TransactionCall;
use ethereum_models::types::H160;
use fixed_hash::clean_0x;
use rustc_serialize::hex::FromHex;
use serde::de::DeserializeOwned;
use serde_json::Value;
use yum::{de, ser};
use {YumFuture, YumBatchFuture};

fn contract_call_var(address: &H160, data: &str) -> TransactionCall {
    TransactionCall::empty()
        .to(*address)
        .data(data)
        .done()
}

pub trait OpSet {
    fn request<T>(
        &self,
        method: &str,
        params: Vec<Value>,
        de: fn(Value) -> Result<T, Error>) -> YumFuture<T>
        where T: DeserializeOwned;

    fn batch_request<T>(
        &self,
        req: Vec<(&str, Vec<Value>, Box<Fn(Value) -> Result<T, Error> + Send + Sync>)>)
        -> YumBatchFuture<T> where T: DeserializeOwned + Send + Sync + 'static;

    fn contract_string_var(&self, address: &H160, method: &str) -> YumFuture<String> {
        let tc = contract_call_var(address, method);
        let op = |v: Value| {
            de::<String>(v)
                .and_then(|s| {
                    clean_0x(&s)
                        .as_bytes()
                        .chunks(64)
                        .last()
                        .map(|l| unsafe { str::from_utf8_unchecked(&l) })
                        .map(|l| l.replace("0", " ").trim_right().replace(" ", "0"))
                        .and_then(|r| r.from_hex().ok())
                        .ok_or(ErrorKind::YumError("Couldn't parse string".into()).into())
                        .and_then(|r| {
                            str::from_utf8(&r)
                                .map(|t| t.to_string())
                                .map_err(|_| {
                                    ErrorKind::YumError("Couldn't parse string".into()).into()
                                })
                        })
                })
        };
        self.request("eth_call", vec![ser(&tc)], op)
    }

}