use std::{fmt::Write, fs::File, io::BufReader, path::Path};

use bertha_types::{Hash, HexConvert};
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

    let mut prev_parent_hash: Option<Hash> = None;
    let before = std::time::Instant::now();
    for result in blocks {
        let block = result?;
        if block_count == 0 {
            // We rely on the fact that blocks are stored in reverse order.
            total_blocks = block.number + 1;
            println!("Importing {total_blocks} blocks for chain ID {chain_id}");
            progress_bar.set_length(total_blocks);
        }

        // Note: blocks are in reverse order
        if let Some(prev_parent_hash) = prev_parent_hash {
            let block_hash = block.to_header().compute_hash();
            if block_hash != prev_parent_hash {
                return Err(format!(
                    "Parent hash mismatch for block {}: previous block hash {}, parent hash {}",
                    block.number + 1,
                    block_hash.to_hex(),
                    prev_parent_hash.to_hex()
                )
                .into());
            }
        }
        prev_parent_hash = Some(block.parent_hash);

        if block.number == 0 && block.parent_hash != Hash::default() {
            return Err(format!(
                "Block zero must have parent hash {}",
                Hash::default().to_hex()
            )
            .into());
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

    use bertha_types::Block;

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
    fn fails_if_parent_hash_mismatches() {
        let tmpdir = tempfile::tempdir().unwrap();

        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        let genesis_file = tmpdir.path().join("genesis.g");

        // block 0 hash no zero parent hash
        let extra_blocks = vec![Block {
            parent_hash: [1; 32],
            ..Block::default_sonic()
        }];
        let genesis_data = genesis_parser::test_utils::generate_test_genesis(0, 0, extra_blocks);
        std::fs::write(&genesis_file, genesis_data).unwrap();

        assert!(
            import(genesis_file.to_str().unwrap())
                .unwrap_err()
                .to_string()
                .contains("Block zero must have parent hash 0x0000000000000000000000000000000000000000000000000000000000000000")
        );

        // hash(block_0) != block_1.parent_hash
        let extra_blocks = vec![Block::default_sonic()];
        let genesis_data = genesis_parser::test_utils::generate_test_genesis(0, 1, extra_blocks);
        std::fs::write(&genesis_file, genesis_data).unwrap();

        assert!(
            import(genesis_file.to_str().unwrap())
                .unwrap_err()
                .to_string()
                .contains("Parent hash mismatch for block 1")
        );
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
