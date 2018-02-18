extern crate ethereum_models;
extern crate ethereyum;
extern crate futures;
extern crate parking_lot;
extern crate serde_json;
extern crate pretty_env_logger;
extern crate tokio;
extern crate futures_cpupool;
extern crate web3;

use std::{env, thread, time};
use std::sync::{atomic, Arc};

use futures::future::lazy;
use ethereum_models::types::U256;
use ethereyum::client::Client;
use ethereyum::Error;
use futures::future::Future;
use futures::IntoFuture;
use futures::future::join_all;
use futures::Canceled;
use parking_lot::Mutex;
use serde_json::Value;
use futures_cpupool::CpuPool;
use futures::future::Executor;

// Benchmark code comes from the rust-web3 library:
// https://github.com/tomusdrw/rust-web3/blob/master/examples/bench.rs

fn as_millis(dur: time::Duration) -> f64 {
    dur.as_secs() as f64 * 1_000.0 + dur.subsec_nanos() as f64 / 1_000_000.0
}

struct Ticker {
    id: String,
    started: atomic::AtomicUsize,
    reqs: atomic::AtomicUsize,
    time: Mutex<time::Instant>,
}

impl Ticker {
    pub fn new(id: &str) -> Self {
        Ticker {
            id: id.to_owned(),
            time: Mutex::new(time::Instant::now()),
            started: Default::default(),
            reqs: Default::default(),
        }
    }

    pub fn start(&self) {
        self.started.fetch_add(1, atomic::Ordering::AcqRel);
    }

    pub fn tick(&self) {
        let reqs = self.reqs.fetch_add(1, atomic::Ordering::AcqRel) as u64;
        self.started.fetch_sub(1, atomic::Ordering::AcqRel);
    }

    pub fn print_result(&self, reqs: u64) {
        let mut time = self.time.lock();
        let elapsed = as_millis(time.elapsed());
        let result = reqs as f64 * 1_000.0 / elapsed;

        println!("[{}] {:.2} reqs/s ({} reqs in {:.2} ms)", self.id, result, reqs, elapsed);

        self.reqs.store(0, atomic::Ordering::Release);
        *time = time::Instant::now();
    }

    pub fn wait(&self) {
        while self.started.load(atomic::Ordering::Relaxed) > 0 {
            thread::sleep(time::Duration::from_millis(100));
        }
        self.print_result(self.reqs.load(atomic::Ordering::Acquire) as u64);
    }
}


fn main() {
    pretty_env_logger::init();

    let requests = 1000000;

    let (eloop, ipc) = web3::transports::Ipc::new("/root/.local/share/io.parity.ethereum/jsonrpc.ipc").unwrap();
    bench_web3("rust-web3:ipc", eloop, ipc, requests);

    let (eloop, http) = web3::transports::Http::new("http://localhost:8545").unwrap();
    bench_web3("rust-web3:http", eloop, http, requests);

    let client_1conn = Client::new("ws://localhost:8546", 1).unwrap();
    bench_ey("ethereyum:websocket:1-connection", &client_1conn, requests);

    drop(client_1conn);

    let client_2conn = Client::new("ws://localhost:8546", 2).unwrap();
    bench_ey("ethereyum:websocket:2-connections", &client_2conn, requests);

    drop(client_2conn);

    let client_3conn = Client::new("ws://localhost:8546", 3).unwrap();
    bench_ey("ethereyum:websocket:3-connections", &client_3conn, requests);

    drop(client_3conn);

    let client_4conn = Client::new("ws://localhost:8546", 4).unwrap();
    bench_ey("ethereyum:websocket:4-connections", &client_4conn, requests);

    std::process::exit(1);
}

fn bench_web3<T: web3::Transport>(id: &str, eloop: web3::transports::EventLoopHandle, transport: T, max: usize) where
    T::Out: Send + 'static,
{
    let web3 = web3::Web3::new(transport);
    let ticker = Arc::new(Ticker::new(id));
    for _ in 0..max {
        let ticker = ticker.clone();
        ticker.start();
        let block = web3.eth().block_number().then(move |res| {
            if let Err(e) = res {
                println!("Error: {:?}", e);
            }
            ticker.tick();
            Ok(())
        });
        eloop.remote().spawn(|_| block);
    }
    ticker.wait()
}

fn bench_ey(id: &str, client: &Client, max: usize) {
    let pool = CpuPool::new_num_cpus();

    let ticker = Arc::new(Ticker::new(id));

    let mut futs = Vec::new();
    for n in 0..max {
        let ticker = ticker.clone();
        ticker.start();

        let block = client.execute_request("eth_blockNumber", vec![]).then(move |res| {
            if let &Err(ref e) = &res {
                println!("{:?}", e);
            }

            ticker.tick();
            Ok(())
        });

        futs.push(pool.execute(block))

    }
    ticker.wait();
    futures::future::join_all(futs).map(|_| ()).wait();
}