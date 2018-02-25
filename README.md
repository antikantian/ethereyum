# ethereyum [![Build Status](https://travis-ci.org/etherswap/ethereyum.svg?branch=master)](https://travis-ci.org/etherswap/ethereyum) [![Coverage Status](https://coveralls.io/repos/github/etherswap/ethereyum/badge.svg?branch=master)](https://coveralls.io/github/etherswap/ethereyum?branch=master) [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
> 'yum': because I was hungry at the time.

This library is a work-in-progress.  Although there is another web3 library for rust, [rust-web3](https://github.com/tomusdrw/rust-web3),
it doesn't allow for websocket connections, only ipc and http.  Some preliminary benchmarks over 1,000,000 `eth_getBlock` requests:

![ethereyum-benchmark](/static/ethereyum-benchmark.png)
