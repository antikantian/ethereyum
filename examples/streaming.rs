extern crate ethereum_models;
extern crate ethereyum;
extern crate futures;
extern crate pretty_env_logger;

use std::io::{self, Write};
use futures::prelude::*;
use ethereum_models::objects::*;
use ethereyum::client::{Client, BlockStream};
use ethereyum::yum::{YumClient};
use ethereyum::error::Error;

fn main() {
    pretty_env_logger::init();

    let client = YumClient::new("ws://127.0.0.1:8546", 1).unwrap();

    let bstream = client.get_block_stream(5000000, 5000099, true);

    let res = bstream.for_each(|block| {
        println!("Have block {}", block.number.unwrap());
        io::stdout().flush().unwrap();
        Ok(())
    }).wait();

    if let Err(e) = res {
        println!("{:?}", e);
    }

    std::process::exit(1);
}