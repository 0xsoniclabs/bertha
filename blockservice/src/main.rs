use std::{fs::File, io::BufReader, path::Path};

use blockservice::{Error, blockdb, blockdb::BlockDb};
use clap::Parser;
use genesis_parser::Genesis;
use prost::Message;

use crate::cli::Args;

mod cli;

fn init(path: Option<impl AsRef<Path>>) -> Result<(), Error> {
    let path = path
        .map(|p| p.as_ref().to_path_buf())
        .unwrap_or_else(|| Path::new("./").to_path_buf());
    println!("Initializing block database at: {}", path.display());
    blockdb::RocksBlockDb::create(format!("{}/.blockdb", path.display()))?;
    Ok(())
}

fn import(path: impl AsRef<Path>) -> Result<(), Error> {
    let mut rb = blockdb::RocksBlockDb::open("./.blockdb")?;

    let file = File::open(path).unwrap();
    let mut reader = BufReader::new(file);
    let mut genesis = Genesis::parse(&mut reader).unwrap();
    let mut blocks = genesis.blocks();

    let mut total_bytes = 0;
    let mut block_count = 0;
    let before = std::time::Instant::now();
    while let Some(Ok(block)) = blocks.next() {
        // We use put_raw so we can count bytes.
        // TODO: Have some kind of stats interface on BlockDb?
        let number = block.number;
        let protoblock = blockservice::proto::Block::from(block).encode_to_vec();
        total_bytes += protoblock.len();
        rb.put_raw(0, number, &protoblock).unwrap();
        block_count += 1;
    }
    let elapsed = before.elapsed();
    println!(
        "Wrote {} blocks, total size: {} MiB, elapsed: {}s, throughput: {:.1} MiB/s",
        block_count,
        total_bytes / (1024 * 1024),
        elapsed.as_secs(),
        total_bytes as f64 / (1024.0 * 1024.0) / elapsed.as_secs_f64()
    );

    Ok(())
}

fn execute(args: Args) -> Result<(), Error> {
    match args.command {
        cli::Command::Init { path } => init(path),
        cli::Command::Import { snapshot_file } => import(snapshot_file),
        cli::Command::List { chain_id: _ } => todo!(),
        cli::Command::Verify {
            chain_id: _,
            block_number: _,
            block_hash: _,
        } => todo!(),
        cli::Command::Purge {
            chain_id: _,
            from: _,
            to: _,
        } => todo!(),
        cli::Command::Clean => todo!(),
        cli::Command::Start => todo!(),
    }
}

fn main() {
    let args = Args::parse();
    execute(args).unwrap();
}
