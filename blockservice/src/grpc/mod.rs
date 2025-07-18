mod client;
pub mod proto_rpc;
mod server;
#[cfg(test)]
pub mod test_utils;

pub use client::RpcClient;
pub use server::RpcServer;
use tonic::codec::CompressionEncoding;

const GRPC_COMPRESSION_ALGORITHM: CompressionEncoding = CompressionEncoding::Zstd;
