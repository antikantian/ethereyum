# ethereyum [![Build Status](https://travis-ci.org/etherswap/ethereyum.svg?branch=master)](https://travis-ci.org/etherswap/ethereyum)
> 'yum': because I was hungry at the time.

This library is a work-in-progress.  Although there is another web3 library for rust, [rust-web3](https://github.com/tomusdrw/rust-web3),
it doesn't allow for websocket connections, only ipc and http.  Some preliminary benchmarks over 1,000,000 `eth_getBlock` requests:

![ethereyum-benchmark](/static/ethereyum-benchmark.png)
