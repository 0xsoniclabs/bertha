# Bertha Block Service and Tools

The bertha projects contains testing infrastructure for the Sonic ecosystem.
It consists of the [Rust Block Service](blockservice/README.md) and the [Go Block DB Tools](go/README.md).

The block service allows to manage, transfer and retrieve block datasets for Ethereum-compatible blockchains in bulk.
This data is stored in a local database.
The Go DB tools use this data to replay the blockchain history.

This repository stores large data files (e.g. the failed bundles in `data/failed_bundles/`) with [Git LFS](https://git-lfs.com/).
A checkout without Git LFS contains only pointer files; fetch the actual files with `git lfs pull`.
