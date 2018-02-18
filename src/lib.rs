#![feature(integer_atomics)]
#![feature(proc_macro, conservative_impl_trait, generators)]

extern crate crossbeam;

#[macro_use]
extern crate crossbeam_channel;
extern crate crossbeam_deque;

#[macro_use]
extern crate error_chain;

#[macro_use]
extern crate log;

#[macro_use]
extern crate futures;
extern crate fixed_hash;
extern crate ethereum_models;
extern crate fnv;
extern crate futures_cpupool;
extern crate jsonrpc_core as rpc;
extern crate parking_lot;
extern crate reqwest;
extern crate serde;
extern crate tokio;
extern crate url;
extern crate ws;


#[macro_use]
extern crate serde_derive;

extern crate serde_json;

pub mod address;
pub mod client;
pub mod error;
pub mod yum;
pub mod web3;

pub use self::error::{Error, ErrorKind};

pub type FutureResult<T, E> = Box<futures::future::Future<Item=T, Error=E> + Send + 'static>;

