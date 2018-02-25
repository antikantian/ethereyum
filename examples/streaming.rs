extern crate ethereum_models;
extern crate ethereyum;
extern crate futures;
extern crate pretty_env_logger;
extern crate serde_json;

use std::io::{self, Write};
use std::thread;
use std::time::Duration;
use futures::prelude::*;
use ethereum_models::objects::*;
use ethereyum::client::{Client, BlockStream};
use ethereyum::yum::{YumClient};
use ethereyum::error::Error;

fn main() {
    pretty_env_logger::init();

    let client = YumClient::new(&["ws://127.0.0.1:8546"], 1).unwrap();

    let bstream = client.get_block_stream(5000000, 5000250, false);

    let mut count = 0;
    let res = bstream.for_each(|block| {
        count += 1;
        //thread::sleep(Duration::from_millis(1000));
        println!("Have block {}", block.number.unwrap());
        io::stdout().flush().unwrap();
        Ok(())
    }).wait();

    println!("Number of blocks: {}", count);

    std::process::exit(1);
}