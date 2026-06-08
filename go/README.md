# Block DB Tools

This package provides a binary providing various Go based utilities for
performing operations on Block Databases.

Currently, only two operations are supported:
- Block Hash Verification: verify the blocks stored in the database, by
  comparing the hash of a block with the parent hash stored in the next block.
- Block Replay: re-execute the blocks using the Sonic client and verify that
  the computed blocks match the stored ones.

## Build

Install dependencies

```sh
sudo apt-get install librocksdb-dev protobuf-compiler
go install google.golang.org/protobuf/cmd/protoc-gen-go@latest
```

>   Note:
    The [Go bindings for RocksDB](https://github.com/linxGnu/grocksdb) support different versions of RocksDB, however the version of the bindings has to be compatible with that of RocksDB.
    Currently the bindings version is pinned to `v1.8.12` which works with RocksDB version 8 (Ubuntu 24.04 ships RocksDB 8.9).

## Block Hash Verification

To verify the hashes of the blocks in the database, run the following
command:
```
go run . verify -db <path-to-database>
```

By default, all blocks in the DB are verified. To focus on a sub-range the flags
`-s` and `-e` can be used to define the start and end of the targeted range. For
instance,
```
go run . verify -db <path-to-database> -s 10_000 -e 10_500
```
validates the blocks between block number 10000 and 10500.

For more details and additional options see the command help.
```
go run . verify --help
```

## Block Replay

To replay the blocks stored in the database, run the following command:
```
go run . replay -db <path-to-database> --json-genesis <json-genesis-file>
```
The `sonic.json` genesis file can be obtained from the Sonic Genesis File [web
page](https://genesis.soniclabs.com/).

For more details and additional options see the command help.
```
go run . replay --help
```

## Testing 

To run all tests in this package, use the standard Go command
```
go test ./...
```

## Profiling With Tracy

Bertha's replay command includes instrumentation for the
[Tracy](https://github.com/wolfpld/tracy) frame profiler. By default, these
instrumentation codes are disabled.

To enable instrumentation, the following steps are required:
- check-out Sonic's [tracy](git@github.com:0xsoniclabs/tracy.git) binding library in a new directory
  - use `--recursive` to check out sub-directories or run `git submodule update --init --recursive`
  after cloning the library
  - build the Tracy-Client shared library using `make` in the tracy project
- add a replace to Bertha's [go.mod](./go.mod) file to use the manually build `tracy` package
instead of the one retrieved from github; (e.g. `replace github.com/0xsoniclabs/tracy => ../../tracy`)
- build Bertha's Go commands with the `enable_tracy` tag. For instance, to run the replay command with instrumentation enabled, the
following command can be used:
```
go run --tags enable_tracy . replay -g sonic.json -db ../.blockdb -e 100
```

This setup is automated in [go-run-with-carmen-and-tracy.sh](./go-run-with-carmen-and-tracy.sh) which also does the setup to integrate carmen's tracy instrumentation listed below.
For instance, to run the replay command, the following command can be used:
```
./go-run-with-carmen-and-tracy.sh . replay -g sonic.json -db ../.blockdb -e 100
```

### Integrate Carmen's Tracy Instrumentation
To integrate support for Carmen's Rust based DB implementations and their
instrumentation, Carmen's Rust libraries need to be build using an external
tracy client library. To do so, define the environment variables `TRACY_CLIENT_LIB`
and `TRACY_CLIENT_LIB_PATH` as follows while building Carmen's Rust library:
```
TRACY_CLIENT_LIB=TracyClient TRACY_CLIENT_LIB_PATH=<path-to-tracy-project>/tracy/build cargo build --release --features tracy
```

After replacing Carmen in Bertha's [go.mod](./../../go.mod) file with the manually
build version of Carmen and importing the experimental package in [state.go](./app/state.go), 
the following command can be used to replay blocks with enabled Tracy support:
```
go run --tags enable_tracy . replay -g sonic.json -db ../.blockdb -e 100 --db-schema 6 --db-variant rust-memory
```
