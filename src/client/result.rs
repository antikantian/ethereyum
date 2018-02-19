use std::collections::HashMap;
use std::marker::PhantomData;
use std::mem;

use futures::{Async, Future, Poll};
use futures::sync::oneshot;
use serde::de::DeserializeOwned;
use serde_json::{self, Value};

use error::Error;

pub enum YumFuture<T> {
    Waiting(oneshot::Receiver<Result<Value, Error>>, fn(Value) -> Result<T, Error>),
    Complete,
    Error(Error)
}


pub struct YumResult<F, T> {
    inner: T,
    f: F
}

pub struct YumBatchResult<T> {
    inner: Vec<T>
}

impl<T> Future for YumFuture<T>
    where T: DeserializeOwned

{
    type Item = T;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use client::result::YumFuture::*;
        match mem::replace(self, Complete) {
            Waiting(mut f, t) => {
                if let Async::Ready(rv) = f.poll()? {
                    match rv.and_then(|v| t(v)) {
                        Ok(ret_t) => {
                            *self = Complete;
                            return Ok(Async::Ready(ret_t));
                        },
                        Err(e) => {
                            *self = Complete;
                            return Err(e);
                        }
                    }
                } else {
                    *self = Waiting(f, t);
                }
            },
            Complete => {},
            Error(e) => {
                *self = Complete;
                return Err(e);
            }
        };
        Ok(Async::NotReady)
    }
}

