use std::collections::VecDeque;
use std::mem;
use std::sync::Arc;

use ethereum_models::objects::Block;
use futures::{Async, Future, Poll, Stream};
use futures::future::{JoinAll, Lazy, lazy};
use futures::stream::FuturesUnordered;
use futures::sync::oneshot;
use parking_lot::RwLock;
use serde::de::DeserializeOwned;
use serde_json::Value;

use client::{BlockOps, Client, YumFuture, YumBatchFuture};
use futures::stream::Buffered;
use error::{Error, ErrorKind};
use yum::Op1;

pub struct BlockStream {
    from: u64,
    to: u64,
    chunk_size: u64,
    has_next: bool,
    with_tx: bool,
    queue: VecDeque<YumFuture<Option<Block>>>,
    buffer: VecDeque<YumFuture<Option<Block>>>,
    client: Arc<Client>
}

impl BlockStream {
    pub fn new(client: Arc<Client>, from: u64, to: u64, with_tx: bool) -> Self {
        BlockStream {
            from,
            to,
            client,
            with_tx,
            chunk_size: 2,
            has_next: true,
            queue: VecDeque::new(),
            buffer: VecDeque::new()
        }
    }

    fn build_queue(&self, from: u64, to: u64)
        -> VecDeque<YumFuture<Option<Block>>>
    {
        let mut queue: VecDeque<YumFuture<Option<Block>>> = VecDeque::new();
        let block_range = (from..to).collect::<Vec<u64>>().into_iter();

        for block_chunk in block_range.as_slice().chunks(self.chunk_size as usize) {
            let batch = self.get_blocks(block_chunk, self.with_tx);

            if let YumBatchFuture::Waiting(futs) = batch {
                for f in futs {
                    queue.push_back(f);
                }
            } else {
                unreachable!()
            }
        }
        queue
    }

    fn fill_buffer(&mut self, n: u64) {
        let mut count = 0;
        while self.has_next() && count < n {
            if let Some(mut queue) = self.next() {
                self.buffer.append(&mut queue);
                count += 1;
            }
        }
    }

    fn has_next(&self) -> bool {
        self.has_next
    }

    fn next(&mut self) -> Option<VecDeque<YumFuture<Option<Block>>>> {
        if !self.has_next {
            trace!("Items exhausted");
            None
        } else {
            if self.to - self.from < self.chunk_size {
                trace!("Getting final chunk: {} -> {}", self.from, self.to);
                let chunk_from = self.from;
                let chunk_to = self.to;
                self.from = chunk_to;
                self.has_next = false;
                Some(self.build_queue(chunk_from, chunk_to + 1))
            } else {
                let chunk_from = self.from;
                let chunk_to = self.from + self.chunk_size;
                trace!("Getting next chunk: {} -> {}", chunk_from, chunk_to);
                self.from = chunk_to;
                let built_queue = self.build_queue(chunk_from, chunk_to);
                trace!("Have next chunk, length: {}", &built_queue.len());
                Some(built_queue)
            }
        }
    }
}

impl BlockOps for BlockStream {
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

impl Stream for BlockStream {
    type Item = Block;
    type Error = Error;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        trace!("Polling lazy stream");
        loop {
            while let Some(mut block_f) = self.queue.pop_front() {
                if let Async::Ready(block) = block_f.poll()? {
                    trace!("Next block in lazy stream available");
                    if let Some(b) = block {
                        return Ok(Async::Ready(Some(b)));
                    } else {
                        return Ok(Async::NotReady)
                    }
                } else {
                    trace!("Item not ready, pushing back to queue, remaining: {}", self.queue.len());
                    self.queue.push_back(block_f);
                    return Ok(Async::NotReady)
                }
            }

            if self.buffer.is_empty() {
                if let Some(next_queue) = self.next() {
                    trace!("Have next queue");
                    self.queue = next_queue;
                    if self.has_next() {
                        self.fill_buffer(5);
                    } else {
                        trace!("Lazy stream finished");
                        return Ok(Async::Ready(None));
                    }
//                    if let Some(buffered_queue) = self.next() {
//                        self.buffer = buffered_queue;
//                    } else {
//                        trace!("Lazy stream finished");
//                        return Ok(Async::Ready(None));
//                    }
                } else {
                    return Ok(Async::Ready(None));
                }
            } else {
                mem::swap(&mut self.queue, &mut self.buffer);
                if self.has_next() {
                    self.fill_buffer(10);
                } else {
                    if self.queue.is_empty() && self.buffer.is_empty() {
                        return Ok(Async::Ready(None))
                    }
                }
//                if let Some(next_queue) = self.next() {
//                    self.buffer = next_queue;
//                } else {
//                    if self.queue.is_empty() && self.buffer.is_empty() {
//                        return Ok(Async::Ready(None))
//                    }
//                }
            }
        }
    }
}