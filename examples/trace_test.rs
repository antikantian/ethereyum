//

extern crate ansi_term;
extern crate ethereum_models;
extern crate ethereyum;
extern crate futures;
extern crate pbr;
extern crate pretty_env_logger;

use std::str::FromStr;
use std::io::Stdout;
use std::thread;
use std::time::Duration;

use ansi_term::Colour;
use ansi_term::Colour::{Green, Red};
use futures::{Future, future, Stream};
use ethereum_models::types::{H160, H256, U256};
use ethereum_models::objects::*;
use ethereyum::{YumFuture, YumBatchFuture, YumBatchFutureT};
use ethereyum::yum::YumClient;
use ethereyum::error::Error;
use pbr::MultiBar;
use pbr::ProgressBar;

fn main() {
    pretty_env_logger::init();

    let hashes = vec![H256::from_str("e70395b9b7ef08bf3a82d0b5f2cec43cd43ce728b881c69159f89ca5d132cc5b").unwrap()];

    let yum = YumClient::new(&["ws://127.0.0.1:8546"], 1).unwrap();

    let traces = yum.trace_transactions(&hashes).wait();

    println!("{:?}", traces);

    std::process::exit(1);
}