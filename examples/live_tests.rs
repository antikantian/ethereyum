extern crate ansi_term;
extern crate ethereum_models;
extern crate ethereyum;
extern crate futures;
extern crate pretty_env_logger;

use std::str::FromStr;

use ansi_term::Colour;
use ansi_term::Colour::{Green, Red};
use futures::Future;
use ethereum_models::types::{H160, H256, U256};
use ethereum_models::objects::*;
use ethereyum::{YumFuture, YumBatchFuture, YumBatchFutureT};
use ethereyum::ops::{TokenOps};
use ethereyum::yum::YumClient;


use ethereyum::error::Error;

fn main() {
    pretty_env_logger::init();

    let yum = YumClient::new(&["ws://127.0.0.1:8546"], 1).unwrap();

    let latest_block = BlockNumber::Name("latest");
    let contract_address = H160::from_str("8d12A197cB00D4747a1fe03395095ce2A5CC6819").unwrap();
    let non_contract_address = H160::from_str("8330b8d3F5E0322E44cd1A9D9c29c61366d74606").unwrap();
    let classify_both = vec![contract_address.clone(), non_contract_address.clone()];
    let some_valid_block = H256::from_str("2426f0c34c043ac4bde36ee4886ba6c70dcb5cafc570e75ed3aa7a3cdcf2fcea").unwrap();
    let some_valid_block_num = 4737415u64;
    let some_valid_blocks_non_sequential = vec![4000000_u64, 4500000_u64, 5000001_u64, 3999999_u64];
    let some_token_address = H160::from_str("f230b790e05390fc8295f4d3f60332c93bed42e2").unwrap();
    let some_token_holder = H160::from_str("a18ff761a52ce1cb71ab9a19bf4e4b707b388b83").unwrap();

    let accounts = {
        match yum.accounts().wait() {
            Ok(accounts) => println!("yum.accounts() ... {}", Green.paint("passed")),
            Err(e) => println!("yum.accounts() ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let is_contract_true = {
        match yum.address_is_contract(&contract_address, &latest_block).wait() {
            Ok(true) => println!("yum.address_is_contract() [true] ... {}", Green.paint("passed")),
            Ok(false) => println!("yum.address_is_contract() [true] ... {}: returned false", Red.paint("failed")),
            Err(e) => println!("yum.address_is_contract() [true] ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let is_contract_false = {
        match yum.address_is_contract(&non_contract_address, &latest_block).wait() {
            Ok(false) => println!("yum.address_is_contract() [false] ... {}", Green.paint("passed")),
            Ok(true) => println!("yum.address_is_contract() [false] ... {}: returned true", Red.paint("failed")),
            Err(e) => println!("yum.address_is_contract() [false] ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let block_number = {
        match yum.block_number().wait() {
            Ok(bn) => println!("yum.block_number() ... {}: {}", Green.paint("passed"), bn),
            Err(e) => println!("yum.block_number() ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let classify_addresses = {
        match yum.classify_addresses(classify_both.iter().cloned().collect()).wait() {
            Ok(addrs) => println!("yum.classify_addresses() ... {}: {:?}", Green.paint("passed"), addrs),
            Err(e) => println!("yum.classify_addresses() ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let coinbase = {
        match yum.coinbase().wait() {
            Ok(cb) => println!("yum.coinbase() ... {}: {:?}", Green.paint("passed"), cb),
            Err(e) => println!("yum.coinbase() ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let gas_price = {
        match yum.gas_price().wait() {
            Ok(gp) => println!("yum.gas_price() ... {}: {:?}", Green.paint("passed"), gp),
            Err(e) => println!("yum.gas_price() ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let get_balance = {
        match yum.get_balance(&contract_address, &latest_block).wait() {
            Ok(x) => println!("yum.get_balance() ... {}: {:?}", Green.paint("passed"), x),
            Err(e) => println!("yum.get_balance() ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let get_block_by_hash = {
        match yum.get_block_by_hash(&some_valid_block, true).wait() {
            Ok(_) => println!("yum.get_block_by_hash() ... {}", Green.paint("passed")),
            Err(e) => println!("yum.get_block_by_hash() ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let get_block_by_number = {
        match yum.get_block_by_number(some_valid_block_num, true).wait() {
            Ok(_) => println!("yum.get_block_by_number() ... {}", Green.paint("passed")),
            Err(e) => println!("yum.get_block_by_number() ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let get_blocks_non_seq = {
        match yum.get_blocks(&some_valid_blocks_non_sequential, true).wait() {
            Ok(blocks) => {
                if blocks.len() == 4 {
                    println!("yum.get_blocks() [non-sequential] ... {}", Green.paint("passed"))
                } else {
                    println!("yum.get_blocks() [non-sequential] ... {}: expected 4 blocks, got {}", Red.paint("failed"), blocks.len())
                }
            },
            Err(e) => println!("yum.get_blocks() [non-sequential] ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let get_block_range = {
        match yum.get_block_range(5000000, 5000009, true).wait() {
            Ok(blocks) => {
                if blocks.len() == 10 {
                    println!("yum.get_block_range() [count: 10] ... {}", Green.paint("passed"))
                } else {
                    println!("yum.get_block_range() [count: 10] ... {}: expected 10, got {}", Red.paint("failed"), blocks.len())
                }

            },
            Err(e) => println!("yum.get_block_range() [count: 10] ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let get_code = {
        match yum.get_code(&contract_address, &latest_block).wait() {
            Ok(_) => println!("yum.get_code() ... {}", Green.paint("passed")),
            Err(e) => println!("yum.get_code() ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let get_token_decimals = {
        match yum.token_decimals(&some_token_address).wait() {
            Ok(d) => {
                if d == 6 {
                    println!("yum.token_decimals() [decimals: 6] ... {}", Green.paint("passed"))
                } else {
                    println!("yum.token_decimals() [decimals: 6] ... {}: expected 6, got {}", Green.paint("passed"), d)
                }
            },
            Err(e) => println!("yum.token_decimals() ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let get_token_name = {
        match yum.token_name(&some_token_address).wait() {
            Ok(s) => {
                if s == "Tronix".to_string() {
                    println!("yum.token_name() [name: Tronix] ... {}", Green.paint("passed"))
                } else {
                    println!("yum.token_name() [name: Tronix] ... {}: expected Tronix, got {}", Red.paint("failed"), s)
                }
            },
            Err(e) => println!("yum.token_name() ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let get_token_symbol = {
        match yum.token_symbol(&some_token_address).wait() {
            Ok(s) => {
                if s == "TRX".to_string() {
                    println!("yum.token_symbol() [TRX] ... {}", Green.paint("passed"))
                } else {
                    println!("yum.token_symbol() [TRX] ... {}: expected TRX, got {}", Red.paint("failed"), s)
                }
            },
            Err(e) => println!("yum.token_symbol() ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    let get_token_balance = {
        match yum.token_get_balance(&some_token_address, &some_token_holder, Some(latest_block)).wait() {
            Ok(x) => println!("yum.token_get_balance() ... {}: {:?}", Green.paint("passed"), x),
            Err(e) => println!("yum.token_get_balance() ... {}: {:?}", Red.paint("failed"), e)
        }
    };

    std::process::exit(1);
}