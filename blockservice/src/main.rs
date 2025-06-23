use std::{fmt::Write, fs::File, io::BufReader, path::Path};

use blockservice::{blockdb, blockdb::BlockDb};
use clap::Parser;
use genesis_parser::Genesis;
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use prost::Message;

use crate::cli::Args;

mod cli;

const BLOCK_DB_NAME: &str = ".blockdb";

fn init(path: Option<impl AsRef<Path>>) -> Result<(), Box<dyn std::error::Error>> {
    let path = path
        .map(|p| p.as_ref().to_path_buf())
        .unwrap_or_else(|| Path::new("./").to_path_buf());
    let path = path.canonicalize()?;
    let path = path.join(BLOCK_DB_NAME).as_path().to_path_buf();
    println!("Initializing new block database at: {}", path.display());
    blockdb::RocksBlockDb::create(path)?;
    Ok(())
}

fn import(path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = Path::new("./").join(BLOCK_DB_NAME).canonicalize()?;
    let mut db = blockdb::RocksBlockDb::open(db_path)?;

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut genesis = Genesis::parse(&mut reader)?;
    let chain_id = genesis.chain_id();
    let mut blocks = genesis.blocks();

    let mut uncompressed_bytes_written = 0;
    let mut block_count = 0;
    let mut total_blocks;
    let progress_bar = ProgressBar::new(1);
    progress_bar.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} (ETA {eta})",
        )?
        .with_key("eta", |state: &ProgressState, w: &mut dyn Write| {
            // Since there is no way of propagating errors from this closure,
            // we just ignore the result (worst case the ETA will not be shown).
            let _ = write!(w, "{:.1}s", state.eta().as_secs_f64());
        })
        .progress_chars("#>-"),
    );
    let before = std::time::Instant::now();
    while let Some(Ok(block)) = blocks.next() {
        if block_count == 0 {
            // We rely on the fact that blocks are stored in reverse order.
            total_blocks = block.number + 1;
            println!("Importing {total_blocks} blocks for chain ID {chain_id}");
            progress_bar.set_length(total_blocks);
        }

        // We use put_raw so we can count bytes.
        let number = block.number;
        let protoblock = blockservice::proto::Block::from(block).encode_to_vec();
        uncompressed_bytes_written += protoblock.len();
        db.put_raw(chain_id, number, &protoblock)?;
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

fn execute(args: Args) -> Result<(), Box<dyn std::error::Error>> {
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

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    fn init_creates_db() {
        // No args: Create in current working directory
        {
            let tmpdir = tempfile::tempdir().unwrap();
            env::set_current_dir(tmpdir.path()).unwrap();
            let args = Args::parse_from(["blockservice", "init"]);
            execute(args).unwrap();
            assert!(tmpdir.path().join(BLOCK_DB_NAME).exists());
        }

        // Optional arg: Create in specified directory
        {
            let tmpdir = tempfile::tempdir().unwrap();
            let args = Args::parse_from(["blockservice", "init", tmpdir.path().to_str().unwrap()]);
            execute(args).unwrap();
            assert!(Path::new(&format!("{}/.blockdb", tmpdir.path().display())).exists());
        }
    }

    #[test]
    fn import_inserts_all_blocks_from_snapshot_file_into_db() {
        let tmpdir = tempfile::tempdir().unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");
        let num_blocks = 5;
        let chain_id = 62;
        let genesis_data = genesis_parser::test_utils::generate_test_genesis(chain_id, num_blocks);
        std::fs::write(&genesis_file, genesis_data).unwrap();

        env::set_current_dir(tmpdir.path()).unwrap();
        init(None::<&Path>).unwrap();
        let args = Args::parse_from(["blockservice", "import", genesis_file.to_str().unwrap()]);
        execute(args).unwrap();

        let db =
            blockdb::RocksBlockDb::open_for_reading(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
        for i in 0..num_blocks {
            let block = db.get(chain_id, i as u64).unwrap();
            assert!(block.is_some(), "Block {i} not found in the database");
        }
    }
}
