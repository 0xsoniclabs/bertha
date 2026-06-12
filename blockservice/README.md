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
  init                    Initialize a new block database
  import-gfile            Import all blocks from the specified snapshot (`.g`) file into the block database, and optionally also verify the parent hashes
  import-era1             Import all blocks from the specified directory (which is expected to contain `.era1` files) into the block database, and optionally also verify the parent hashes. The blocks are stored under the specified chain ID
  import-era              Import all blocks from the specified directory (which is expected to contain `.era` files) into the block database. The blocks are stored under the specified chain ID
  import-upgrade-heights  Import upgrade heights from a JSON file into the block database for the specified chain ID
  import-corrections      Import corrections from a JSON file into the block database for the specified chain ID
  fetch                   Fetch blocks from a remote block service and store them in the local database
  fetch-metadata          Fetch metadata (upgrade heights and corrections) from a remote block service and store them in the local database
  list                    List all block ranges for all chains or only for the specific chain if specified. If url is not set this lists the locally stored block ranges, otherwise the block ranges of the remote block service
  verify                  Check that all parent hashes match the hash of the parent block starting from the specified block number with the specified block hash
  purge                   Delete all blocks of the specified chain, optionally restricted to the range from `from` to `to`
  view                    Print the block as JSON
  view-upgrade-heights    Print the upgrade heights stored in the block database for the specified chain ID
  view-corrections        Print the corrections stored in the block database for the specified chain ID
  start                   Start the block server
  help                    Print this message or the help of the given subcommand(s)

Options:
      --dir <DIR>  The path to the blockservice directory [default: .]
  -h, --help       Print help
```

Subcommand usage (e.g. for `init`)
```sh
$ cargo run --release -- init --help
```

**Note: by default, all commands assume that the db directory (called `.blockdb`) is in the current directory, however you can specify a different path using `--dir <DIR>`.**

## Running

Create a new block database

```sh
cargo run --release -- init
```

Import a Sonic `.g` file

```sh
cargo run --release -- import-gfile </path/to/snapshot.g>
```

Import Ethereum `.era1` and `.era` files

`.era1` files store pre-merge Ethereum history, while `.era` files store post-merge Ethereum beacon chain history.
Thus, `.era1` files contain all data that is stored in bertha, while `.era` files are missing transaction receipts.
Therefore, `.era` file import does not support parent hash verification, because the computed parent hash will be always incorrect because of the missing receipts.

*Note: First import the `.era1` files and then the `.era` files.*

```sh
cargo run --release -- import-era1 </path/to/era1_directory> [--verify]
cargo run --release -- import-era </path/to/era_directory>
```

Import upgrade heights

*Note: New upgrade heights can be detected and applied automatically for Sonic chains. The import is not mandatory but can be used to verify that the detected upgrade heights match the stored ones.*

```sh
cargo run --release -- import-upgrade-heights <chain-id> </path/to/upgrade-heights.json>
```

Import corrections

*Note: Corrections are not needed for all chains.*

```sh
cargo run --release -- import-corrections <chain-id> </path/to/corrections.json>
```

Start the gRPC server (by default the port is 8080, configured in `blockservice.toml`)

```sh
cargo run --release -- start
```

Fetch blocks from a gRPC server

```sh
cargo run --release -- fetch <url> <chain_id>
```
