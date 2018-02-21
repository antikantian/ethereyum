use std::collections::VecDeque;
use std::mem;

use ethereum_models::objects::Block;
use futures::{Async, Future, Poll, Stream};
use futures::future::JoinAll;
use futures::sync::oneshot;

use client::{Client, YumFuture, YumBatchFuture};
use error::{Error, ErrorKind};
use yum::YumClient;

pub enum BlockStream {
    Processing(VecDeque<YumFuture<Option<Block>>>),
    Error(Error),
    Done
}

impl BlockStream {
    pub fn new(client: &YumClient, from: u64, to: u64, with_tx: bool) -> Self {
        let mut queue: VecDeque<YumFuture<Option<Block>>> = VecDeque::new();
        let block_range = (from..to + 1).collect::<Vec<u64>>().into_iter();

        trace!("Building block stream");

        for block_chunk in block_range.as_slice().chunks(10) {
            let batch = client.get_blocks(block_chunk, with_tx);

            if let YumBatchFuture::Waiting(futs) = batch {
                for f in futs {
                    queue.push_back(f);
                }
            }
        }
        BlockStream::Processing(queue)
    }
}

impl Stream for BlockStream {
    type Item = Block;
    type Error = Error;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        trace!("Polling stream");
        use self::BlockStream::*;

        match mem::replace(self, Done) {
            Processing(mut block_queue) => {
                if let Some(mut block_f) = block_queue.pop_front() {
                    if let Async::Ready(block) = block_f.poll()? {
                        trace!("Next block in stream available");
                        *self = Processing(block_queue);
                        if let Some(b) = block {
                            return Ok(Async::Ready(Some(b)));
                        }
                    } else {
                        block_queue.push_front(block_f);
                        *self = Processing(block_queue);
                    }
                } else {
                    return Ok(Async::Ready(None));
                }
            },
            Error(e) => {
                *self = Done;
                return Err(e);
            },
            Done => {}
        };
        Ok(Async::NotReady)
    }
}