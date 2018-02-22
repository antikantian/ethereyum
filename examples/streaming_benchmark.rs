extern crate ethereum_models;
extern crate ethereyum;
extern crate futures;
extern crate pretty_env_logger;

#[macro_use]
extern crate log;

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use futures::prelude::*;
use ethereum_models::objects::*;
use ethereyum::client::{Client, BlockStream};
use ethereyum::yum::{YumClient};
use ethereyum::error::Error;

fn calc_blocks_per_sec(start: &Instant, blocks_processed: f64) -> f64 {
    let elapsed = start.elapsed().as_secs() as f64;
    blocks_processed / elapsed
}


fn main() {
    pretty_env_logger::init();

    let from_block = 5000000;
    let to_block = 5010000;
    // 10,000 blocks;
    let total_blocks = to_block - from_block;
    let start_time = Instant::now();

    let blocks_processed = Arc::new(AtomicUsize::new(0));
    let blocks_processed2 = blocks_processed.clone();

    let done = Arc::new(AtomicBool::new(false));
    let done2 = done.clone();

    let t_handle = thread::spawn(move || {
        loop {
            if done2.load(Ordering::Relaxed) {
                break;
            }

            let processed = blocks_processed2.load(Ordering::Relaxed) as u64;
            let remaining = total_blocks - processed;
            let bps: f64 = calc_blocks_per_sec(&start_time, processed as f64);
            let time_remaining: f64 = if bps > 1f64 {
                remaining as f64 / bps
            } else {
                0f64
            };

            let time_remaining_mins = time_remaining / 60f64;

            print!(
                "processed: {}, remaining: {}, rate: {:.2} blocks/sec, time remaining: {:.2} mins      \r",
                processed, remaining, bps, time_remaining_mins
            );
            io::stdout().flush().unwrap();
            thread::sleep(Duration::from_millis(250));
        }
    });

    let client = YumClient::new("ws://127.0.0.1:8546", 4).unwrap();

    let bstream = client.get_block_stream(from_block, to_block, true);

    let run_it = bstream.for_each(|block| {
        blocks_processed.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }).wait();

    if let Err(e) = run_it {
        println!("Error: {:?}", e);
    }

    done.store(true, Ordering::Relaxed);
    t_handle.join();

    std::process::exit(1);
}