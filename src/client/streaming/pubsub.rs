use std::cmp;
use std::collections::{BTreeSet, VecDeque};
use std::mem;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ethereum_models::objects::Transaction;
use ethereum_models::types::H256;
use futures::{Async, Future, Stream};
use futures::sync::mpsc;
use serde::de::DeserializeOwned;
use serde_json::Value;

use client::{Client, YumFuture, YumBatchFuture};
use error::{Error, ErrorKind};
use ops::{OpSet, TransactionOps};

enum State {
    Subscribing(YumFuture<usize>),
    Subscribed
}

pub struct Pubsub<T> {
    id: usize,
    state: State,
    notifications: mpsc::UnboundedReceiver<Value>,
    tx: Option<mpsc::UnboundedSender<Value>>,
    de: fn(Value) -> Result<T, Error>,
    client: Arc<Client>,
    subscribed: bool,
    done: bool,
}

impl<T> Pubsub<T> {
    pub fn new(
        id_future: YumFuture<usize>,
        client: Arc<Client>,
        rx: mpsc::UnboundedReceiver<Value>,
        tx: Option<mpsc::UnboundedSender<Value>>,
        de: fn(Value) -> Result<T, Error>) -> Self
    {
        Pubsub {
            de,
            tx,
            client,
            id: 0,
            state: State::Subscribing(id_future),
            notifications: rx,
            subscribed: false,
            done: false
        }
    }
}

impl<T> Stream for Pubsub<T> {
    type Item = T;
    type Error = Error;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        while !self.done {
            match mem::replace(&mut self.state, State::Subscribed) {
                State::Subscribing(mut fut) => {
                    if let Async::Ready(id) = fut.poll()? {
                        debug!("Got subscription id: {}", id);
                        self.id = id;
                        let tx = self.tx.take().expect("Only take this once");
                        self.client.add_subscription(id, tx);
                        self.state = State::Subscribed;
                    } else {
                        self.state = State::Subscribing(fut);
                        return Ok(Async::NotReady)
                    }
                },
                State::Subscribed => {
                    if let Ok(Async::Ready(Some(v))) = self.notifications.poll() {
                        if let Ok(t) = (self.de)(v) {
                            return Ok(Async::Ready(Some(t)))
                        } else {
                            return Ok(Async::NotReady)
                        }
                    } else {
                        return Ok(Async::NotReady)
                    }
                }
            }
        }
        Ok(Async::Ready(None))
    }
}