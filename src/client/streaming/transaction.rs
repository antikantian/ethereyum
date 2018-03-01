use std::cmp;
use std::collections::{BTreeSet, VecDeque};
use std::mem;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ethereum_models::objects::Transaction;
use ethereum_models::types::H256;
use futures::{Async, Future, Stream};
use serde::de::DeserializeOwned;
use serde_json::Value;

use client::{Client, YumFuture, YumBatchFuture};
use error::{Error, ErrorKind};
use ops::{OpSet, TransactionOps};

pub struct TransactionStream {
    tx_list: VecDeque<H256>,
    buffer_size: usize,
    batch_size: u64,
    queue: VecDeque<(Instant, YumFuture<Option<Transaction>>)>,
    client: Arc<Client>
}

impl TransactionStream {
    pub fn new(client: Arc<Client>, txns: VecDeque<H256>) -> Self {
        TransactionStream {
            client,
            tx_list: txns,
            batch_size: 10,
            buffer_size: 5,
            queue: VecDeque::new()
        }
    }

    fn has_next(&self) -> bool {
        !self.tx_list.is_empty()
    }

    fn queue_capacity(&self) -> usize {
        self.buffer_size * self.batch_size as usize
    }

    fn half_capacity(&self) -> usize {
        self.queue_capacity() / 2
    }

    fn remaining_capacity(&self) -> usize {
        self.queue_capacity() - self.queue.len() - 1
    }

    fn is_full(&self) -> bool {
        self.queue_capacity() == self.queue.len()
    }

    fn next(&mut self) -> Option<(Instant, YumFuture<Option<Transaction>>)> {
        if self.tx_list.is_empty() && self.queue.is_empty() {
            return None
        } else if self.tx_list.is_empty() && !self.queue.is_empty() {
            // We're done, but there are a few more items in the queue, finish it out
            return self.queue.pop_front()
        } else if self.queue.len() > self.half_capacity() {
            // Don't start adding more futures until there is a good amount of space
            // Too little and it negates the benefits of batching requests, too much and
            // you're waiting unnecessarily for futures to complete since they weren't spawned
            return self.queue.pop_front()
        } else {
            let txns_left = self.tx_list.len();
            let get_this_many = cmp::min(txns_left, self.remaining_capacity());
            let mut this_batch = Vec::new();

            for _ in 0..get_this_many {
                if let Some(tx) = self.tx_list.pop_front() {
                    this_batch.push(tx);
                }
            }

            let next_futs = self.get_transactions(this_batch);
            if let YumBatchFuture::Waiting(futs) = next_futs {
                for f in futs {
                    self.queue.push_back((Instant::now(), f));
                }
            }
            self.queue.pop_front()
        }
    }

}

impl OpSet for TransactionStream {
    fn request<T>(
        &self,
        method: &str,
        params: Vec<Value>,
        de: fn(Value) -> Result<T, Error>) -> YumFuture<T>
        where T: DeserializeOwned
    {
        self.client.request(method, params, de)
    }

    fn batch_request<T>(
        &self,
        req: Vec<(&str, Vec<Value>, Box<Fn(Value) -> Result<T, Error> + Send + Sync>)>)
        -> YumBatchFuture<T> where T: DeserializeOwned + Send + Sync + 'static
    {
        self.client.batch_request(req)
    }
}

impl TransactionOps for TransactionStream { }

impl Stream for TransactionStream {
    type Item = Transaction;
    type Error = Error;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        trace!("Polling lazy stream");
        while let Some((ts, mut next_item)) = self.next() {
            if let Async::Ready(transaction) = next_item.poll()? {
                trace!("Next block in stream available");
                if let Some(tx) = transaction {
                    return Ok(Async::Ready(Some(tx)));
                } else {
                    // if we're here, there was probably a problem decoding the block.
                    return Ok(Async::NotReady)
                }
            } else {
                // The future isn't done yet, push it back onto the queue and move on.
                if ts.elapsed() < Duration::from_secs(5) {
                    self.queue.push_back((ts, next_item));
                }
                return Ok(Async::NotReady)
            }
        }
        Ok(Async::Ready(None))
    }
}