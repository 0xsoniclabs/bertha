use std::{
    collections::HashMap,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    path::Path,
};

use blockservice::{
    app_dir::open_app_dir,
    cli::{Args, Command},
    cmd,
};
use clap::Parser;
use tokio::signal;
use tokio_util::sync::CancellationToken;

async fn execute(
    args: Args,
    cancellation_token: CancellationToken,
) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        Command::Init => cmd::init(args.dir, std::io::stdout()),
        Command::Import {
            snapshot_file,
            verify,
        } => cmd::import(args.dir, snapshot_file, verify, std::io::stdout()),
        Command::List { chain_id, url } => {
            cmd::list(args.dir, chain_id, url, std::io::stdout()).await
        }
        Command::Fetch {
            url,
            chain_id,
            from,
            to,
        } => cmd::fetch(args.dir, url, chain_id, from, to, std::io::stdout()).await,
        Command::Purge { chain_id, from, to } => cmd::purge(
            args.dir,
            chain_id,
            from,
            to,
            std::io::stdout(),
            std::io::stdin().lock(),
        ),
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
        Command::FetchStateUpdates { url, chain_id } => {
            cmd::fetch_state_updates(args.dir, url, chain_id, std::io::stdout()).await
        }
        Command::Start => {
            let port = {
                let app_dir = Path::new("./").canonicalize()?;
                let (cfg, _db) = open_app_dir(app_dir, true)?;
                cfg.get_port()
            };

            // TODO Get JSON-RPC servers from config
            let json_rpc_config = HashMap::new();

            let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port);
            let listener = tokio::net::TcpListener::bind(addr).await?;
            cmd::start(
                args.dir,
                listener,
                json_rpc_config,
                cancellation_token,
                None,
            )
            .await
        }
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let cancellation_token = CancellationToken::new();

    tokio::spawn({
        let cancellation_token = cancellation_token.clone();
        async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => {},
                result = execute(args, cancellation_token.clone()) =>{
                    if let Err(e) = result {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                    cancellation_token.cancel();
                }
            }
        }
    });

    tokio::select! {
        _ = cancellation_token.cancelled() => {},
        result = signal::ctrl_c() => {
            if result.is_err() {
                eprintln!("failed to install Ctrl+C handler");
                std::process::exit(1);
            }
            println!("\nReceived Ctrl+C, shutting down...");
            cancellation_token.cancel();
        }
    }
}
