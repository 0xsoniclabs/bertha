// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

pub mod auth;
mod client;
pub mod proto_rpc;
mod server;
#[cfg(test)]
pub mod test_utils;

pub use client::RpcClient;
pub use server::RpcServer;

/// The compression algorithm used for gRPC messages.
const GRPC_COMPRESSION_ALGORITHM: tonic::codec::CompressionEncoding =
    tonic::codec::CompressionEncoding::Zstd;
