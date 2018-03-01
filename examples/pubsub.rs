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
use ethereyum::client::{Client, Pubsub};
use ethereyum::ops::PubsubOps;
use ethereyum::yum::{YumClient};
use ethereyum::error::Error;

fn main() {
    pretty_env_logger::init();

    let client = YumClient::new(&["ws://127.0.0.1:8546"], 1).unwrap();

    let bstream = client.new_blocks_with_tx();

    let res = bstream.for_each(|block| {
        println!("New block {:?}", block);
        Ok(())
    }).wait();

    std::process::exit(1);
}