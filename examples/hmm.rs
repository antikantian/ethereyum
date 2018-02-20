extern crate ethereum_models;
extern crate ethereyum;
extern crate futures;
extern crate pretty_env_logger;
extern crate serde_json;
extern crate serde;
#[macro_use] extern crate serde_derive;

use std::fmt;
use std::str::FromStr;
use std::marker::PhantomData;
use futures::Future;
use futures::future::join_all;
use ethereum_models::types::{H160, H256, U256};
use ethereum_models::objects::*;
use ethereyum::client::Client;
use ethereyum::yum::{YumClient};
use serde::de::*;
use serde::de;
use serde_json::Value;

use ethereyum::error::Error;

fn mmm<T: DeserializeOwned>(v: Value) -> Result<T, Error> {
    serde_json::from_value(v).map_err(Into::into)
}


fn main() {
    pretty_env_logger::init();

    let client = YumClient::new("ws://127.0.0.1:8546", 1).unwrap();
    //let client = Client::new("ws://127.0.0.1:8546", 1).unwrap();

    println!("hmmm");

    let address1 = H160::from_str("5d14fe1e974640dA880E2f04382F2c679E15bc84").unwrap();
    let address2 = H160::from_str("5d14fe1e974640dA880E2f04382F2c679E15bc84").unwrap();
    let address3 = H160::from_str("5d14fe1e974640dA880E2f04382F2c679E15bc84").unwrap();
    let address4 = H160::from_str("5d14fe1e974640dA880E2f04382F2c679E15bc84").unwrap();

    let addresses = vec![address1, address2, address3, address4];

    let x = client.classify_addresses(addresses).wait();

    println!("{:?}", x);

//
//    let calls = vec![
//        ("eth_getCode".to_string(), vec![ser(&address1)]),
//        ("eth_getCode".to_string(), vec![ser(&address2)]),
//        ("eth_getCode".to_string(), vec![ser(&address3)]),
//        ("eth_getCode".to_string(), vec![ser(&address4)]),
//    ];


//    let a = client.accounts().map(|x| println!("Accounts: {:?}", x)).wait();
//
//    let b = client.block_number().map(|x| println!("Block number: {:?}", x)).wait();
//
//    let c = client.coinbase().map(|x| println!("Coinbase: {:?}", x)).wait();
//
//    let d = client.gas_price().map(|x| println!("Gas price: {:?}", x)).wait();
//
//    let e = client.get_balance(&H160::from_str("8d12A197cB00D4747a1fe03395095ce2A5CC6819").unwrap(), &BlockNumber::Name("latest"))
//        .map(|x| println!("Balance: {:?}", x)).wait();

//    let f = client.get_block_by_hash(H256::from_str("0946d10c54cce1202bb11297f750a713cc3929d66de1ceb3aae75777abdcdbdb").unwrap(), true)
//        .map(|x| println!("Block: {:?}", x)).wait();



    std::process::exit(1);
}