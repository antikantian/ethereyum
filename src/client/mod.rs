mod ops;
mod result;
mod stream;
pub mod tcp;

use std::collections::VecDeque;
use std::sync::{Arc, self};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{Utc};
use fnv::FnvHashMap;
use futures::{Async, Future, IntoFuture, Sink, Stream};
use futures::sync::{mpsc, oneshot};
use futures::sync::mpsc::{SendError, TrySendError};
use futures_cpupool::CpuPool;
use hyper::header::ContentType;
use parking_lot::Mutex;
use reqwest::{Client as HttpClient, StatusCode};
use rpc::{self, Output};
use serde::de::DeserializeOwned;
use serde_json::{self, Value};
use tokio_timer::Timer;
use tungstenite::protocol::Message;

use error::{Error, ErrorKind};
pub use self::ops::BlockOps;
pub use self::result::{YumBatchFuture, YumBatchFutureT, YumFuture};
pub use self::stream::{BlockStream};

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
type Timestamps = Arc<Mutex<FnvHashMap<i64, Vec<usize>>>>;

// Using Fowler-Noll-Vo hasher because we're using integer keys and I'm a loser.
// See: http://cglab.ca/%7Eabeinges/blah/hash-rs/
// This is probably insignificant since we're not expecting THAT many concurrent requests.
pub struct Client {
    hosts: Vec<String>,
    timeout: Duration,
    worker_id: Arc<AtomicUsize>,
    sockets: Arc<Mutex<VecDeque<SocketWorker>>>,
    http: HttpClient,
    ids: Arc<AtomicUsize>,
    pending: PendingRequests,
    ts: Timestamps,
    ts_thread: Option<thread::JoinHandle<()>>,
    sentinel_thread: Option<thread::JoinHandle<()>>,
    reconnection_attempts: AtomicUsize
}

impl Client {
    pub fn new(hosts: &[&str], num_connections: u32, timeout: Duration) -> Result<Self, Error> {
        let requests = Arc::new(Mutex::new(FnvHashMap::default()));
        let timestamps = Arc::new(Mutex::new(FnvHashMap::default()));
        let mut client = Client {
            timeout,
            hosts: hosts.iter().cloned().map(|h| h.to_string()).collect(),
            worker_id: Arc::new(AtomicUsize::new(0)),
            sockets: Arc::new(Mutex::new(VecDeque::new())),
            http: HttpClient::new(),
            ids: Arc::new(AtomicUsize::new(1)),
            pending: requests,
            ts: timestamps,
            ts_thread: None,
            sentinel_thread: None,
            reconnection_attempts: AtomicUsize::new(0)
        };

        for host in hosts {
            for _ in 0..num_connections {
                client.start_connection(&host)?;
            }
        }
        client.with_timeout(10);
        client.with_sentinel();
        Ok(client)
    }

    pub fn is_connected(&self) -> bool {
        self.sockets.lock().iter().any(|worker| worker.is_connected())
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
        self.ts.lock().entry(Utc::now().timestamp()).or_insert(Vec::new()).push(id);

        let serialized_request = serde_json::to_string(&request)
            .expect("Serialization never fails");

        if let Err(_) = self.send(SocketRequest::Text(serialized_request.clone())) {
            return YumFuture::Error(ErrorKind::YumError(
                format!("Couldn't send request: {}", serialized_request)
            ).into());
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

        if let Err(_) = self.send(SocketRequest::Text(serialized_batch_calls)) {
            return YumBatchFuture::Waiting(responses
                .into_iter()
                .map(|_| YumFuture::Error(ErrorKind::YumError("Couldn't send request".into()).into()))
                .collect::<Vec<YumFuture<_>>>());
        }

        let futs = responses
            .into_iter()
            .map(|(rx, de)| YumFuture::WaitingFn(rx, de))
            .collect::<Vec<YumFuture<T>>>();

        YumBatchFuture::Waiting(futs)
    }

    fn start_connection(&self, host: &str) -> Result<(), Error> {
        trace!("Client is starting new connection");

        let worker = self.new_socket_worker(host)?;
        thread::sleep(Duration::from_millis(1000));
        info!("{}", worker.is_connected());

        self.sockets.lock().push_back(worker);
        Ok(())
    }

    fn next_id(&self) -> usize {
        self.ids.fetch_add(1, Ordering::Relaxed)
    }

    pub fn send_http(&self, msg: String) -> Result<(), Error> {
        debug!("Sending via http");
        let mut resp = self.http.post("http://127.0.0.1:8545")
            .header(ContentType::json())
            .body(msg)
            .send()?;

        let resp_text = resp.text()?;

        let decoded_msg = handle_text_message(resp_text);

        if let Ok(rpc_result) = decoded_msg {
            let mut guard = self.pending.lock();

            for response in rpc_result {
                let (id, result) = response.into_response();

                if let Some(pending_request) = guard.remove(&id) {
                    trace!("[Http] Responding to request (id: {})", id);
                    if let Err(e) = pending_request.send(result) {
                        warn!("[Http] Receiving end deallocated for response: {:?}", e);
                    }
                } else {
                    warn!("[Http] Unknown request (id: {:?}", id);
                }
            }
        } else {
            warn!("[Http] Couldn't decode response");
        }
        Ok(())
    }

    fn send(&self, msg: SocketRequest) -> Result<(), Error> {
        let worker = self.sockets.lock().pop_front();
        if let Some(w) = worker {
            match w.send_request(msg) {
                Ok(()) => {
                    debug!("Sending via websocket");
                    self.sockets.lock().push_back(w);
                    return Ok(())
                },
                Err(e) => {
                    if let SocketRequest::Text(s) = e.into_inner() {
                        return self.send_http(s)
                    }
                    return Ok(())
                }
            }
        } else {
            if let SocketRequest::Text(s) = msg {
                return self.send_http(s)
            }
        }
        Ok(())
    }

    pub fn stop(&self) -> Result<(), Error> {
        for worker in self.sockets.lock().iter_mut() {
            worker.stop()?;
        }
        Ok(())
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    fn with_sentinel(&mut self) {
        let workers = self.sockets.clone();
        let timeout = self.timeout.clone();
        let th = Some(thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(30));
                for worker in workers.lock().iter_mut() {
                    let (tx, rx) = oneshot::channel();
                    debug!("Sending ping");
                    match worker.send_request(SocketRequest::Ping(tx)) {
                        Ok(()) => {
                            if let Ok(SocketRequest::Pong) = rx.wait() {
                                debug!("Got pong");
                                continue
                            } else {
                                if let Err(e) = worker.reconnect(&timeout) {
                                    error!("Couldn't reconnect: {:?}", e);
                                } else {
                                    continue
                                }
                            }
                        },
                        Err(e) => {
                            error!("Error sending ping: {:?}", e);
                            if let Err(e) = worker.reconnect(&timeout) {
                                error!("Couldn't reconnect: {:?}", e);
                            } else {
                                continue
                            }
                        }
                    }
                }
            }
        }));
        self.sentinel_thread = th;
    }

    fn with_timeout(&mut self, timeout: u64) {
        let timestamps = self.ts.clone();
        let pending = self.pending.clone();
        let th = Some(thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(timeout));
                let now = Utc::now().timestamp();
                let mut removed = Vec::new();

                for (k, vs) in timestamps.lock().iter() {
                    if now - *k >= 10 {
                        for v in vs {
                            if let Some(f) = pending.lock().remove(v) {
                                error!("Request timed out (id: {})", v);
                                removed.push(*v as i64);
                                f.send(Err(
                                    ErrorKind::RpcError("Rpc request timed out".into()).into()
                                )).ok();
                            }
                        }
                    }
                }
                for rem in &removed {
                    timestamps.lock().remove(&rem);
                }
                removed.clear();
            }
        }));
        self.ts_thread = th;
    }

    fn new_socket_worker(&self, host: &str) -> Result<SocketWorker, Error> {
        let next_worker_id = self.worker_id.fetch_add(1, Ordering::Relaxed);
        let host = host.to_string();
        let pending = self.pending.clone();

        trace!("Client is starting new socket worker: (id: {}, host: {})", &next_worker_id, &host);
        let worker = SocketWorker::new(next_worker_id, &host, pending, &self.timeout)?;

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

pub enum SocketRequest {
    Text(String),
    Binary(Vec<u8>),
    ConnectedStatus(oneshot::Sender<bool>),
    Ping(oneshot::Sender<SocketRequest>),
    Pong,
    Stop
}

struct SocketWorker {
    id: usize,
    host: String,
    tx: mpsc::UnboundedSender<SocketRequest>,
    pending: PendingRequests,
    thread: Option<thread::JoinHandle<()>>
}

impl SocketWorker {
    pub fn new(id: usize, host: &str, pending: PendingRequests, timeout: &Duration)
        -> Result<Self, Error>
    {
        info!("Attempting to open new socket with id: {}, connecting to: {}", &id, &host);
        let (ws_tx, th) = SocketWorker::connect(id, host, pending.clone(), &timeout)?;

        Ok(SocketWorker {
            id,
            host: host.to_string(),
            tx: ws_tx,
            pending,
            thread: Some(th)
        })
    }

    pub fn is_connected(&self) -> bool {
        if self.thread.is_none() {
            debug!("Socket-{} has no thread", self.id);
            return false
        } else {
            let (tx, rx) = oneshot::channel();
            match self.send_request(SocketRequest::ConnectedStatus(tx)) {
                Err(e) => {
                    debug!("Socket-{} disconnected: {:?}", self.id, e);
                    false
                },
                Ok(_) => rx.wait().expect("Status rx")
            }
        }
    }

    pub fn reconnect(&mut self, timeout: &Duration) -> Result<(), Error> {
        debug!("Attempting to reconnect to socket-{}", self.id);
        if self.is_connected() {
            return Ok(())
        } else {
            self.thread = None;
            //self.thread.take().and_then(|th| th.join().ok());
            let (new_tx, new_thread) = SocketWorker::connect(
                self.id, &self.host, self.pending.clone(), &timeout
            )?;
            self.tx = new_tx;
            self.thread = Some(new_thread);
            let t0 = Instant::now();
            while t0.elapsed() <= *timeout {
                if self.is_connected() {
                    debug!("Reconnection of socket-{} successful", self.id);
                    return Ok(())
                }
                thread::sleep(Duration::from_millis(1));
            }
            Err(ErrorKind::ConnectError.into())
        }
    }

    pub fn send_request(&self, req: SocketRequest) -> Result<(), SendError<SocketRequest>> {
        self.tx.unbounded_send(req)
    }

    pub fn stop(&mut self) -> Result<(), Error> {
        self.send_request(SocketRequest::Stop)?;
        self.thread.take().expect("is only called once").join().expect("thread shuts down");
        Ok(())
    }

    fn connect(
        id: usize,
        host: &str,
        pending: PendingRequests,
        timeout: &Duration)
        -> Result<(mpsc::UnboundedSender<SocketRequest>, thread::JoinHandle<()>), Error>
    {
        let trace_id = format!("[socket-worker-{} -> {}]", &id, &host);
        trace!("{} starting up...", &trace_id);

        let host = host.replace("ws://", "");
        let host = host.replace("/", "");

        let (tx, rx) = mpsc::unbounded();
        let rx = rx.map_err(|_| panic!()); // Errors not possible on receiving end
        let pending = pending.clone();
        let (conn_tx, mut conn_rx) = sync::mpsc::sync_channel(0);

        let socket_thread = thread::spawn(move || {
            let client = tcp::connect_async(&host).and_then(|(ws, _)| {
                info!("Socket stream connected");
                conn_tx.send(true).unwrap();

                let (mut sink, stream) = ws.split();

                let write_stream = rx
                    .take_while(|req| {
                        if let &SocketRequest::Stop = req {
                            Ok(false)
                        } else {
                            Ok(true)
                        }
                    })
                    .filter_map(|req| {
                        match req {
                            SocketRequest::Text(s) => Some(Message::Text(s)),
                            SocketRequest::Binary(b) => Some(Message::Binary(b)),
                            SocketRequest::Ping(ping_tx) => {
                                if let Err(e) = ping_tx.send(SocketRequest::Pong) {
                                    error!("Error sending connection pong");
                                }
                                None
                            }
                            SocketRequest::Pong => None,
                            SocketRequest::ConnectedStatus(status_tx) => {
                                if let Err(e) = status_tx.send(true) {
                                    error!("Error sending connection status: {:?}", e);
                                }
                                None
                            }
                            SocketRequest::Stop => None
                        }
                    })
                    .for_each(move |msg| {
                        debug!("Sending message to socket");
                        if let Err(e) = sink.start_send(msg) {
                            error!("Error writing request to socket: {:?}", e);
                        }
                        Ok(())
                    });

                    let read_stream = stream
                        .for_each(|msg| {
                            debug!("Received message from socket");

                            let decoded_msg = handle_message(msg);

                            if let &Err(ref e) = &decoded_msg {
                                error!("{:?}", e)
                            }

                            if let Ok(rpc_result) = decoded_msg {
                                let mut guard = pending.lock();

                                for response in rpc_result {
                                    let (id, result) = response.into_response();

                                    if let Some(pending_request) = guard.remove(&id) {
                                        trace!("Responding to request (id: {})", id);
                                        if let Err(e) = pending_request.send(result) {
                                            warn!("Receiving end deallocated for response: {:?}", e);
                                        }
                                    } else {
                                        warn!("Unknown request (id: {:?}", id);
                                    }
                                }
                            } else {
                                warn!("Couldn't decode response");
                            }
                            Ok(())
                        });

                    write_stream
                        .map(|_| ())
                        .select(read_stream.map(|_| ()))
                        .then(|_| {
                            Ok(())
                        })
                });
            if let Err(e) = client.wait() {
                debug!("Socket error: {:?}", e);
            }
        });

        let t0 = Instant::now();
        while t0.elapsed() <= *timeout {
            if let Ok(true) = conn_rx.try_recv() {
                return Ok((tx, socket_thread))
            }
        }
        Err(ErrorKind::ConnectError.into())
    }
}

//impl Drop for SocketWorker {
//    fn drop(&mut self) {
//        if let Err(e) = self.tx.shutdown() {
//            warn!("Error shutting down websocket: {:?}", e);
//        }
//
//        self.thread
//            .take()
//            .expect("Thread is only dropped once")
//            .join()
//            .expect("Socket thread shuts down properly");
//    }
//}

fn handle_message(msg: Message) -> Result<Vec<RpcResponse>, Error> {
    match msg {
        Message::Text(response) => handle_text_message(response),
        Message::Binary(response) => handle_binary_message(response),
        Message::Ping(_) => unreachable!(),
        Message::Pong(_) => unreachable!()
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

