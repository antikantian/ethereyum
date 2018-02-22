use std::collections::VecDeque;
use std::mem;
use std::sync::Arc;

use ethereum_models::objects::Block;
use futures::{Async, Future, Poll, Stream};
use futures::future::{JoinAll, Lazy, lazy};
use futures::sync::oneshot;
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
    client: Arc<Client>
}

impl BlockStream {
    pub fn new(client: Arc<Client>, from: u64, to: u64, with_tx: bool) -> Self {
        BlockStream {
            from,
            to,
            client,
            with_tx,
            chunk_size: 10,
            has_next: true,
            queue: VecDeque::new(),
        }
    }

    fn build_queue(&self, from: u64, to: u64)
        -> VecDeque<YumFuture<Option<Block>>>
    {
        let mut queue: VecDeque<YumFuture<Option<Block>>> = VecDeque::new();
        let block_range = (from..to).collect::<Vec<u64>>().into_iter();

        for block_chunk in block_range.as_slice().chunks(10) {
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
                    self.queue.push_front(block_f);
                    return Ok(Async::NotReady)
                }
            }

            if let Some(next_queue) = self.next() {
                trace!("Have next queue");
                self.queue = next_queue;
            } else {
                trace!("Lazy stream finished");
                return Ok(Async::Ready(None));
            }
        }
    }
}