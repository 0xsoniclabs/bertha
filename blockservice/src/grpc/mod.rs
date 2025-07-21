mod client;
pub mod proto_rpc;
mod server;
#[cfg(test)]
pub mod test_utils;

pub use client::RpcClient;
pub use server::RpcServer;

const GRPC_COMPRESSION_ALGORITHM: tonic::codec::CompressionEncoding =
    tonic::codec::CompressionEncoding::Gzip;
