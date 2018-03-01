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
    use std::str::FromStr;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;
    use crossbeam_utils as crossbeam;
    use ethereum_models::objects::BlockNumber;
    use ethereum_models::types::{H160, U256};
    use ethereyum::yum::{YumClient};
    use futures::Future;
    use futures::future::lazy;
    use jsonrpc_core::*;
    use jsonrpc_core::Params;
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

    #[test]
    fn gets_block_number() {
        thread::sleep(Duration::from_millis(2000));

        let block_number_hex = "0x4dfbff";
        let block_number_decimal = 5110783_u64;
        let (server, server_thread) = setup_websocket(
            3103, "eth_blockNumber", Value::String(block_number_hex.into())
        );

        thread::sleep(Duration::from_millis(2000));

        let client = YumClient::new(&["ws://127.0.0.1:3103"], 1).expect("Client must start");

        thread::sleep(Duration::from_millis(2000));

        let block_op = client.block_number().wait().unwrap();

        thread::sleep(Duration::from_millis(2000));

        assert_eq!(block_op, block_number_decimal);
        assert!(server.shutdown().is_ok());
        assert!(server_thread.join().is_ok());
    }

    #[test]
    fn gets_accounts() {
        thread::sleep(Duration::from_millis(2000));

        let eth_account = "0x0000000000000000000000000000000000000000";
        let eth_as_h160 = H160::from_str("0000000000000000000000000000000000000000").unwrap();
        let response = serde_json::to_value(vec![Value::String(eth_account.into())]).unwrap();
        let (server, server_thread) = setup_websocket(
            3104, "eth_accounts", response
        );

        thread::sleep(Duration::from_millis(2000));

        let client = YumClient::new(&["ws://127.0.0.1:3104"], 1).expect("Client must start");

        thread::sleep(Duration::from_millis(2000));

        let account_op = client.accounts().wait().unwrap();

        thread::sleep(Duration::from_millis(2000));

        assert_eq!(format!("{:?}", account_op.get(0).unwrap()), format!("{:?}", eth_as_h160));
        assert!(server.shutdown().is_ok());
        assert!(server_thread.join().is_ok());
    }

    #[test]
    fn gets_code_at_address() {
        thread::sleep(Duration::from_millis(2000));

        let dummy_address = H160::from_str("a94f5374fce5edbc8e2a8697c15331677e6ebf0b").unwrap();
        let dummy_block = BlockNumber::Name("latest");
        let contract_code = "0x600160008035811a818181146012578301005b601b6001356025565b8060005260206000f25b600060078202905091905056";
        let (server, server_thread) = setup_websocket(
            3105, "eth_getCode", Value::String(contract_code.into())
        );

        thread::sleep(Duration::from_millis(2000));

        let client = YumClient::new(&["ws://127.0.0.1:3105"], 1).expect("Client must start");

        thread::sleep(Duration::from_millis(2000));

        let code_op = client.get_code(&dummy_address, &dummy_block)
            .wait()
            .unwrap();

        thread::sleep(Duration::from_millis(2000));

        assert_eq!(code_op, contract_code);
        assert!(server.shutdown().is_ok());
        assert!(server_thread.join().is_ok());
    }

    #[test]
    fn handles_unresponsive_host() {
        thread::sleep(Duration::from_millis(2000));
        let block_number_hex = "0x4dfbff";
        let dummy_address = H160::from_str("a94f5374fce5edbc8e2a8697c15331677e6ebf0b").unwrap();
        let dummy_block = BlockNumber::Name("latest");
        let (server, server_thread) = setup_websocket(
            3106, "eth_blockNumber", Value::String(block_number_hex.into())
        );

        thread::sleep(Duration::from_millis(2000));

        let client = YumClient::new(&["ws://127.0.0.1:3106"], 1).expect("Client must start");

        thread::sleep(Duration::from_millis(2000));

        let block_op = client.block_number().wait().unwrap();
        assert!(client.is_connected());
        assert!(server.shutdown().is_ok());
        assert!(server_thread.join().is_ok());

        thread::sleep(Duration::from_millis(2000));

        assert!(!client.is_connected())
    }

}


