use std::{str, u8};
use std::str::FromStr;

use bigdecimal::BigDecimal;
use chrono::{DateTime, Timelike, TimeZone, Utc};
use ethereum_models::objects::{BlockNumber, TransactionCall};
use ethereum_models::types::{H160, U256};
use fixed_hash::clean_0x;
use futures::Future;
use num::ToPrimitive;
use reqwest;
use serde::de::{DeserializeOwned, Deserialize, Deserializer};
use serde_json::{self, Value};
use yum::{de, ser, YumClient};

use super::OpSet;
use {YumFuture, YumBatchFuture};
use error::{Error, ErrorKind};

#[derive(Debug, Deserialize)]
pub struct Candle {
    #[serde(deserialize_with = "utc_from_timestamp")]
    pub time: DateTime<Utc>,
    pub low: f64,
    pub high: f64,
    pub open: f64,
    pub close: f64,
    pub volume: f64
}

pub trait MarketOps: OpSet {
    fn get_candles(&self, start: &DateTime<Utc>, end: &DateTime<Utc>, granularity: u64) -> Result<Vec<Candle>, Error> {
        let url = format!(
            "https://api.gdax.com/products/ETH-USD/candles?start={}&end={}&granularity={}",
            &start.format("%FT%T"), &end.format("%FT%T"), &granularity
        );
        let resp = reqwest::get(&url)?.text()?;

        debug!("Gdax response: {}", &resp);
        serde_json::from_str::<Vec<Candle>>(&resp).map_err(Into::into)
    }

    fn eth_price_usd(&self, date: &DateTime<Utc>) -> Result<f64, Error> {
        let dt_start = date.with_hour(0)
            .and_then(|t| t.with_minute(0))
            .and_then(|t| t.with_second(0))
            .unwrap_or(date.clone());

        let dt_end = date.with_hour(23)
            .and_then(|t| t.with_minute(59))
            .and_then(|t| t.with_second(59))
            .unwrap_or(date.clone());

        self.get_candles(&dt_start, &dt_end, 86400).and_then(|res| {
            res.first().ok_or(ErrorKind::GdaxError.into()).map(|candle| candle.close)
        })
    }
}

pub fn utc_from_timestamp<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where D: Deserializer<'de>
{
    let s = i64::deserialize(deserializer)?;
    Ok(Utc.timestamp(s, 0))
}
