# Block DB Go Tools

## Build

Install dependencies

```sh
sudo apt-get install librocksdb-dev protobuf-compiler
go install google.golang.org/protobuf/cmd/protoc-gen-go@latest
```

>   Note:
    The [Go bindings for RocksDB](https://github.com/linxGnu/grocksdb) support different versions of RocksDB, however the version of the bindings has to be compatible with that of RocksDB.
    Currently the bindings version is pinned to `v1.8.12` which works with RocksDB version 8 (Ubuntu 24.04 ships RocksDB 8.9).
