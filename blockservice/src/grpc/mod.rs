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
