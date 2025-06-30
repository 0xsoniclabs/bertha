use clap::Parser;

use crate::cli::{Args, Command};

mod cli;
mod cmd;

const BLOCK_DB_NAME: &str = ".blockdb";

fn execute(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        Command::Init { path } => cmd::init(path),
        Command::Import { snapshot_file } => cmd::import(snapshot_file),
        Command::List { chain_id: _ } => todo!(),
        Command::Verify {
            chain_id,
            block_number,
            block_hash,
        } => cmd::verify(chain_id, block_number, block_hash, std::io::stdout()),
        Command::Purge { chain_id, from, to } => cmd::purge(chain_id, from, to),
        Command::Clean => todo!(),
        Command::Start => todo!(),
    }
}

fn main() {
    let args = Args::parse();
    execute(args).unwrap();
}
