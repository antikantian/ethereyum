use std::{str, u8};
use std::str::FromStr;

use bigdecimal::BigDecimal;
use ethereum_models::objects::{BlockNumber, TransactionCall};
use ethereum_models::types::{H160, U256};
use fixed_hash::clean_0x;
use num::ToPrimitive;
use serde::de::DeserializeOwned;
use serde_json::Value;
use yum::{de, ser};

use super::OpSet;
use {YumFuture, YumBatchFuture};
use error::{Error, ErrorKind};

pub trait TokenOps: OpSet {

    fn token_amount_in_eth(&self, address: &H160, amount: U256) -> YumFuture<Option<f64>> {
        let tc = TransactionCall::empty()
            .to(*address)
            .data("0x313ce567")
            .done();

        let op = move |v: Value| de::<String>(v).and_then(|s| {
            u8::from_str_radix(clean_0x(&s), 16)
                .map_err(|e| {
                    ErrorKind::YumError(
                        format!("Couldn't parse decimal string value as u8: {:?}", e)
                    ).into()
                })
                .and_then(|decimals| {
                    let amt = format!("{:?}", amount);
                    BigDecimal::from_str(clean_0x(&amt))
                        .map(|x| {
                            let y = x / BigDecimal::from(1.0 * 10_f64.powi(decimals as i32));
                            y.with_scale(12).to_f64()
                        })
                        .map_err(|_| {
                            ErrorKind::YumError("Couldn't convert amount to BigDecimal".into()).into()
                        })
                })
        });

        let req = match self.batch_request(vec![("eth_call", vec![ser(&tc)], Box::new(op))]) {
            YumBatchFuture::Waiting(mut futs) => futs.pop().unwrap(),
            _ => unreachable!()
        };

        req
    }

    fn _token_decimals<T>(&self, address: &H160, op: fn(Value) -> Result<T, Error>) -> YumFuture<T>
        where T: DeserializeOwned
    {
        let tc = TransactionCall::empty()
            .to(*address)
            .data("0x313ce567")
            .done();

        self.request("eth_call", vec![ser(&tc)], op)
    }

    fn token_decimals(&self, address: &H160) -> YumFuture<u8> {
        let op = |v: Value| de::<String>(v).and_then(|s| {
            u8::from_str_radix(clean_0x(&s), 16)
                .map_err(|_| {
                    ErrorKind::YumError("Couldn't parse decimal string value as u8".into()).into()
                })
        });

        self._token_decimals(&address, op)
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