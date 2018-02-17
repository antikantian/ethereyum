extern crate ethereyum;
extern crate futures;
extern crate pretty_env_logger;

use ethereyum::client::Client;
use futures::future::Future;

fn main() {
    pretty_env_logger::init();

    let client = Client::new("ws://localhost:8546").unwrap();
    let x = client.execute_request("eth_blockNumber", vec![]).wait().unwrap();
    println!("{:?}", x);

}