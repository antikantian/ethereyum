extern crate ansi_term;
extern crate ethereum_models;
extern crate ethereyum;
extern crate futures;
extern crate pretty_env_logger;

use std::str::FromStr;
use std::thread;
use std::time::Duration;

use ansi_term::Colour;
use ansi_term::Colour::{Green, Red};
use futures::{Future, future};
use ethereum_models::types::{H160, H256, U256};
use ethereum_models::objects::*;
use ethereyum::{YumFuture, YumBatchFuture, YumBatchFutureT};
use ethereyum::yum::YumClient;

use ethereyum::error::Error;

fn main() {
    pretty_env_logger::init();

    let yum = YumClient::new(&["ws://127.0.0.1:8546"], 1).unwrap();

    while true {
        println!("Client connected: {}", yum.is_connected());
        yum.block_number().then(move |bn| {
            match bn {
                Ok(num) => println!("Block {}", num),
                Err(e) => println!("Error: {:?}", e)
            }
            future::ok::<_, Error>(())
        }).wait();
        thread::sleep(Duration::from_secs(1));
    }
    std::process::exit(1);
}