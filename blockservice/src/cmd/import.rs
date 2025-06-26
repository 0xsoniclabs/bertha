use std::{fmt::Write, fs::File, io::BufReader, path::Path};

use blockservice::blockdb::{BlockDb, RocksBlockDb};
use genesis_parser::Genesis;
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use prost::Message;

use crate::BLOCK_DB_NAME;

pub fn import(path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
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

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::cmd::{ChangeWorkingDir, init};

    #[test]
    fn inserts_all_blocks_from_snapshot_file_into_db() {
        let tmpdir = tempfile::tempdir().unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");
        let num_blocks = 5;
        let chain_id = 62;
        let genesis_data =
            genesis_parser::test_utils::generate_test_genesis(chain_id, num_blocks, Vec::new());
        std::fs::write(&genesis_file, genesis_data).unwrap();

        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        import(genesis_file.to_str().unwrap()).unwrap();

        let db = RocksBlockDb::open_for_reading(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
        for i in 0..num_blocks {
            let block = db.get(chain_id, i as u64).unwrap();
            assert!(block.is_some(), "Block {i} not found in the database");
        }
    }

    #[test]
    fn aborts_on_invalid_snapshot_file() {
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

            let result = import(genesis_file.to_str().unwrap());
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("invalid header"));
        }

        // Corrupted block
        {
            let mut genesis_data = genesis_data.clone();
            genesis_data[data_len - corruption.len()..].copy_from_slice(&corruption); // Corrupt the last part of the file
            std::fs::write(&genesis_file, genesis_data).unwrap();

            let result = import(genesis_file.to_str().unwrap());
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
    fn fails_if_no_write_permissions() {
        let tmpdir = tempfile::tempdir().unwrap();

        // Create a read-only database
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = import("somepath");
        // We expect an error because we cannot write to the database
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
    }
}
