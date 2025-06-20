use std::{fmt::Write, fs::File, io::BufReader, path::Path};

use blockservice::{Error, blockdb, blockdb::BlockDb};
use clap::Parser;
use genesis_parser::Genesis;
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use prost::Message;

use crate::cli::Args;

mod cli;

fn init(path: Option<impl AsRef<Path>>) -> Result<(), Error> {
    let path = path
        .map(|p| p.as_ref().to_path_buf())
        .unwrap_or_else(|| Path::new("./").to_path_buf());
    let path = path.canonicalize().map_err(|_| Error::Io)?;
    println!("Initializing new block database at: {}", path.display());
    blockdb::RocksBlockDb::create(format!("{}/.blockdb", path.display()))?;
    Ok(())
}

fn import(path: impl AsRef<Path>) -> Result<(), Error> {
    let mut rb = blockdb::RocksBlockDb::open("./.blockdb")?;

    let file = File::open(path).unwrap();
    let mut reader = BufReader::new(file);
    let mut genesis = Genesis::parse(&mut reader).unwrap();
    let chain_id = genesis.chain_id();
    let mut blocks = genesis.blocks();

    let mut uncompressed_bytes_written = 0;
    let mut block_count = 0;
    let mut total_blocks;
    let progress_bar = ProgressBar::new(1);
    progress_bar.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} (ETA {eta})",
        )
        .unwrap()
        .with_key("eta", |state: &ProgressState, w: &mut dyn Write| {
            write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap()
        })
        .progress_chars("#>-"),
    );
    let before = std::time::Instant::now();
    while let Some(Ok(block)) = blocks.next() {
        if block_count == 0 {
            total_blocks = block.number + 1;
            println!("Importing {total_blocks} blocks for chain ID {chain_id}");
            progress_bar.set_length(total_blocks);
        }

        // We use put_raw so we can count bytes.
        // TODO: Have some kind of stats interface on BlockDb?
        let number = block.number;
        let protoblock = blockservice::proto::Block::from(block).encode_to_vec();
        uncompressed_bytes_written += protoblock.len();
        rb.put_raw(0, number, &protoblock).unwrap();
        block_count += 1;
        progress_bar.set_position(block_count);
    }
    let elapsed = before.elapsed();
    progress_bar.finish();
    println!(
        "Wrote {} blocks, total uncompressed size: {} MiB, elapsed: {}s, throughput: {:.1} MiB/s",
        block_count,
        uncompressed_bytes_written / (1024 * 1024),
        elapsed.as_secs(),
        uncompressed_bytes_written as f64 / (1024.0 * 1024.0) / elapsed.as_secs_f64()
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
