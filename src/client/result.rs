use std::collections::HashMap;
use std::marker::PhantomData;
use std::mem;
use std::sync::Arc;

use futures::{Async, Future, Poll};
use futures::future::{JoinAll, join_all};
use futures::sync::oneshot;
use serde::de::DeserializeOwned;
use serde_json::{self, Value};

use error;
use error::Error;

pub enum YumFuture<T> {
    Waiting(oneshot::Receiver<Result<Value, Error>>, fn(Value) -> Result<T, Error>),
    WaitingFn(oneshot::Receiver<Result<Value, Error>>, Arc<Box<Fn(Value) -> Result<T, Error> + Send + Sync>>),
    Complete,
    Error(Error)
}

impl<T> Future for YumFuture<T>
    where T: DeserializeOwned + Send + Sync + 'static

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
            WaitingFn(mut f, t) => {
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
                    *self = WaitingFn(f, t);
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

pub enum YumBatchFuture<T: DeserializeOwned + Send + Sync + 'static> {
    Waiting(JoinAll<Vec<YumFuture<T>>>),
    Complete,
    Error(Error)
}

impl<T> Future for YumBatchFuture<T>
    where T: DeserializeOwned + Send + Sync + 'static
{
    type Item = Vec<T>;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use client::result::YumBatchFuture::*;
        match mem::replace(self, Complete) {
            Waiting(mut jyf) => {
                if let Async::Ready(vyf) = jyf.poll()? {
                    *self = Complete;
                    return Ok(Async::Ready(vyf));
                } else {
                    *self = Waiting(jyf);
                }
            },
            Complete => {},
            Error(e) => {
                *self = Complete;
                return Err(e)
            }
        };
        Ok(Async::NotReady)
    }
}

pub enum YumBatchFutureT<T: DeserializeOwned + Send + Sync + 'static, U> {
    Waiting(YumBatchFuture<T>, Arc<Box<Fn(Vec<T>) -> U + Send + Sync>>),
    Complete,
    Error(Error)
}

impl<T, U> Future for YumBatchFutureT<T, U>
    where T: DeserializeOwned + Send + Sync + 'static,
          U: Send + Sync + 'static
{
    type Item = U;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use client::result::YumBatchFutureT::*;
        match mem::replace(self, Complete) {
            Waiting(mut ybf, op) => {
                if let Async::Ready(vt) = ybf.poll()? {
                    *self = Complete;
                    return Ok(Async::Ready(op(vt)));
                } else {
                    *self = Waiting(ybf, op);
                }
            },
            Complete => {},
            Error(e) => {
                *self = Complete;
                return Err(e)
            }
        };
        Ok(Async::NotReady)
    }
}