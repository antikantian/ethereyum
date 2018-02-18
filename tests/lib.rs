extern crate ethereyum;

#[cfg(test)]
extern crate crossbeam_utils;

#[cfg(test)]
extern crate ethereum_models;

#[cfg(test)]
extern crate futures;

#[cfg(test)]
extern crate jsonrpc_core;

#[cfg(test)]
extern crate jsonrpc_ws_server;

#[cfg(test)]
extern crate pretty_env_logger;

#[cfg(test)]
extern crate serde_json;

#[cfg(test)]
extern crate tokio;

#[cfg(test)]
extern crate ws;

#[cfg(test)]
mod yum_tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;
    use crossbeam_utils as crossbeam;
    use ethereum_models::types::U256;
    use ethereyum::yum::{YumClient, YumResult};
    use futures::Future;
    use futures::future::lazy;
    use jsonrpc_core::*;
    use pretty_env_logger;
    use serde_json::{self, Value};
    use tokio::executor::current_thread;
    use ws;
    use ws::Message;

    fn setup_mock_rpc(method: &str, response_value: Value) -> IoHandler {
        let mut io = IoHandler::new();
        io.add_method(method, move |_| {
            Ok(response_value.clone())
        });
        io
    }

    fn setup_websocket(port: u32, method: &str, response_value: Value) -> (ws::Sender, thread::JoinHandle<()>) {
        let method = method.to_string();
        let socket = ws::Builder::new().build(move |out: ws::Sender| {
            let mut io = setup_mock_rpc(&method.to_string(), response_value.clone());
            move |msg: Message| {
                match msg {
                    Message::Binary(_) => {},
                    Message::Text(request) => {
                        let response = io.handle_request_sync(&request).unwrap();
                        out.send(Message::Text(response));
                    }
                }
                Ok(())
            }
        }).unwrap();

        let handle = socket.broadcaster();

        let addr = format!("127.0.0.1:{}", port);
        let t = thread::spawn(move || {
            socket.listen(&addr).unwrap();
        });

        (handle, t)
    }

    #[test]
    fn mock_io_works() {
        let io = setup_mock_rpc("eth_blockNumber", Value::String("0x4dfbff".into()));

        let request = r#"{"jsonrpc": "2.0", "method": "eth_blockNumber", "params": [], "id": 1}"#;
        let response = r#"{"jsonrpc":"2.0","result":"0x4dfbff","id":1}"#;

        assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
    }

//    #[test]
//    fn client_connects_then_shuts_down() {
//        pretty_env_logger::init();
//        let (socket, t) = setup_websocket(3102, "hello", Value::String("said hello".into()));
//
//        thread::sleep(Duration::from_millis(1000));
//
//        let mut client = YumClient::new("ws://127.0.0.1:3012", 1)
//            .expect("Client connection required");
//
//        thread::sleep(Duration::from_millis(1000));
//
//        let client_shutdown = client.close();
//
//        assert!(client_shutdown.is_ok());
//        assert!(socket.shutdown().is_ok());
//        assert!(t.join().is_ok());
//    }

    #[test]
    fn gets_block_number() {
        pretty_env_logger::init();

        let block_number_hex = "0x4dfbff";
        let block_number_decimal = 5110783_u64;
        let (server, server_thread) = setup_websocket(
            3102, "eth_blockNumber", Value::String(block_number_hex.into())
        );

        thread::sleep(Duration::from_millis(1000));

        let client = YumClient::new("ws://127.0.0.1:3102", 1).expect("Client must start");

        let block_op = client.block_number().wait().unwrap().expect("Must have block number");

        assert_eq!(block_op, block_number_decimal);
        assert!(client.close().is_ok());
        assert!(server.shutdown().is_ok());
        assert!(server_thread.join().is_ok());
    }



}


