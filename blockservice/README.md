# Block Service

The block service allows to manage, transfer and retrieve block datasets for Ethereum-compatible blockchains in bulk.

## Dependencies

- The [`protoc` protobuf compiler](https://protobuf.dev/installation/) (tested with version 3.21.12, the default on Ubuntu 24.04)
- [RocksDB](https://rocksdb.org/) (tested with version 8.9, the default on Ubuntu 24.04)
- [Clang](https://clang.llvm.org/get_started.html) for generating the RocksDB Rust bindings

## Installation

On Ubuntu 24.04, run

```
sudo apt-get install librocksdb-dev protobuf-compiler clang
```

## Usage

```sh
$ cargo run --release -- --help
Block Service

Usage: blockservice [OPTIONS] <COMMAND>

Commands:
  init                 Initialize a new block database
  import               Import all blocks from the specified snapshot (`.g`) file into the block database, and optionally also verify the parent hashes
  fetch                Fetch blocks from a remote block service and store them in the local database
  fetch-state-updates  Fetch state update files from a remote block service
  list                 List all block ranges for all chains or only for the specific chain if specified. If url is not set this lists the locally stored block ranges, otherwise the block ranges of the remote block service
  verify               Check that all parent hashes match the hash of the parent block starting from the specified block number with the specified block hash
  purge                Delete all blocks of the specified chain, optionally restricted to the range from `from` to `to`
  view                 Print the block as JSON
  start                Start the block server
  help                 Print this message or the help of the given subcommand(s)

Options:
      --dir <DIR>  The path to the blockservice directory [default: .]
  -h, --help       Print help
```

Subcommand usage (e.g. for `init`)
```sh
$ cargo run --release -- init --help
```

**Note: by default, all commands assume that the db directory (`.blockdb`) is in the current directory, however you can specify a different path using `--path PATH`.**

## Running

Create a new block database

```sh
cargo run --release -- init
```

Import a Sonic genesis snapshot

```sh
cargo run --release -- import </path/to/snapshot.g>
```

Start the gRPC server (by default the port is 8080)

```sh
cargo run --release -- start [PORT]
```

Fetch blocks from a gRPC server

```sh
cargo run --release -- fetch 8080
```
