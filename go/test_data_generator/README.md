# Test data generator

Test data generator in GO for Rust.

## How does it work

The generator produces a test case for every "corner case" of a type. A "corner case" is either:

- An integral type (min, min + 1, max, max - 1, random value)
- A vector (empty, non-empty)
- Transaction types
- An inner struct

It generates `max_len(type_field_cases)` values by scanning the field sequentially. The exhausted fields are default initialized.
E.g. for a struct `foo{bar int, baz int}` and cases `bar: [1,2], baz: [3,4,5]`, the following cases are generated: `foo{bar: 1, baz: 3}, foo{bar: 2, baz: 4}, foo{bar: 0, baz: 6}`

## Additional notes  

Under the `go/test_data_generator/geth_files` a collection of Geth files have been copied to bypass the internal protection or to adapt the marshalling functions

## Usage

```bash
go run . [transactions | receipts | blocks] > your_file.rs
```
