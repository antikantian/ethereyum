use std::cmp;
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ethereum_models::objects::Block;
use futures::{Async, Future, Stream};
use serde::de::DeserializeOwned;
use serde_json::Value;

use client::{Client, YumFuture, YumBatchFuture};
use error::Error;
use ops::{BlockOps, OpSet};

pub struct BlockStream {
    start_block: u64,
    from: u64,
    to: u64,
    buffer_size: usize,
    batch_size: u64,
    with_tx: bool,
    queue: VecDeque<(Instant, YumFuture<Option<Block>>)>,
    completed: BTreeSet<u64>,
    should_skip: BTreeSet<u64>,
    client: Arc<Client>
}

impl BlockStream {
    pub fn new(
        client: Arc<Client>,
        from: u64,
        to: u64,
        with_tx: bool,
        skip: BTreeSet<u64>) -> Self
    {
        BlockStream {
            from,
            to,
            client,
            with_tx,
            start_block: from,
            batch_size: 10,
            buffer_size: 5,
            queue: VecDeque::new(),
            completed: BTreeSet::new(),
            should_skip: skip
        }
    }

    pub fn with_options(
        client: Arc<Client>,
        from: u64,
        to: u64,
        with_tx: bool,
        skip: BTreeSet<u64>,
        batch: u64,
        buffer: u64) -> Self
    {
        BlockStream {
            from,
            to,
            client,
            with_tx,
            start_block: from,
            batch_size: batch,
            buffer_size: buffer as usize,
            queue: VecDeque::new(),
            completed: BTreeSet::new(),
            should_skip: skip
        }
    }

    fn has_next(&self) -> bool {
        self.from < self.to || !self.queue.is_empty()
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

    fn next(&mut self) -> Option<(Instant, YumFuture<Option<Block>>)> {
        debug!("from: {}, to: {}, queue len: {}", self.from, self.to, self.queue.len());
        if self.from == self.to && self.queue.is_empty() {
            //We're done with the stream.  Check if any blocks didn't go through.
            let mut requested_blocks = (self.start_block..self.to + 1)
                .into_iter()
                .collect::<BTreeSet<u64>>();

            requested_blocks.append(&mut self.should_skip);

            let missing_blocks = requested_blocks
                .difference(&self.completed)
                .cloned()
                .collect::<Vec<u64>>();

            if missing_blocks.is_empty() {
                return None
            } else {
                debug!("Stream finished, getting missing blocks: {:?}", &missing_blocks);
                let missing_futs = self.get_blocks(&missing_blocks, self.with_tx);
                if let YumBatchFuture::Waiting(futs) = missing_futs {
                    for f in futs {
                        self.queue.push_back((Instant::now(), f));
                    }
                }
                self.queue.pop_front()
            }
        } else if self.from == self.to && !self.queue.is_empty() {
            // We're done, but there are a few more items in the queue, finish it out
            return self.queue.pop_front()
        } else if self.queue.len() > self.half_capacity() {
            // Don't start adding more futures until there is a good amount of space
            // Too little and it negates the benefits of batching requests, too much and
            // you're waiting unnecessarily for futures to complete since they weren't spawned
            return self.queue.pop_front()
        } else {
            let blocks_left = self.to - self.from;
            let get_this_many = cmp::min(blocks_left, self.remaining_capacity() as u64);

            let next_futs = {
                if self.should_skip.len() == 0 {
                    self.get_block_range(self.from, self.from + get_this_many, self.with_tx)
                } else {
                    self.get_partial_block_range(
                        self.from, self.from + get_this_many, self.with_tx, &self.should_skip
                    )
                }
            };

            if let YumBatchFuture::Waiting(futs) = next_futs {
                for f in futs {
                    self.queue.push_back((Instant::now(), f));
                }
            }
            self.from += get_this_many;
            self.queue.pop_front()
        }
    }

}

impl OpSet for BlockStream {
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

impl BlockOps for BlockStream { }

impl Stream for BlockStream {
    type Item = Block;
    type Error = Error;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        trace!("Polling lazy stream");
        while let Some((ts, mut next_item)) = self.next() {
            if let Async::Ready(block) = next_item.poll()? {
                trace!("Next block in stream available");
                if let Some(b) = block {
                    let block_num = b.number.expect("Will always have a number here");
                    self.completed.insert(block_num.low_u64());
                    return Ok(Async::Ready(Some(b)));
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