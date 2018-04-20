use std::{str, u8};
use std::str::FromStr;

use bigdecimal::BigDecimal;
use ethereum_models::objects::{BlockNumber, TransactionCall};
use ethereum_models::types::{H160, U256};
use fixed_hash::clean_0x;
use futures::Future;
use num::ToPrimitive;
use serde::de::DeserializeOwned;
use serde_json::Value;
use yum::{de, ser};

use super::OpSet;
use {YumFuture, YumBatchFuture};
use error::{Error, ErrorKind};

pub trait TokenOps: OpSet {
    fn token_amount_in_eth(&self, address: &H160, amount: U256) -> YumFuture<f64> {
        let astr = format!("{:?}", &address);
        let astr2 = format!("{:?}", &address);
        let tc = TransactionCall::empty()
            .to(*address)
            .data("0x313ce567")
            .done();

        let op = move |v: Value| de::<String>(v).and_then(|s| {
            u8::from_str_radix(clean_0x(&s), 16)
                .map_err(|e| {
                    ErrorKind::YumError(
                        format!(
                            "[{}] Couldn't parse decimal string value ({}) as u8: {:?}",
                            astr, &s, e
                        )
                    ).into()
                })
                .and_then(|decimals| {
                    let amt = format!("{:?}", amount);
                    BigDecimal::from_str(clean_0x(&amt))
                        .map_err(Into::into)
                        .and_then(|x| {
                            let y = x / BigDecimal::from(1.0 * 10_f64.powi(decimals as i32));
                            y.to_f64()
                                .ok_or(
                                    ErrorKind::YumError(
                                        format!(
                                            "[{}] Couldn't convert amount ({}) to BigDecimal",
                                            astr, &amt
                                        )
                                    ).into()
                                )
                        })
                })
        });

        non_compliant_tokens(&address)
            .map(|(_, _, decimals)| {
                let amt = format!("{:?}", amount);
                let res = BigDecimal::from_str(clean_0x(&amt))
                    .map_err::<Error, _>(Into::into)
                    .and_then(|x| {
                        let y = x / BigDecimal::from(1.0 * 10_f64.powi(decimals as i32));
                        y.to_f64()
                            .ok_or::<Error>(
                                ErrorKind::YumError(
                                    format!(
                                        "[{}] Couldn't convert amount ({}) to BigDecimal",
                                        astr2, &amt
                                    )
                                ).into()
                            )
                    });
                YumFuture::Now(res.unwrap())
            })
            .unwrap_or({
                let req = match self.batch_request(vec![("eth_call", vec![ser(&tc)], Box::new(op))]) {
                    YumBatchFuture::Waiting(mut futs) => futs.pop().unwrap(),
                    _ => unreachable!()
                };
                req
            })
    }

    fn _token_decimals(
        &self,
        address: &H160,
        op: Box<Fn(Value) -> Result<u8, Error> + Send + Sync>) -> YumFuture<u8>
    {
        let tc = TransactionCall::empty()
            .to(*address)
            .data("0x313ce567")
            .done();

        let req = match self.batch_request(vec![("eth_call", vec![ser(&tc)], op)]) {
            YumBatchFuture::Waiting(mut futs) => futs.pop().unwrap(),
            _ => unreachable!()
        };
        req
    }

    fn token_decimals(&self, address: &H160) -> YumFuture<u8> {
        let astr = format!("{:?}", &address);
        non_compliant_tokens(&address)
            .map(|(_, _, decimals)| YumFuture::Now(decimals))
            .unwrap_or({
                let op = move |v: Value| de::<String>(v).and_then(|s| {
                    u8::from_str_radix(clean_0x(&s), 16)
                        .map_err(|e| {
                            ErrorKind::YumError(
                                format!(
                                    "[{}] Couldn't parse decimal string value as u8: {:?}",
                                    astr, e
                                )
                            ).into()
                        })
                });
                self._token_decimals(&address, Box::new(op))
            })
    }

    fn etherdelta_get_balance(&self, token: &H160, user: &H160, block: Option<BlockNumber>)
        -> YumFuture<U256>
    {
        let block = match block {
            Some(b) => b,
            None => BlockNumber::Name("latest")
        };

        let method_id = "0xf7888aec";
        let data = format!(
            "{}000000000000000000000000{:?}000000000000000000000000{:?}", method_id, &token, &user
        );
        let tc = TransactionCall::empty()
            .to(H160::from_str("8d12a197cb00d4747a1fe03395095ce2a5cc6819").unwrap())
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
        non_compliant_tokens(&address)
            .map(|(name, _, _)| YumFuture::Now(name))
            .unwrap_or(self.contract_string_var(address, "0x06fdde03"))
    }

    fn token_symbol(&self, address: &H160) -> YumFuture<String> {
        non_compliant_tokens(&address)
            .map(|(_, symbol, _)| YumFuture::Now(symbol))
            .unwrap_or(self.contract_string_var(address, "0x95d89b41"))
    }

}

fn non_compliant_tokens(address: &H160) -> Option<(String, String, u8)> {
    match clean_0x(format!("{:?}", &address).as_str()) {
        "0000000000000000000000000000000000000000" => {
            Some(("Ether".to_string(), "ETH".to_string(), 18))
        },
        "84119cb33e8f590d75c2d6ea4e6b0741a7494eda" => {
            Some(("GigaWatt Token".to_string(), "WTT".to_string(), 0))
        },
        "ef68e7c694f40c8202821edf525de3782458639f" => {
            Some(("Loopring".to_string(), "LRC".to_string(), 18))
        },
        "e0b7927c4af23765cb51314a0e0521a9645f0e2a" => {
            Some(("DigixDAO".to_string(), "DGD".to_string(), 9))
        },
        "ff3519eeeea3e76f1f699ccce5e23ee0bdda41ac" => {
            Some(("Blockchain Capital".to_string(), "BCAP".to_string(), 0))
        },
        "5554e04e76533e1d14c52f05beef6c9d329e1e30" => {
            Some(("Autonio".to_string(), "NIO".to_string(), 0))
        },
        "b5a5f22694352c15b00323844ad545abb2b11028" => {
            Some(("ICON".to_string(), "ICX".to_string(), 18))
        },
        "c66ea802717bfb9833400264dd12c2bceaa34a6d" => {
            Some(("MakerDAO".to_string(), "MKR".to_string(), 18))
        },
        "5c543e7ae0a1104f78406c340e9c64fd9fce5170" => {
            Some(("vSlice".to_string(), "VSL".to_string(), 18))
        },
        "52903256dd18d85c2dc4a6c999907c9793ea61e3" => {
            Some(("INS Promo".to_string(), "INSP".to_string(), 0))
        },
        "d2308446536a0bad028ab8c090d62e1ea2a51f24" => {
            Some(("GNEISS Coin".to_string(), "GNEISS".to_string(), 0))
        },
        "014b50466590340d41307cc54dcee990c8d58aa8" => {
            Some(("ICOS".to_string(), "ICOS".to_string(), 6))
        },
        "8effd494eb698cc399af6231fccd39e08fd20b15" => {
            Some(("PIX Token".to_string(), "PIX".to_string(), 0))
        },
        "2df8286c9396f52e17dfee75d2e41e52609cf897" => {
            Some(("Silent Notary".to_string(), "SNTR".to_string(), 4))
        },
        "62a56a4a2ef4d355d34d10fbf837e747504d38d4" => {
            Some(("Paypex".to_string(), "PAYX".to_string(), 2))
        },
        "d51e852630debc24e9e1041a03d80a0107f8ef0c" => {
            Some(("Orium".to_string(), "ORM".to_string(), 0))
        },
        "01afc37f4f85babc47c0e2d0eababc7fb49793c8" => {
            Some(("W-GNT".to_string(), "GNTM".to_string(), 18))
        },
        "ce3d9c3f3d302436d12f18eca97a3b00e97be7cd" => {
            Some(("EPOSY".to_string(), "EPOSY".to_string(), 18))
        },
        "289fe11c6f46e28f9f1cfc72119aee92c1da50d0" => {
            Some(("EPOSN".to_string(), "EPOSN".to_string(), 18))
        },
        _ => None
    }
}