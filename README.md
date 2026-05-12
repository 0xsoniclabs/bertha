# Bertha Block Service and Tools

The bertha projects contains testing infrastructure for the Sonic ecosystem.
It consists of the [Rust Block Service](blockservice/README.md) and the [Go Block DB Tools](go/README.md).

The block service allows to manage, transfer and retrieve block datasets for Ethereum-compatible blockchains in bulk.
This data is stored in a local database.
The Go DB tools use this data to replay the blockchain history.
