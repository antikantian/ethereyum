use std::borrow::ToOwned;
use std::marker::PhantomData;
use std::u64;

use ethereum_models::types::U256;
use fixed_hash::clean_0x;
use futures::{Async, Future, Poll};
use serde::de::DeserializeOwned;
use serde_json::{self, Value};

use client::ClientResponse;
use error::{Error, ErrorKind};

pub enum YumResult<T> {
    Ok(T),
    Err(Error),
    NotReady(ClientResponse)
}

//impl<T: DeserializeOwned> Future for YumResult<T> {
//    type Item = T;
//    type Error = Error;
//
//    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
//        loop {
//            if let YumResult::NotReady(ref mut response) = *self {
//                let result = try_ready!(response.poll());
//                match result {
//                    Ok(v) => { return serde_json::from_value::<T>(v).map_err(Into::into).map(Async::Ready); },
//                    Err(e) => { return Err(e).map(Async::Ready); }
//                }
//            } else {
//                return Ok(self).map(Async::Ready)
//            }
//        }
//    }
//}

impl<T: DeserializeOwned + Clone> Future for YumResult<T> {
    type Item = YumResult<T>;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            if let &mut YumResult::NotReady(ref mut response) = &mut *self {
                let result = try_ready!(response.poll());

                if let Ok(value) = result {
                    match serde_json::from_value::<T>(value).map_err(Into::into) {
                        Ok(t) => return Ok(Async::Ready(YumResult::Ok(t))),
                        Err(e) => return Err(e)
                    };
                };

                if let Err(e) = result {
                    return Ok(Async::Ready(YumResult::Err(e)));
                }
            }

            if let YumResult::Ok(ref t) = *self {
                return Ok(Async::Ready(YumResult::Ok(t.clone())));
            }

            if let YumResult::Err(_) = *self {
                return Ok(Async::Ready(YumResult::Err(ErrorKind::YumError("Hmm".into()).into())));
            }
        }
    }
}


