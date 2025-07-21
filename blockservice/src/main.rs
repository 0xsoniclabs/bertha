use std::{
    collections::HashMap,
    net::{IpAddr, Ipv6Addr, SocketAddr},
};

use clap::Parser;

use crate::cli::{Args, Command};

mod cli;

use blockservice::cmd;

async fn execute(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        Command::Init { path } => cmd::init(path),
        Command::Import {
            snapshot_file,
            verify,
        } => cmd::import(snapshot_file, verify),
        Command::List { chain_id } => cmd::list(chain_id, std::io::stdout()),
        Command::Fetch {
            url,
            chain_id,
            from,
            to,
        } => cmd::fetch(url, chain_id, from, to, std::io::stdout()).await,
        Command::Purge { chain_id, from, to } => cmd::purge(chain_id, from, to),
        Command::Verify {
            chain_id,
            block_number,
            block_hash,
        } => cmd::verify(chain_id, block_number, block_hash, std::io::stdout()),
        Command::View {
            chain_id,
            block_number,
        } => cmd::view(chain_id, block_number, std::io::stdout()),
        Command::Start { port } => {
            // TODO read config
            let config = HashMap::new();
            // This allows both IPv4 and IPv6 connections
            let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port);
            let listener = tokio::net::TcpListener::bind(addr).await?;
            cmd::start(listener, config).await
        }
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let result = execute(args).await;
    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
