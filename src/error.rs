#![allow(unknown_lints)]
#![allow(missing_docs)]

use std::io;

use bigdecimal;
use client;
use crossbeam_channel;
use futures;
use reqwest;
use serde_json;
use tungstenite;

error_chain! {
  foreign_links {
    Crossbeam(crossbeam_channel::RecvError);
    Io(io::Error);
    Futurs(futures::Canceled);
    Http(reqwest::Error);
    Json(serde_json::Error);
    Mpsc(futures::sync::mpsc::SendError<client::SocketRequest>);
    Ws(tungstenite::Error);
    ParseBigDecimal(bigdecimal::ParseBigDecimalError);
  }
  errors {
    HostsUnreachable {
        description("hosts unreachable error"),
        display("Couldn't establish connection, all hosts unreachable")
    }
    ClientNotConnected {
        description("client not connected error"),
        display("Request was made of a disconnected client")
    }
    BatchRequestError {
        description("batch error"),
        display("error sending batch request")
    }
    RpcError(e: String) {
        description("rpc errpr"),
        display("RPC error: {}", e)
    }
    SocketTimeout(t: u64) {
        description("socket timeout error"),
        display("Couldn't connect to host, socket timed out after: {}", t)
    }
    ConnectionTimeout {
        description("connection timeout error"),
        display("Couldn't connect to host, connection timed out after the configured duration")
    }
    ConnectError {
        description("connect error"),
        display("Host unreachable")
    }
    YumError(e: String) {
        description("ethereyum error"),
        display("EthereYUM: {}", e)
    }
  }
}

