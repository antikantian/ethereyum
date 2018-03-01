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

    let yum = YumClient::new(&["ws://127.0.0.1:8546"], 1).unwrap();

    let latest_block = yum.block_number().wait().unwrap();

    let mut pb = ProgressBar::new(latest_block - 3154196);

    let bstream = yum.get_block_stream(3154196, latest_block, false)
        .map_err(|_| ())
        .fold(pb, |mut prog, block| {
            prog.inc();
            Ok::<ProgressBar<Stdout>, ()>(prog)
        })
        .and_then(|mut pb| Ok(pb.finish()));

    bstream.wait();

    std::process::exit(1);
}