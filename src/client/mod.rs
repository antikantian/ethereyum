use std::collections::VecDeque;
use std::{self, io};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::convert::From;

use crossbeam_channel as channel;
use futures::future::IntoFuture;
use fnv::FnvHashMap;
use futures::{Async, Poll, Future, Stream, Sink};
use futures::sync::{oneshot, mpsc};
use parking_lot::Mutex;
use rpc::{self, Output};
use serde_json::{self, Value};
use ws;
use futures::future::lazy;
use tokio::executor::current_thread;

use error::{Error, ErrorKind};

pub mod result;

pub use self::result::{YumResult};

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

// Using Fowler-Noll-Vo hasher because it performs significantly better
// with integer keys.  See: http://cglab.ca/%7Eabeinges/blah/hash-rs/
// This is probably insignificant since we're not expecting that many concurrent requests
pub struct Client {
    host: String,
    sockets: Arc<Mutex<VecDeque<mpsc::UnboundedSender<ws::Message>>>>,
    socket_threads: Vec<Option<thread::JoinHandle<()>>>,
    ids: Arc<AtomicU64>,
    pending: PendingRequests
}

impl Client {
    pub fn new(host: &str, num_connections: u32) -> Result<Self, Error> {
        let requests = Arc::new(Mutex::new(FnvHashMap::default()));

        let mut client = Client {
            host: host.to_string(),
            sockets: Arc::new(Mutex::new(VecDeque::new())),
            socket_threads: Vec::new(),
            ids: Arc::new(AtomicU64::new(0)),
            pending: requests
        };

        for _ in 0..num_connections {
            client.start_connection()?;
        }

        Ok(client)
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
        let (tx, rx) = mpsc::unbounded();
        let worker = self.socket_worker(rx)?;
        self.sockets.lock().push_back(tx);
        self.socket_threads.push(Some(worker));
        Ok(())
    }

    fn next_id(&self) -> u64 {
        self.ids.fetch_add(1, Ordering::Relaxed)
    }

    fn send(&self, msg: ws::Message) -> Result<(), Error> {
        let worker: mpsc::UnboundedSender<ws::Message> = self.sockets.lock().pop_front()
            .ok_or::<Error>(ErrorKind::YumError("No sockets available".into()).into())?;

        if let Err(e) = worker.unbounded_send(msg) {
            error!("{:?}", e);
        }

        self.sockets.lock().push_back(worker);

        Ok(())
    }

    fn socket_worker(&self, rx: mpsc::UnboundedReceiver<ws::Message>) -> Result<thread::JoinHandle<()>, Error> {
        let host = self.host.to_string();
        let pending = self.pending.clone();

        let t_handle = thread::spawn(move || {
            info!("Starting new socket worker");

            let socket = RpcSocket::new(&host, pending);
            let done = socket.into_future()
                .and_then(move |rpc_socket| rx
                    .map_err(|_| ErrorKind::YumError("Socket unreachable".into()).into())
                    .fold(rpc_socket, |websocket, msg| {
                        trace!("Writing request to socket: {:?}", &msg);
                        websocket.send(msg).and_then(|_| Ok(websocket))
                    })
                    .and_then(|ws| ws.close())
                );

            current_thread::run(|_| {
                current_thread::spawn(lazy(|| {
                    done.and_then(|_| Ok(())).map_err(|e| error!("Socket worker: {:?}", e))
                }))
            });
        });
        Ok(t_handle)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        trace!("Dropping client");
        let mut socket_guard = self.sockets.lock();
        for socket in socket_guard.iter_mut() {
            socket.flush().and_then(|s| s.close()).wait()
                .expect("Socket queue will flush");
        }

        for th in self.socket_threads.iter_mut() {
            trace!("Dropping worker");
            th.take()
                .expect("Drop is called once")
                .join()
                .expect("Socket threads shut down cleanly");
        }
    }
}

struct RpcSocket {
    tx: ws::Sender,
    thread: Option<thread::JoinHandle<()>>,
    pending: PendingRequests,
}

impl RpcSocket {
    pub fn new(url: &str, pending: PendingRequests) -> Result<Self, Error> {
        let (socket, handle) = RpcSocket::connect(url, pending.clone()).unwrap();

        Ok(RpcSocket {
            tx: socket,
            thread: Some(handle),
            pending: pending.clone()
        })
    }

    pub fn close(self) -> Result<(), Error> {
        self.tx.close(ws::CloseCode::Normal).map_err(Into::into)
    }

    fn connect(host: &str, pending: PendingRequests) -> Result<(ws::Sender, thread::JoinHandle<()>), Error> {
        trace!("connecting to host: {:?}", &host);

        let (tx, rx) = channel::bounded(0);
        let host = host.to_string();

        let handle = thread::spawn(move || {
            ws::connect(host.clone(), |out| {
                // get write handle out of closure
                tx.send(out).unwrap();

                SocketConnection {
                    pending: pending.clone()
                }
            }).expect(&format!("Couldn't connect to {}", &host));
        });

        let socket_tx = rx.recv().unwrap();
        Ok((socket_tx, handle))
    }

    pub fn send(&self, msg: ws::Message) -> Result<(), Error> {
        self.tx.send(msg).map_err(Into::into)
    }
}

struct SocketConnection {
    pending: PendingRequests
}

impl ws::Handler for SocketConnection {
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
        self.tx.close(ws::CloseCode::Normal)
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
    state: MessageState,
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

