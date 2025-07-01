# Block DB utility

This package provides a binary providing various Go based utilities for 
performing operations on Block Databases.

This project is in an early stage. Right now, only one operation is supported:
the verification of blocks in a given database. To do so, run the following
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

## Testing ##

To run all tests in this package, use the standard Go command
```
go test ./...
```