extern crate ethereyum;
extern crate futures;


use futures::Future;
use ethereyum::yum::YumClient;

fn main() {
    let client = YumClient::new("ws://localhost:8546", 1).unwrap();

    client.block_number().map(|x| println!("{:?}", x)).wait();
}