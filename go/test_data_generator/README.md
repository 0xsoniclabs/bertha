# Test data generator

Test data generator in GO for Rust.

## How does it work

The generator produces a test case for every "corner case" of a type. A "corner case" is either:

- A vector (empty, non-empty)
- Transaction types
- An inner struct

For the inner struct case, this is recursively applied (e.g. in a `Transaction`, a corner case is generated for each corner case of an `AccessList`)

## Additional notes  

Under the `go/test_data_generator/geth_files` a collection of Geth files have been copied to bypass the internal protection or to adapt the marshalling functions

## Usage

```bash
go run . [transactions | receipts | blocks] > your_file.rs
```
