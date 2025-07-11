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
go run ./cmd/block-db verify -db <path-to-database>
```

By default, all blocks in the DB are verified. To focus on a sub-range the flags
`-s` and `-e` can be used to define the start and end of the targeted range. For
instance,
```
go run ./cmd/block-db verify -db <path-to-database> -s 10_000 -e 10_500
```
validates the blocks between block number 10000 and 10500.

For more details and additional options see the command help.
```
go run ./cmd/block-db
```

## State Root Verification

To verify all state roots listed in blocks, and thus the ability to reproduce
the history of the chain from the content of the blocks, run the following 
command:
```
go run ./cmd/block-db replay -db <path-to-database> --json-genesis <json-genesis-file>
```
The `sonic.json` genesis file can be obtained from the Sonic Genesis File [web
page](https://genesis.soniclabs.com/).

## Testing ##

To run all tests in this package, use the standard Go command
```
go test ./...
```