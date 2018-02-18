use std::collections::VecDeque;
use std::{self, io};
use std::marker::PhantomData;
use std::sync::{self, Arc};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;
use std::convert::From;

use futures::future::IntoFuture;
use futures_cpupool::CpuPool;
use futures::future::Executor;
use fnv::FnvHashMap;
use futures::{Async, Poll, Future, Stream, Sink};
use futures::sync::{oneshot, mpsc};
use parking_lot::Mutex;
use rpc::{self, Output};
use serde::ser::Serialize;
use serde_json::{self, Value};
use ws;
use futures::future::lazy;
use tokio::executor::current_thread;

use error::{Error, ErrorKind};

#[derive(Debug)]
pub enum RpcResponse {
    Success { id: u64, result: Value },
    Failure { id: u64, result: Error }
}

impl RpcResponse {
    pub fn into_response(self) -> (u64, Result<Value, Error>) {
        match self {
            RpcResponse::Success { id, result } => (id, Ok(result)),
            RpcResponse::Failure { id, result } => (id, Err(result))
        }
    }
}

type PendingRequests = Arc<Mutex<FnvHashMap<u64, oneshot::Sender<Result<Value, Error>>>>>;



// Using Fowler-Noll-Vo hasher because we're using integer keys and I'm a loser.
// See: http://cglab.ca/%7Eabeinges/blah/hash-rs/
// This is probably insignificant since we're not expecting THAT many concurrent requests
// I've benchmarked this client at ~140k req/s... how many requests do you think
// will be in this hashmap before they get removed on response?  Yeah, clearly should
// have went with std::collections::ThatHashMapThatMightBeFivePicoSecondsSlower
pub struct Client {
    host: String,
    worker_id: Arc<AtomicU64>,
    sockets: Arc<Mutex<VecDeque<SocketWorker>>>,
    ids: Arc<AtomicU64>,
    pending: PendingRequests
}

impl Client {
    pub fn new(host: &str, num_connections: u32) -> Result<Self, Error> {
        let requests = Arc::new(Mutex::new(FnvHashMap::default()));

        let mut client = Client {
            host: host.to_string(),
            worker_id: Arc::new(AtomicU64::new(0)),
            sockets: Arc::new(Mutex::new(VecDeque::new())),
            ids: Arc::new(AtomicU64::new(0)),
            pending: requests
        };

        for _ in 0..num_connections {
            client.start_connection()?;
        }

        Ok(client)
    }

    pub fn shutdown_all(&self) -> Result<(), Error> {
        let mut guard = self.sockets.lock();
        guard
            .iter_mut()
            .fold(Ok(()), |acc, sock| {
                acc.and_then(|_| sock.stop())
            })
    }

    pub fn execute_request(&self, method: &str, params: Vec<Value>) -> ClientResponse {
        let (tx, rx) = oneshot::channel();

        let id = self.next_id();
        let request = build_request(id, &method, params);

        self.pending.lock().insert(id, tx);

        let serialized_request = serde_json::to_string(&request)
            .expect("Serialization never fails");

        trace!("Writing request to socket: {:?}", &request);

        ClientResponse::new(
            id, self.send(ws::Message::Text(serialized_request)), rx
        )
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

    fn next_id(&self) -> u64 {
        self.ids.fetch_add(1, Ordering::Relaxed)
    }

    fn send(&self, msg: ws::Message) -> Result<(), Error> {
        let worker = self.sockets.lock().pop_front()
            .ok_or::<Error>(ErrorKind::YumError("No sockets available".into()).into())?;

        let socket_msg = WorkerRequest::Message(msg);
        let _: () = worker.send_request(socket_msg)?;

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

//impl Drop for Client {
//    fn drop(&mut self) {
//        trace!("Dropping client");
//        let mut socket_guard = self.sockets.lock();
//        for socket in socket_guard.iter_mut() {
//            socket.stop().expect("Socket worker will close");
//        }
//    }
//}

pub enum WorkerRequest {
    Message(ws::Message),
    Close(ws::CloseCode),
    Shutdown
}

struct SocketWorker {
    tx: mpsc::UnboundedSender<WorkerRequest>,
    status: Arc<RpcSocketStatus>,
    thread: Option<thread::JoinHandle<()>>
}

impl SocketWorker {
    pub fn new(id: u64, host: &str, pending: PendingRequests) -> Result<Self, Error> {
        trace!("Attempting to start new socket with id: {}, connecting to: {}", &id, &host);
        let (worker_queue, worker_status, worker_thread) = SocketWorker::start(id, host, pending)?;

        Ok(SocketWorker {
            tx: worker_queue,
            status: worker_status,
            thread: Some(worker_thread)
        })
    }

    pub fn is_connected(&self) -> bool {
        self.status.is_closed()
    }

    pub fn send_request(&self, req: WorkerRequest) -> Result<(), Error> {
        self.tx.unbounded_send(req).map_err(Into::into)
    }

    pub fn stop(&self) -> Result<(), Error> {
        self.send_request(WorkerRequest::Shutdown)
    }

    fn start(id: u64, host: &str, pending: PendingRequests)
             -> Result<(mpsc::UnboundedSender<WorkerRequest>, Arc<RpcSocketStatus>, thread::JoinHandle<()>), Error>
    {
        let trace_id = format!("[socket-worker-{} -> {}]", &id, &host);
        trace!("{} starting up...", &trace_id);
        let host = host.to_string();
        let thread_id = format!("yum-socket-worker-{}", id);

        let (request_tx, request_rx) = sync::mpsc::channel();
        let (status_tx, status_rx) = sync::mpsc::channel();

        let t_handle = thread::Builder::new().name(thread_id.clone()).spawn(move || {
            trace!("{} entering thread: {}", &trace_id.clone(), &thread_id.clone());

            let pool = CpuPool::new(1);
            let socket = Arc::new(RpcSocket::new(&host, pending).unwrap());
            let socket_handle = socket.clone();
            status_send.send(socket.get_status()).unwrap();
            let done = Ok(socket_handle).into_future()
                .and_then(move |rpc_socket| {
                    trace!("{} Starting receive queue...", &thread_id);

                    rx.map_err(|_| ErrorKind::YumError("Socket unreachable".into()).into())
                        .fold(rpc_socket, |websocket, w_req| {
                            match w_req {
                                WorkerRequest::Message(msg) => {
                                    trace!("Writing request to socket: {:?}", &msg);
                                    websocket.send(msg).and_then(|_| Ok(websocket))
                                },
                                WorkerRequest::Close(code) => {
                                    trace!("Closing websocket connection with code: {:?}", &code);
                                    websocket.close_with(code).and_then(|_| Ok(websocket))
                                },
                                WorkerRequest::Shutdown => {
                                    trace!("Shutting down websocket connection");
                                    websocket.shutdown().and_then(|_| Ok(websocket))
                                }
                            }
                        })
                        .and_then(|ws| ws.shutdown())
                });

            pool.execute(done.and_then(|_| Ok(())).map_err(|e| error!("Socket worker: {:?}", e))).unwrap();
        })?;
        trace!("Attempting to receive rpc socket status");
        let rpc_socket_status = status_recv.recv().unwrap();
        Ok((tx, rpc_socket_status, t_handle))
    }

//    fn start(id: u64, host: &str, pending: PendingRequests)
//        -> Result<(mpsc::UnboundedSender<WorkerRequest>, Arc<RpcSocketStatus>, thread::JoinHandle<()>), Error>
//    {
//        let trace_id = format!("[socket-worker-{} -> {}]", &id, &host);
//        trace!("{} starting up...", &trace_id);
//        let host = host.to_string();
//        let thread_id = format!("yum-socket-worker-{}", id);
//        let (tx, rx) = mpsc::unbounded();
//        let (status_send, status_recv) = sync::mpsc::channel();
//
//        let t_handle = thread::Builder::new().name(thread_id.clone()).spawn(move || {
//            trace!("{} entering thread: {}", &trace_id.clone(), &thread_id.clone());
//
//            let pool = CpuPool::new(1);
//            let socket = Arc::new(RpcSocket::new(&host, pending).unwrap());
//            let socket_handle = socket.clone();
//            status_send.send(socket.get_status()).unwrap();
//            let done = Ok(socket_handle).into_future()
//                .and_then(move |rpc_socket| {
//                    trace!("{} Starting receive queue...", &thread_id);
//
//                    rx.map_err(|_| ErrorKind::YumError("Socket unreachable".into()).into())
//                        .fold(rpc_socket, |websocket, w_req| {
//                            match w_req {
//                                WorkerRequest::Message(msg) => {
//                                    trace!("Writing request to socket: {:?}", &msg);
//                                    websocket.send(msg).and_then(|_| Ok(websocket))
//                                },
//                                WorkerRequest::Close(code) => {
//                                    trace!("Closing websocket connection with code: {:?}", &code);
//                                    websocket.close_with(code).and_then(|_| Ok(websocket))
//                                },
//                                WorkerRequest::Shutdown => {
//                                    trace!("Shutting down websocket connection");
//                                    websocket.shutdown().and_then(|_| Ok(websocket))
//                                }
//                            }
//                        })
//                        .and_then(|ws| ws.shutdown())
//                });
//
//            pool.execute(done.and_then(|_| Ok(())).map_err(|e| error!("Socket worker: {:?}", e))).unwrap();
//

//            thread::spawn(move || {
//
//
//                let done = socket.into_future()
//                    .and_then(move |rpc_socket| {
//                        let status = rpc_socket.get_status();
//                        status_send.send(status).expect("Couldn't send socket status");
//                        trace!("{} Starting receive queue...", &thread_id);
//
//                        rx.map_err(|_| ErrorKind::YumError("Socket unreachable".into()).into())
//                            .fold(rpc_socket, |websocket, w_req| {
//                                match w_req {
//                                    WorkerRequest::Message(msg) => {
//                                        trace!("Writing request to socket: {:?}", &msg);
//                                        websocket.send(msg).and_then(|_| Ok(websocket))
//                                    },
//                                    WorkerRequest::Close(code) => {
//                                        trace!("Closing websocket connection with code: {:?}", &code);
//                                        websocket.close_with(code).and_then(|_| Ok(websocket))
//                                    },
//                                    WorkerRequest::Shutdown => {
//                                        trace!("Shutting down websocket connection");
//                                        websocket.shutdown().and_then(|_| Ok(websocket))
//                                    }
//                                }
//                            })
//                            .and_then(|ws| ws.shutdown())
//                    }).wait().unwrap();
//            });
            //pool.execute(done.and_then(|_| Ok(())).map_err(|e| error!("Socket worker: {:?}", e))).unwrap();

//            current_thread::run(|_| {
//                trace!("Running receive queue future");
//                current_thread::spawn(lazy(|| {
//                    done.and_then(|_| Ok(())).map_err(|e| error!("Socket worker: {:?}", e))
//                }))
//            });
//        })?;
//        trace!("Attempting to receive rpc socket status");
//        let rpc_socket_status = status_recv.recv().unwrap();
//        Ok((tx, rpc_socket_status, t_handle))
//    }
}

//impl Drop for SocketWorker {
//    fn drop(&mut self) {
//        self.stop().expect("Socket stops properly");
//        self.tx.close().expect("Socket queue closes properly");
//        self.thread
//            .take()
//            .expect("Thread is only dropped once")
//            .join()
//            .expect("Socket thread shuts down properly");
//    }
//}

#[derive(Debug)]
struct RpcSocketStatus {
    is_closed: Arc<AtomicBool>,
    is_shutdown: Arc<AtomicBool>,
}

impl RpcSocketStatus {
    pub fn new() -> Self {
        RpcSocketStatus {
            is_closed: Arc::new(AtomicBool::new(true)),
            is_shutdown: Arc::new(AtomicBool::new(false))
        }
    }

    pub fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::Relaxed)
    }

    pub fn is_shutdown(&self) -> bool {
        self.is_shutdown.load(Ordering::Relaxed)
    }

    pub fn set_closed(&self, status: bool) {
        self.is_closed.store(status, Ordering::Relaxed)
    }

    pub fn set_shutdown(&self, status: bool) {
        self.is_shutdown.store(status, Ordering::Relaxed)
    }
}

struct RpcSocket {
    tx: ws::Sender,
    status: Arc<RpcSocketStatus>,
    thread: Option<thread::JoinHandle<()>>,
    pending: PendingRequests,
}

impl RpcSocket {
    pub fn new(url: &str, pending: PendingRequests) -> Result<Self, Error> {
        trace!("Attempting to get new rpc socket connection");
        let (socket, socket_status, handle) = RpcSocket::connect(url, pending.clone())?;

        Ok(RpcSocket {
            tx: socket,
            status: socket_status,
            thread: Some(handle),
            pending: pending.clone()
        })
    }

    pub fn get_status(&self) -> Arc<RpcSocketStatus> {
        self.status.clone()
    }

    pub fn close(&self) -> Result<(), Error> {
        self.close_with(ws::CloseCode::Normal)
    }

    pub fn close_with(&self, code: ws::CloseCode) -> Result<(), Error> {
        if self.status.is_closed() {
            Ok(())
        } else {
            self.tx.close(code).map_err(Into::into)
        }
    }

    pub fn shutdown(&self) -> Result<(), Error> {
        if self.status.is_shutdown() {
            Ok(())
        } else {
            self.tx.shutdown().map_err(Into::into)
        }
    }

    fn connect(host: &str, pending: PendingRequests)
        -> Result<(ws::Sender, Arc<RpcSocketStatus>, thread::JoinHandle<()>), Error>
    {
        trace!("connecting to host: {:?}", &host);

        let (tx, rx) = sync::mpsc::channel();
        let h = host.to_string();
        let status = Arc::new(RpcSocketStatus::new());
        let status_socket = status.clone();

        let handle = thread::Builder::new().spawn(move || {
            trace!("entering actual websocket connection thread");
            ws::connect(h.clone(), |out| {
                trace!("in ws-rs connection closure");
                // get write handle out of closure
                tx.send(out).unwrap();

                SocketConnection {
                    status: status_socket.clone(),
                    pending: pending.clone()
                }
            }).expect(&format!("Couldn't connect to {}", &h));
        }).expect("Threads needs to spawn");

        let mut iter_count = 0;

        loop {
            iter_count += 1;

            // 5 secs
            if iter_count > 50 {
                return Err(ErrorKind::YumError(format!("Couldn't connect to host: {}", &host)).into());
            }

            if status.is_closed() {
                thread::sleep(Duration::from_millis(100));
                continue
            } else {
                trace!("Socket connection open");
                break
            }
        }

        let socket_tx = rx.recv().expect("Couldn't get ws socket handle");
        Ok((socket_tx, status, handle))
    }

    pub fn send(&self, msg: ws::Message) -> Result<(), Error> {
        self.tx.send(msg).map_err(Into::into)
    }
}

struct SocketConnection {
    status: Arc<RpcSocketStatus>,
    pending: PendingRequests
}

impl ws::Handler for SocketConnection {
    fn on_open(&mut self, _: ws::Handshake) -> Result<(), ws::Error> {
        self.status.set_closed(false);
        Ok(())
    }

    fn on_close(&mut self, _: ws::CloseCode, _: &str) {
        self.status.set_closed(true);
    }

    fn on_shutdown(&mut self) {
        self.status.set_shutdown(true);
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

impl Drop for RpcSocket {
    fn drop(&mut self) {
        trace!("Closing socket");
        self.shutdown()
            .expect("Socket closes without issue");

        self.thread
            .take()
            .expect("drop is only called once")
            .join()
            .expect("thread shuts down cleanly")
    }
}

enum MessageState {
    InFlight(Option<Result<(), Error>>, oneshot::Receiver<Result<Value, Error>>),
    Waiting(oneshot::Receiver<Result<Value, Error>>),
    Complete
}

pub struct ClientResponse {
    id: u64,
    state: MessageState
}

impl ClientResponse {
    pub fn new(
        id: u64,
        send_status: Result<(), Error>,
        rx: oneshot::Receiver<Result<Value, Error>>) -> Self
    {
        ClientResponse {
            id,
            state: MessageState::InFlight(Some(send_status), rx)
        }
    }
}

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
            id: response_id,
            result: success.result
        })
    } else {
        let err_message = format!("invalid correlation id, expected u64, got: {:?}", success.id);
        Err(ErrorKind::RpcError(err_message).into())
    }
}

fn handle_failure(failure: rpc::Failure) -> Result<RpcResponse, Error> {
    if let rpc::Id::Num(response_id) = failure.id {
        Ok(RpcResponse::Failure {
            id: response_id,
            result: ErrorKind::YumError(format!("{:?}", failure.error)).into()
        })
    } else {
        let err_message = format!("invalid correlation id, expected u64, got: {:?}", failure.id);
        Err(ErrorKind::RpcError(err_message).into())
    }
}

pub fn build_request(id: u64, method: &str, params: Vec<Value>) -> rpc::Call {
    rpc::Call::MethodCall(rpc::MethodCall {
        jsonrpc: Some(rpc::Version::V2),
        method: method.to_string(),
        params: Some(rpc::Params::Array(params)),
        id: rpc::Id::Num(id)
    })
}

