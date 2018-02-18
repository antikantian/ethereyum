use std::{self, io};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;

use futures::{self, Async, Poll};
use futures::future::Future;
use futures::sync::oneshot;
use serde_json::Value;

use error::{Error, ErrorKind};

enum State<A> {
    Enqueued(Option<Result<(), Error>>, oneshot::Receiver<Result<A, Error>>),
    NotReady(oneshot::Receiver<Result<A, Error>>),
    Completed
}

pub struct Response<F, A> {
    id: u64,
    state: State<A>,
    to_value: F
}

impl<F, A> Response<F, A> {
    pub fn new(
        id: u64,
        send_status: Result<(), Error>,
        rx: oneshot::Receiver<Result<A, Error>>,
        to_value: F) -> Self
    {
        Response {
            id,
            state: State::Enqueued(Some(send_status), rx),
            to_value
        }
    }
}

impl<F, A, B> Future for Response<F, A>
    where F: Fn(A) -> Result<B, Error>
{
    type Item = B;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let to_value = &self.to_value;
            match self.state {
                State::Enqueued(ref mut is_sent, _) => {
                    trace!("Request enqueued (id: {})", self.id);
                    if let Some(Err(e)) = is_sent.take() {
                        return Err(e);
                    }
                },
                State::NotReady(ref mut rx) => {
                    trace!("Waiting on response (id: {})", self.id);
                    let result = try_ready!(rx.poll()
                        .map_err(|_| Error::from(ErrorKind::Io(io::ErrorKind::TimedOut.into()))));
                    trace!("Extracting result (id: {})", self.id);
                    return result.and_then(|x| to_value(x)).map(Async::Ready);
                },
                State::Completed => {
                    return Err(ErrorKind::YumError("Couldn't complete request".into()).into())
                }
            }
            let next_state = std::mem::replace(&mut self.state, State::Completed);
            self.state = if let State::Enqueued(_, rx) = next_state {
                State::NotReady(rx)
            } else {
                next_state
            }
        }
    }

}