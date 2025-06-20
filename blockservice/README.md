# Block Service

The block service allows manage, transfer and retrieve block datasets for Ethereum-compatible blockchains in bulk.

## Dependencies

- The [`protoc` protobuf compiler](https://protobuf.dev/installation/) (tested with version 3.21.12, the default on Ubuntu 24.04)
- [RocksDB](https://rocksdb.org/) (tested with version 8.9, the default on Ubuntu 24.04)

## Installation

On Ubuntu 24.04, run

```
sudo apt-get install librocksdb-dev protobuf-compiler
```

## Running

Create a new block database

```
cargo run --release -- init ./
```

Import a Sonic genesis snapshot

```
cargo run --release -- import /path/to/snapshot.g
```
