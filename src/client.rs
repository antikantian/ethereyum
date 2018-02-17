use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::convert::From;

use serde::de::DeserializeOwned;
use crossbeam_channel as channel;
use futures::future::IntoFuture;
use crossbeam_deque::{Deque, Steal, Stealer};
use fnv::FnvHashMap;
use futures::{self, Async, Canceled, Future, Stream, Sink};
use std::io::{Read, Write};
use futures::sync::{oneshot, mpsc};
use futures::sync::oneshot::Receiver;
use parking_lot::Mutex;
use rpc::{self, Response, Output};
use serde_json::{self, Value};
use ws;
use serde::de::Deserializer;
use futures::prelude::*;
use futures_cpupool::CpuPool;
use futures::future::lazy;
use tokio::executor::current_thread;
use futures::future::FutureResult;

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

// Using Fowler-Noll-Vo hasher because it performs significantly better
// with integer keys.  See: http://cglab.ca/%7Eabeinges/blah/hash-rs/
pub struct Client {
    socket: Arc<RpcSocket>,
    ids: Arc<AtomicU64>,
    pending: PendingRequests
}

impl Client {
    pub fn new(host: &str) -> Result<Self, Error> {
        let mut connections: Vec<Arc<RpcSocket>> = Vec::new();

        let requests = Arc::new(Mutex::new(FnvHashMap::default()));

//        for _ in 0..1 {
//            let rpc_socket = RpcSocket::new(host, requests.clone(), work_queue.stealer())?;
//            connections.push(Arc::new(rpc_socket));
//        }

        let rpc_socket = RpcSocket::new(host, requests.clone())?;

        Ok(Client {
            socket: Arc::new(rpc_socket),
            ids: Arc::new(AtomicU64::new(0)),
            pending: requests
        })
    }

    pub fn next_id(&self) -> u64 {
        self.ids.fetch_add(1, Ordering::Relaxed)
    }

    //Box<Future<Item=Result<Value, Error>, Error=Canceled>>
    pub fn execute_request(&self, method: &str, params: Vec<Value>) -> FutureResult<Receiver<Result<Value, Error>>, Canceled>
    {
        let (tx, rx) = oneshot::channel();

        let id = self.next_id();
        let request = build_request(id, &method, params);

        self.pending.lock().insert(id, tx);

        let serialized_request = serde_json::to_string(&request)
            .expect("Serialization never fails");

        trace!("Writing request to socket: {:?}", &request);

        self.socket.send(ws::Message::Text(serialized_request)).unwrap();
        Ok(rx).into_future()
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
        self.thread
            .take()
            .expect("drop is only called once")
            .join()
            .expect("thread shuts down cleanly")
    }
}

//impl Drop for SocketConnection {
//    fn drop(&mut self) {
//        if let Err(e) = self.out.close(ws::CloseCode::Normal) {
//            error!("Error on close: {:?}", e);
//        }
//    }
//}


fn call_to_message(rpc_call: rpc::Call) -> ws::Message {
    let call = serde_json::to_string(&rpc_call)
        .expect("Serialization of a call should never fail");
    ws::Message::Text(call)
}

fn handle_text_message(text: String) -> Result<Vec<RpcResponse>, Error> {
    serde_json::from_str::<Response>(&text)
        .map_err(Into::into)
        .and_then(|resp| handle_response(resp))
}

fn handle_binary_message(binary: Vec<u8>) -> Result<Vec<RpcResponse>, Error> {
    serde_json::from_slice::<Response>(&binary)
        .map_err(Into::into)
        .and_then(|resp| handle_response(resp))
}

fn handle_response(response: Response) -> Result<Vec<RpcResponse>, Error> {
     match response {
        Response::Single(output) => {
            let rpc_response = handle_output(output)?;
            Ok(vec![rpc_response])
        },
        Response::Batch(vec_output) => {
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

