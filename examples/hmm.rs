extern crate ethereum_models;
extern crate ethereyum;
extern crate futures;
extern crate pretty_env_logger;

use std::str::FromStr;
use futures::Future;
use ethereum_models::types::{H160, H256};
use ethereum_models::objects::*;
use ethereyum::yum::YumClient;

fn main() {
    pretty_env_logger::init();

    let client = YumClient::new("ws://127.0.0.1:8546", 1).unwrap();

    println!("hmmm");

    let a = client.accounts().map(|x| println!("Accounts: {:?}", x)).wait();

    let b = client.block_number().map(|x| println!("Block number: {:?}", x)).wait();

    let c = client.coinbase().map(|x| println!("Coinbase: {:?}", x)).wait();

    let d = client.gas_price().map(|x| println!("Gas price: {:?}", x)).wait();

    let e = client.get_balance(H160::from_str("8d12A197cB00D4747a1fe03395095ce2A5CC6819").unwrap(), BlockNumber::Name("latest"))
        .map(|x| println!("Balance: {:?}", x)).wait();

//    let f = client.get_block_by_hash(H256::from_str("0946d10c54cce1202bb11297f750a713cc3929d66de1ceb3aae75777abdcdbdb").unwrap(), true)
//        .map(|x| println!("Block: {:?}", x)).wait();

    std::process::exit(1);
}