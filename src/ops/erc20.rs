use std::{str, u8};
use std::str::FromStr;

use ethereum_models::objects::{BlockNumber, TransactionCall};
use ethereum_models::types::{H160, U256};
use fixed_hash::clean_0x;
use serde_json::Value;
use yum::{de, ser};

use super::OpSet;
use YumFuture;
use error::ErrorKind;

pub trait TokenOps: OpSet {

    fn token_decimals(&self, address: &H160) -> YumFuture<u8> {
        let tc = TransactionCall::empty()
            .to(*address)
            .data("0x313ce567")
            .done();

        let op = |v: Value| de::<String>(v).and_then(|s| {
            u8::from_str_radix(clean_0x(&s), 16)
                .map_err(|_| {
                    ErrorKind::YumError("Couldn't parse decimal string value as u8".into()).into()
                })
        });

        self.request("eth_call", vec![ser(&tc)], op)
    }

    fn token_get_balance(&self, token: &H160, address: &H160, block: Option<BlockNumber>)
        -> YumFuture<U256>
    {
        let block = match block {
            Some(b) => b,
            None => BlockNumber::Name("latest")
        };
        let method_id = "0x70a08231";
        let data = format!("{}000000000000000000000000{:?}", method_id, &address);
        let tc = TransactionCall::empty()
            .to(*token)
            .data(&data)
            .done();

        let op = |v: Value| {
            de::<String>(v)
                .and_then(|s| {
                    U256::from_str(clean_0x(&s))
                        .map_err(|_| ErrorKind::YumError("Couldn't parse u256".into()).into())
                })
        };

        self.request("eth_call", vec![ser(&tc), ser(&block)], op)
    }

    fn token_name(&self, address: &H160) -> YumFuture<String> {
        self.contract_string_var(address, "0x06fdde03")
    }

    fn token_symbol(&self, address: &H160) -> YumFuture<String> {
        self.contract_string_var(address, "0x95d89b41")
    }

}