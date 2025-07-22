use std::{
    collections::HashMap,
    net::{IpAddr, Ipv6Addr, SocketAddr},
};

use blockservice::{
    cli::{Args, Command},
    cmd,
};
use clap::Parser;

async fn execute(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        Command::Init => cmd::init(args.dir),
        Command::Import {
            snapshot_file,
            verify,
        } => cmd::import(args.dir, snapshot_file, verify),
        Command::List { chain_id, url } => {
            cmd::list(args.dir, chain_id, url, std::io::stdout()).await
        }
        Command::Fetch {
            url,
            chain_id,
            from,
            to,
        } => cmd::fetch(args.dir, url, chain_id, from, to, std::io::stdout()).await,
        Command::Purge { chain_id, from, to } => cmd::purge(args.dir, chain_id, from, to),
        Command::Verify {
            chain_id,
            block_number,
            block_hash,
        } => cmd::verify(
            args.dir,
            chain_id,
            block_number,
            block_hash,
            std::io::stdout(),
        ),
        Command::View {
            chain_id,
            block_number,
        } => cmd::view(args.dir, chain_id, block_number, std::io::stdout()),
        Command::Start { port } => {
            // TODO read config
            let config = HashMap::new();
            // This allows both IPv4 and IPv6 connections
            let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port);
            let listener = tokio::net::TcpListener::bind(addr).await?;
            cmd::start(args.dir, listener, config).await
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
