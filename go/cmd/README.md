# Block DB utility

This package provides a binary providing various Go based utilities for 
performing operations on Block Databases.

This project is in an early stage. Right now, only two operations are supported:
- the verification of the hashes of blocks in a given database
- the verification of the state root hashes in blocks in a given database


## Block Hash Verification
To verify the hashes of the blocks in the database, run the following
command:
```
go run ./cmd verify -db <path-to-database>
```

By default, all blocks in the DB are verified. To focus on a sub-range the flags
`-s` and `-e` can be used to define the start and end of the targeted range. For
instance,
```
go run ./cmd verify -db <path-to-database> -s 10_000 -e 10_500
```
validates the blocks between block number 10000 and 10500.

For more details and additional options see the command help.
```
go run ./cmd
```

## State Root Verification

To verify all state roots listed in blocks, and thus the ability to reproduce
the history of the chain from the content of the blocks, run the following
command:
```
go run ./cmd replay -db <path-to-database> --json-genesis <json-genesis-file>
```
The `sonic.json` genesis file can be obtained from the Sonic Genesis File [web
page](https://genesis.soniclabs.com/).

## Testing ##

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
  - use `--recursive` to check out sub-directories or run `go submodule update --init --recursive`
  after cloning the library
  - build the Tracy-Client shared library using `make` in the tracy project
- add a replace to Bertha's [go.mod](./../../go.mod) file to use the manually build `tracy` package
instead of the one retrieved from github; (e.g. `replace github.com/0xsoniclabs/tracy => ../../tracy`)
- build Bertha's Go commands with the `enable_tracy` tag. For instance, to run the replay command with instrumentation enabled, the
following command can be used:
```
go run --tags enable_tracy ./cmd replay -g sonic.json -db ../.blockdb -e 100
```

This setup is automated in [go-run-with-tracy.sh](../../go-run-with-tracy.sh) which also does the setup to integrate carmen's tracy instrumentation listed below.
For instance, to run the replay command, the following command can be used:
```
./go-run-with-tracy ./cmd replay -g sonic.json -db ../.blockdb -e 100
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
go run --tags enable_tracy ./cmd replay -g sonic.json -db ../.blockdb -e 100 --db-schema 6 --db-variant rust-memory
```
