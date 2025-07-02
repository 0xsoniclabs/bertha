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
        Command::Clean => todo!(),
        Command::View {
            chain_id,
            block_number,
        } => cmd::view(chain_id, block_number, std::io::stdout()),
        Command::Start { port } => cmd::start(port).await,
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    execute(args).await.unwrap();
}
