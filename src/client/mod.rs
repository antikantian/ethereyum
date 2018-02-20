mod result;

use std::collections::{HashMap, VecDeque};
use std::{self, io};
use std::sync::{self, Arc};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;
use std::convert::From;

use fnv::FnvHashMap;
use futures::{Async, Poll, future, Future};
use futures::future::{JoinAll, join_all};
use futures::sync::oneshot;
use parking_lot::Mutex;
use rpc::{self, Output};
use serde::de::DeserializeOwned;
use serde_json::{self, Value};
use ws;

use error::{Error, ErrorKind};
pub use self::result::{YumBatchFuture, YumBatchFutureT, YumFuture};

#[derive(Debug)]
pub enum RpcResponse {
    Success { id: usize, result: Value },
    Failure { id: usize, result: Error }
}

impl RpcResponse {
    pub fn into_response(self) -> (usize, Result<Value, Error>) {
        match self {
            RpcResponse::Success { id, result } => (id, Ok(result)),
            RpcResponse::Failure { id, result } => (id, Err(result))
        }
    }
}

type PendingRequests = Arc<Mutex<FnvHashMap<usize, oneshot::Sender<Result<Value, Error>>>>>;

// Using Fowler-Noll-Vo hasher because we're using integer keys and I'm a loser.
// See: http://cglab.ca/%7Eabeinges/blah/hash-rs/
// This is probably insignificant since we're not expecting THAT many concurrent requests
// I've benchmarked this client at ~140k req/s... how many requests do you think
// will be in this hashmap before they get removed on response?  Yeah, clearly should
// have went with std::collections::ThatHashMapThatMightBeFivePicoSecondsSlower
pub struct Client {
    host: String,
    worker_id: Arc<AtomicUsize>,
    sockets: Arc<Mutex<VecDeque<SocketWorker>>>,
    ids: Arc<AtomicUsize>,
    pending: PendingRequests
}

impl Client {
    pub fn new(host: &str, num_connections: u32) -> Result<Self, Error> {
        let requests = Arc::new(Mutex::new(FnvHashMap::default()));

        let mut client = Client {
            host: host.to_string(),
            worker_id: Arc::new(AtomicUsize::new(0)),
            sockets: Arc::new(Mutex::new(VecDeque::new())),
            ids: Arc::new(AtomicUsize::new(0)),
            pending: requests
        };

        for _ in 0..num_connections {
            client.start_connection()?;
        }

        Ok(client)
    }

    pub fn request<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Vec<Value>,
        de: fn(Value) -> Result<T, Error>) -> YumFuture<T>
    {
        let (tx, rx) = oneshot::channel();

        let id = self.next_id();
        let request = build_request(id, &method, params);

        self.pending.lock().insert(id, tx);

        let serialized_request = serde_json::to_string(&request)
            .expect("Serialization never fails");

        trace!("Writing request to socket: {:?}", &request);

        if let Err(_) = self.send(ws::Message::Text(serialized_request)) {
            return YumFuture::Error(ErrorKind::YumError("Couldn't send request".into()).into());
        }
        YumFuture::Waiting(rx, de)
    }

    pub fn batch_request<T: DeserializeOwned + Send + Sync + 'static>(
        &self,
        requests: Vec<(&str, Vec<Value>, Box<Fn(Value) -> Result<T, Error> + Send + Sync>)>)
        -> YumBatchFuture<T>
    {
        let mut guard = self.pending.lock();
        let mut calls = Vec::new();
        let mut responses = Vec::new();

        for (method, params, de) in requests {
            let (tx, rx) = oneshot::channel();
            let id = self.next_id();
            let request = build_request(id, &method, params);

            calls.push(request);
            guard.insert(id, tx);
            responses.push((rx, Arc::new(de)));
        }
        guard.unlock_fair();

        let batch_call = rpc::request::Request::Batch(calls);
        let serialized_batch_calls = serde_json::to_string(&batch_call)
            .expect("Serialization never fails");

        trace!("Writing batch request to socket: {:?}", &serialized_batch_calls);

        if let Err(_) = self.send(ws::Message::Text(serialized_batch_calls)) {
            return YumBatchFuture::Waiting(join_all(responses
                .into_iter()
                .map(|_| YumFuture::Error(ErrorKind::YumError("Couldn't send request".into()).into()))
                .collect::<Vec<YumFuture<_>>>()));
        }

        let futs = responses
            .into_iter()
            .map(|(rx, de)| YumFuture::WaitingFn(rx, de))
            .collect::<Vec<YumFuture<T>>>();

        YumBatchFuture::Waiting(join_all(futs))
    }

    fn start_connection(&mut self) -> Result<(), Error> {
        trace!("Client is starting new connection");

        let worker = self.new_socket_worker()?;

        loop {
            if worker.is_connected() {
                break
            } else {
                continue
            }
        }

        self.sockets.lock().push_back(worker);
        Ok(())
    }

    fn next_id(&self) -> usize {
        self.ids.fetch_add(1, Ordering::Relaxed)
    }

    fn send(&self, msg: ws::Message) -> Result<(), Error> {
        let worker = self.sockets.lock().pop_front()
            .ok_or::<Error>(ErrorKind::YumError("No sockets available".into()).into())?;

        let _: () = worker.send_request(msg)?;

        self.sockets.lock().push_back(worker);
        Ok(())
    }

    fn new_socket_worker(&self) -> Result<SocketWorker, Error> {
        let next_worker_id = self.worker_id.fetch_add(1, Ordering::AcqRel);
        let host = self.host.to_string();
        let pending = self.pending.clone();

        trace!("Client is starting new socket worker: (id: {}, host: {})", &next_worker_id, &host);
        let worker = SocketWorker::new(next_worker_id, &host, pending)?;

        Ok(worker)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        trace!("Dropping client");
        self.sockets.lock().clear();
        self.pending.lock().clear();
    }
}

pub enum WorkerRequest {
    Message(ws::Message),
    Close(ws::CloseCode),
    Shutdown
}

struct SocketWorker {
    tx: ws::Sender,
    is_closed: Arc<AtomicBool>,
    is_shutdown: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>
}

impl SocketWorker {
    pub fn new(id: usize, host: &str, pending: PendingRequests) -> Result<Self, Error> {
        trace!("Attempting to start new socket with id: {}, connecting to: {}", &id, &host);
        let (ws_tx, closed, shutdown, ws_thread) = SocketWorker::start(id, host, pending)?;

        Ok(SocketWorker {
            tx: ws_tx,
            is_closed: closed,
            is_shutdown: shutdown,
            thread: Some(ws_thread)
        })
    }

    pub fn is_connected(&self) -> bool {
        !self.is_closed.load(Ordering::Relaxed)
    }

    pub fn send_request(&self, req: ws::Message) -> Result<(), Error> {
        self.tx.send(req).map_err(Into::into)
    }

    pub fn stop(&self) -> Result<(), Error> {
        if let Err(e) = self.tx.close(ws::CloseCode::Normal) {
            error!("Couldn't close websocket: {:?}", e);
        }
        if let Err(e) = self.tx.shutdown() {
            error!("Couldn't shutdown websocket: {:?}", e);
        }
        Ok(())
    }

    fn start(id: usize, host: &str, pending: PendingRequests)
             -> Result<(
                 ws::Sender, Arc<AtomicBool>, Arc<AtomicBool>, thread::JoinHandle<()>
             ), Error>
    {
        let trace_id = format!("[socket-worker-{} -> {}]", &id, &host);
        trace!("{} starting up...", &trace_id);

        let host = host.to_string();

        let closed_status = Arc::new(AtomicBool::new(true));
        let shutdown_status = Arc::new(AtomicBool::new(true));

        let (socket_sender, socket_thread) = SocketWorker::get_connection(
            &host, pending.clone(), closed_status.clone(), shutdown_status.clone()
        )?;

        Ok((socket_sender, closed_status, shutdown_status, socket_thread))
    }

    fn get_connection(
        host: &str,
        pending: PendingRequests,
        closed_status: Arc<AtomicBool>,
        shutdown_status: Arc<AtomicBool>) -> Result<(ws::Sender, thread::JoinHandle<()>), Error>
    {
        trace!("connecting to host: {:?}", &host);

        let (tx, rx) = sync::mpsc::channel();
        let h = host.to_string();

        let c_status = closed_status.clone();

        let ws_thread = thread::Builder::new().spawn(move || {
            trace!("entering actual websocket connection thread");
            ws::connect(h.clone(), |out| {
                trace!("in ws-rs connection closure");
                // get write handle out of closure
                tx.send(out).unwrap();

                SocketConnection {
                    is_closed: closed_status.clone(),
                    is_shutdown: shutdown_status.clone(),
                    pending: pending.clone()
                }
            }).expect(&format!("Couldn't connect to {}", &h));
        }).expect("Threads needs to spawn");

        let mut iter_count = 0;

        loop {
            iter_count += 1;

            // 5 secs
            if iter_count > 50 {
                return Err(
                    ErrorKind::YumError(format!("Couldn't connect to host: {}", &host)).into()
                );
            }

            if c_status.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(100));
                continue
            } else {
                trace!("Socket connection open");
                break
            }
        }

        let socket_tx = rx.recv().expect("Couldn't get ws socket handle");
        Ok((socket_tx, ws_thread))
    }


}

impl Drop for SocketWorker {
    fn drop(&mut self) {
        if let Err(e) = self.tx.shutdown() {
            warn!("Error shutting down websocket: {:?}", e);
        }

        self.thread
            .take()
            .expect("Thread is only dropped once")
            .join()
            .expect("Socket thread shuts down properly");
    }
}

struct SocketConnection {
    is_closed: Arc<AtomicBool>,
    is_shutdown: Arc<AtomicBool>,
    pending: PendingRequests
}

impl ws::Handler for SocketConnection {
    fn on_open(&mut self, _: ws::Handshake) -> Result<(), ws::Error> {
        self.is_shutdown.store(false, Ordering::Relaxed);
        self.is_closed.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn on_close(&mut self, _: ws::CloseCode, _: &str) {
        self.is_closed.store(true, Ordering::Relaxed);
    }

    fn on_shutdown(&mut self) {
        self.is_shutdown.store(true, Ordering::Relaxed);
    }

    fn on_message(&mut self, msg: ws::Message) -> Result<(), ws::Error> {
        trace!("Received message: {}", &msg);

        let decoded_msg = match msg.clone() {
            ws::Message::Text(resp) => handle_text_message(resp),
            ws::Message::Binary(resp) => handle_binary_message(resp),
            _ => Err(ErrorKind::RpcError("Invalid socket response".into()).into()),
        };

        if let Ok(rpc_result) = decoded_msg {
            let mut guard = self.pending.lock();

            for response in rpc_result {
                let (id, result) = response.into_response();

                if let Some(pending_request) = guard.remove(&id) {
                    trace!("Responding to (id: {}) with {:?}", id, result);
                    if let Err(e) = pending_request.send(result) {
                        warn!("Receiving end deallocated for response: {:?}", e);
                    }
                } else {
                    warn!("Unknown request (id: {:?}", id);
                }
            }
        } else {
            warn!("Couldn't decode response: {:?}", msg);
        }
        Ok(())
    }
}

enum MessageState {
    InFlight(Option<Result<(), Error>>, oneshot::Receiver<Result<Value, Error>>),
    Waiting(oneshot::Receiver<Result<Value, Error>>),
    Complete
}

pub struct ClientResponse {
    id: usize,
    state: MessageState
}

impl ClientResponse {
    pub fn new(
        id: usize,
        send_status: Result<(), Error>,
        rx: oneshot::Receiver<Result<Value, Error>>) -> Self
    {
        ClientResponse {
            id,
            state: MessageState::InFlight(Some(send_status), rx)
        }
    }
}

// This is based on code from the rust-web3 library:
// https://github.com/tomusdrw/rust-web3/blob/master/src/transports/shared.rs
impl Future for ClientResponse {
    type Item = Result<Value, Error>;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match self.state {
                MessageState::InFlight(ref mut is_sent, _) => {
                    trace!("Request enqueued (id: {})", self.id);
                    if let Some(Err(e)) = is_sent.take() {
                        return Err(e);
                    }
                },
                MessageState::Waiting(ref mut rx) => {
                    trace!("Waiting on response (id: {})", self.id);
                    let result = try_ready!(rx.poll()
                        .map_err(|_| Error::from(ErrorKind::Io(io::ErrorKind::TimedOut.into()))));
                    trace!("Got result (id: {})", self.id);
                    return Ok(result).map(Async::Ready)

                }
                MessageState::Complete => {
                    return Err(ErrorKind::YumError("Couldn't complete request".into()).into())
                }
            }
            let next_state = std::mem::replace(&mut self.state, MessageState::Complete);
            self.state = if let MessageState::InFlight(_, rx) = next_state {
                MessageState::Waiting(rx)
            } else {
                next_state
            }
        }
    }
}

fn call_to_message(rpc_call: rpc::Call) -> ws::Message {
    let call = serde_json::to_string(&rpc_call)
        .expect("Serialization of a call should never fail");
    ws::Message::Text(call)
}

fn handle_message(msg: ws::Message) -> Result<Vec<RpcResponse>, Error> {
    match msg {
        ws::Message::Text(response) => handle_text_message(response),
        ws::Message::Binary(response) => handle_binary_message(response)
    }
}

fn handle_text_message(text: String) -> Result<Vec<RpcResponse>, Error> {
    serde_json::from_str::<rpc::Response>(&text)
        .map_err(Into::into)
        .and_then(|resp| handle_response(resp))
}

fn handle_binary_message(binary: Vec<u8>) -> Result<Vec<RpcResponse>, Error> {
    serde_json::from_slice::<rpc::Response>(&binary)
        .map_err(Into::into)
        .and_then(|resp| handle_response(resp))
}

fn handle_response(response: rpc::Response) -> Result<Vec<RpcResponse>, Error> {
    match response {
        rpc::Response::Single(output) => {
            let rpc_response = handle_output(output)?;
            Ok(vec![rpc_response])
        },
        rpc::Response::Batch(vec_output) => {
            handle_batch_output(vec_output)
                .into_iter()
                .collect::<Result<Vec<RpcResponse>, Error>>()
        }
    }
}

fn handle_batch_output(output: Vec<Output>) -> Vec<Result<RpcResponse, Error>> {
    output.into_iter().map(|out| handle_output(out)).collect::<Vec<Result<RpcResponse, Error>>>()
}

fn handle_output(output: rpc::Output) -> Result<RpcResponse, Error> {
    match output {
        Output::Success(success) => handle_success(success),
        Output::Failure(failure) => handle_failure(failure)
    }
}

fn handle_success(success: rpc::Success) -> Result<RpcResponse, Error> {
    if let rpc::Id::Num(response_id) = success.id {
        Ok(RpcResponse::Success {
            id: response_id as usize,
            result: success.result
        })
    } else {
        let err_message = format!("invalid correlation id, expected int, got: {:?}", success.id);
        Err(ErrorKind::RpcError(err_message).into())
    }
}

fn handle_failure(failure: rpc::Failure) -> Result<RpcResponse, Error> {
    if let rpc::Id::Num(response_id) = failure.id {
        Ok(RpcResponse::Failure {
            id: response_id as usize,
            result: ErrorKind::YumError(format!("{:?}", failure.error)).into()
        })
    } else {
        let err_message = format!("invalid correlation id, expected int, got: {:?}", failure.id);
        Err(ErrorKind::RpcError(err_message).into())
    }
}

pub fn build_request(id: usize, method: &str, params: Vec<Value>) -> rpc::Call {
    rpc::Call::MethodCall(rpc::MethodCall {
        jsonrpc: Some(rpc::Version::V2),
        method: method.to_string(),
        params: Some(rpc::Params::Array(params)),
        id: rpc::Id::Num(id as u64)
    })
}

