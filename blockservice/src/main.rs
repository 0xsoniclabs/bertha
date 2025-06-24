use std::{fmt::Write, fs::File, io::BufReader, path::Path};

use bertha_types::{Hash, HexConvert};
use blockservice::blockdb::{BlockDb, RocksBlockDb};
use clap::Parser;
use genesis_parser::Genesis;
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use prost::Message;

use crate::cli::{Args, Command};

mod cli;

const BLOCK_DB_NAME: &str = ".blockdb";

fn init(path: Option<impl AsRef<Path>>) -> Result<(), Box<dyn std::error::Error>> {
    let path = path
        .map(|p| p.as_ref().to_path_buf())
        .unwrap_or_else(|| Path::new("./").to_path_buf());
    let path = path.canonicalize()?;
    let path = path.join(BLOCK_DB_NAME);
    println!("Initializing new block database at: {}", path.display());
    RocksBlockDb::create(path)?;
    Ok(())
}

fn import(path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = Path::new("./").join(BLOCK_DB_NAME).canonicalize()?;
    let mut db = RocksBlockDb::open(db_path)?;

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut genesis = Genesis::parse(&mut reader)?;
    let chain_id = genesis.chain_id();
    let blocks = genesis.blocks();

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
    for result in blocks {
        let block = result?;
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

fn verify(
    chain_id: u64,
    block_number: Option<u64>,
    block_hash: Option<Hash>,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = Path::new("./").join(BLOCK_DB_NAME).canonicalize()?;
    let db = RocksBlockDb::open_for_reading(db_path)?;

    if let (Some(block_number), Some(expected_hash)) = (block_number, block_hash) {
        match db.get(chain_id, block_number)? {
            Some(block) => {
                let block_hash = block.to_header().compute_hash();
                if block_hash != expected_hash {
                    writeln!(
                        writer,
                        "[chain ID {}] block hash verification failed for block {}: expected hash {}, got {}.",
                        chain_id,
                        block_number,
                        expected_hash.to_hex(),
                        block_hash.to_hex()
                    )?;
                }
            }
            None => writeln!(
                writer,
                "[chain ID {chain_id}] requested block {block_number} does not exit"
            )?,
        }
    }

    // start with the first block if no block number is provided
    let block_number = block_number.unwrap_or_default();
    let mut prev_block_number = block_number;
    let mut prev_block_hash: Option<Hash> = None;
    for entry in db.iterate_with_key(chain_id, block_number) {
        let (block_number, block) = entry?;
        if block.number != block_number {
            writeln!(
                writer,
                "[chain ID {}] block number mismatch: block number in key = {}, block.number = {}.",
                chain_id, block_number, block.number
            )?;
        }
        if prev_block_number + 1 != block_number {
            prev_block_hash = None; // there was a gap so we have to skip the parent hash check
        }
        if let Some(prev_block_hash) = prev_block_hash {
            if block.parent_hash != prev_block_hash {
                writeln!(
                    writer,
                    "[chain ID {}] parent hash verification failed for block {}: expected hash {}, got {}.",
                    chain_id,
                    block_number,
                    prev_block_hash.to_hex(),
                    block.parent_hash.to_hex()
                )?;
            }
        }
        prev_block_number = block_number;
        prev_block_hash = Some(block.to_header().compute_hash());
    }

    Ok(())
}

fn execute(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        Command::Init { path } => init(path),
        Command::Import { snapshot_file } => import(snapshot_file),
        Command::List { chain_id: _ } => todo!(),
        Command::Verify {
            chain_id,
            block_number,
            block_hash,
        } => verify(chain_id, block_number, block_hash, std::io::stdout()),
        Command::Purge {
            chain_id: _,
            from: _,
            to: _,
        } => todo!(),
        Command::Clean => todo!(),
        Command::Start => todo!(),
    }
}

fn main() {
    let args = Args::parse();
    execute(args).unwrap();
}

#[cfg(test)]
mod tests {
    use std::{env, os::unix::fs::PermissionsExt, path::PathBuf};

    use bertha_types::Block;
    use blockservice::proto;

    use super::*;

    static WORKING_DIR_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A guard type to temporarily change the working directory while avoiding
    /// race-conditions across concurrently executed tests.
    struct ChangeWorkingDir<'a> {
        _guard: std::sync::MutexGuard<'a, ()>,
        prev: PathBuf,
    }

    impl ChangeWorkingDir<'_> {
        fn new(path: impl AsRef<Path>) -> Self {
            let guard = WORKING_DIR_MUTEX.lock().unwrap();
            let prev = env::current_dir().unwrap();
            env::set_current_dir(path).unwrap();
            ChangeWorkingDir {
                _guard: guard,
                prev,
            }
        }
    }

    impl Drop for ChangeWorkingDir<'_> {
        fn drop(&mut self) {
            env::set_current_dir(&self.prev).unwrap();
        }
    }

    #[test]
    fn init_creates_db() {
        // No args: Create in current working directory
        {
            let tmpdir = tempfile::tempdir().unwrap();
            let _cwd = ChangeWorkingDir::new(tmpdir.path());
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
    fn init_fails_if_db_already_exists() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        let args = Args::parse_from(["blockservice", "init"]);
        let result = execute(args);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("error_if_exists is true")
        );
    }

    #[test]
    fn init_fails_if_no_write_permissions() {
        let tmpdir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(tmpdir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();

        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        let args = Args::parse_from(["blockservice", "init"]);
        let result = execute(args);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
    }

    #[test]
    fn import_inserts_all_blocks_from_snapshot_file_into_db() {
        let tmpdir = tempfile::tempdir().unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");
        let num_blocks = 5;
        let chain_id = 62;
        let genesis_data =
            genesis_parser::test_utils::generate_test_genesis(chain_id, num_blocks, Vec::new());
        std::fs::write(&genesis_file, genesis_data).unwrap();

        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let args = Args::parse_from(["blockservice", "import", genesis_file.to_str().unwrap()]);
        execute(args).unwrap();

        let db = RocksBlockDb::open_for_reading(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
        for i in 0..num_blocks {
            let block = db.get(chain_id, i as u64).unwrap();
            assert!(block.is_some(), "Block {i} not found in the database");
        }
    }

    #[test]
    fn import_aborts_on_invalid_snapshot_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");
        let genesis_data = genesis_parser::test_utils::generate_test_genesis(0, 5, Vec::new());
        let data_len = genesis_data.len();
        let corruption = [0xde, 0xad, 0xbe, 0xef];
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        // Corrupted header
        {
            let mut genesis_data = genesis_data.clone();
            genesis_data[0..corruption.len()].copy_from_slice(&corruption); // Corrupt the first part of the file
            std::fs::write(&genesis_file, genesis_data).unwrap();

            let args = Args::parse_from(["blockservice", "import", genesis_file.to_str().unwrap()]);
            let result = execute(args);
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("invalid header"));
        }

        // Corrupted block
        {
            let mut genesis_data = genesis_data.clone();
            genesis_data[data_len - corruption.len()..].copy_from_slice(&corruption); // Corrupt the last part of the file
            std::fs::write(&genesis_file, genesis_data).unwrap();

            let args = Args::parse_from(["blockservice", "import", genesis_file.to_str().unwrap()]);
            let result = execute(args);
            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("corrupt gzip stream")
            );
        }
    }

    #[test]
    fn import_fails_if_no_write_permissions() {
        let tmpdir = tempfile::tempdir().unwrap();

        // Create a read-only database
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o555)).unwrap();

        let args = Args::parse_from(["blockservice", "import", "somepath"]);
        let result = execute(args);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
    }

    #[test]
    fn verify_fails_if_no_read_permissions() {
        let tmpdir = tempfile::tempdir().unwrap();

        // create database
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        // remove read permissions
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o333)).unwrap();

        let args = Args::parse_from(["blockservice", "verify", "0"]);
        let result = execute(args);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to open"));
    }

    #[test]
    fn verify_fails_if_db_does_not_exist() {
        let tmpdir = tempfile::tempdir().unwrap();

        let _cwd = ChangeWorkingDir::new(tmpdir.path());

        let args = Args::parse_from(["blockservice", "verify", "0"]);
        let result = execute(args);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No such file or directory")
        );
    }

    #[test]
    fn verify_checks_hash_of_block() {
        let chain_id = 146;
        let block = Block::default();

        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut db = RocksBlockDb::open(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
        db.put(chain_id, block.clone()).unwrap();

        // correct hash
        let mut buf = Vec::new();
        let result = verify(
            chain_id,
            Some(block.number),
            Some(block.to_header().compute_hash()),
            &mut buf,
        );
        assert!(result.is_ok());
        assert!(buf.is_empty());

        // incorrect hash
        let mut buf = Vec::new();
        let result = verify(
            chain_id,
            Some(block.number),
            Some(Hash::default()), // intentionally wrong hash
            &mut buf,
        );
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!(
                "[chain ID {}] block hash verification failed for block {}: expected hash {}, got {}.\n",
                chain_id,
                block.number,
                Hash::default().to_hex(),
                block.to_header().compute_hash().to_hex()
            )
        );
    }

    #[test]
    fn verify_prints_message_if_block_not_found() {
        let chain_id = 146;
        let block_number = 0;

        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        let mut buf = Vec::new();
        let result = verify(
            chain_id,
            Some(block_number),
            Some(Hash::default()),
            &mut buf,
        );
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!("[chain ID {chain_id}] requested block {block_number} does not exit\n")
        );
    }

    #[test]
    fn verify_checks_number_of_block() {
        let chain_id = 146;
        let mut block = Block::default();

        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut db = RocksBlockDb::open(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
        db.put(chain_id, block.clone()).unwrap();

        // block number matches
        let mut buf = Vec::new();
        let result = verify(chain_id, None, None, &mut buf);
        assert!(result.is_ok());
        println!("{}", String::from_utf8(buf.clone()).unwrap());
        assert!(buf.is_empty());

        // block number mismatches
        // at block number 0, blocknumber (0) and block.number (1) mismatch
        block.number = 1;
        let data = proto::Block::from(block.clone()).encode_to_vec();
        db.put_raw(chain_id, 0, &data).unwrap();

        let mut buf = Vec::new();
        let result = verify(chain_id, None, None, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!(
                "[chain ID {}] block number mismatch: block number in key = {}, block.number = {}.\n",
                chain_id, 0, block.number
            )
        );
    }

    #[test]
    fn verify_checks_parent_hash_of_block() {
        let chain_id = 146;
        let block0 = Block::default();

        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut db = RocksBlockDb::open(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
        db.put(chain_id, block0.clone()).unwrap();

        // correct hash
        let mut block1 = Block {
            number: 1,
            parent_hash: block0.to_header().compute_hash(),
            ..Block::default()
        };
        db.put(chain_id, block1.clone()).unwrap();

        let mut buf = Vec::new();
        let result = verify(chain_id, None, None, &mut buf);
        assert!(result.is_ok());
        assert!(buf.is_empty());

        // incorrect parent hash
        block1.parent_hash = Hash::default(); // intentionally wrong parent hash
        db.put(chain_id, block1.clone()).unwrap();

        let mut buf = Vec::new();
        let result = verify(chain_id, None, None, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!(
                "[chain ID {}] parent hash verification failed for block {}: expected hash {}, got {}.\n",
                chain_id,
                block1.number,
                block0.to_header().compute_hash().to_hex(),
                block1.parent_hash.to_hex()
            )
        );
    }

    #[test]
    fn verify_skips_parent_hash_check_between_disjoint_ranges() {
        let chain_id = 146;

        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut db = RocksBlockDb::open(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();

        let mut block = Block::default();
        db.put(chain_id, block.clone()).unwrap();

        // correct hash
        block.parent_hash = block.to_header().compute_hash();
        block.number = 1;
        db.put(chain_id, block.clone()).unwrap();

        // skip one block and use mismatching parent hash
        block.parent_hash = Hash::default();
        block.number = 3;
        db.put(chain_id, block.clone()).unwrap();

        // correct hash
        block.parent_hash = block.to_header().compute_hash();
        block.number = 4;
        db.put(chain_id, block.clone()).unwrap();

        let mut buf = Vec::new();
        let result = verify(chain_id, None, None, &mut buf);
        assert!(result.is_ok());
        println!("{}", String::from_utf8(buf.clone()).unwrap());
        assert!(buf.is_empty());
    }
}
