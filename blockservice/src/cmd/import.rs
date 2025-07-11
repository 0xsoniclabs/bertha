use std::{fs::File, io::BufReader, path::Path};

use bertha_types::{Hash, HexConvert};
use genesis_parser::Genesis;
use prost::Message;

use crate::{
    cmd::make_progress_bar,
    db::{BLOCK_DB_NAME, BlockDb, RocksBlockDb, proto},
};

pub fn import(path: impl AsRef<Path>, verify: bool) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = Path::new("./").join(BLOCK_DB_NAME).canonicalize()?;
    let mut db = RocksBlockDb::open(db_path)?;

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut genesis = Genesis::parse(&mut reader)?;
    let chain_id = genesis.chain_id();
    let mut blocks = genesis.blocks().peekable();
    let total_blocks = blocks
        .peek()
        .and_then(|b| b.as_ref().map(|b| b.number + 1).ok())
        .unwrap_or_default();

    println!("Genesis file contains {total_blocks} blocks for chain ID {chain_id}");

    // Determine until which block we have to import blocks, and which range we already have in the
    // DB. To keep things simple, we only skip a range if it starts at block 0.
    // We do this because if we would skip also ranges after that, we would have to parse the blocks
    // anyway because there is no way to seek in RLP. Also parent hash validation would require
    // loading blocks from the db.
    // And the primary use case for this partial import is that if you imported a genesis in the
    // past and there is a newer one, we should only import the new blocks.
    let ranges = db.get_ranges_of_chain_id(chain_id)?;
    let mut smallest_import_block_number = 0; // this is the smallest block number we have to import
    if let Some(range) = ranges.first() {
        if range.0 == 0 {
            smallest_import_block_number = range.1 + 1;
        }
    }

    let import_blocks = total_blocks - smallest_import_block_number;

    if import_blocks != total_blocks {
        println!(
            "Skipping {} blocks that are already in the database",
            total_blocks - import_blocks
        );
    }

    let mut uncompressed_bytes_written = 0;
    let mut block_count = 0;
    let progress_bar = make_progress_bar(total_blocks)?;

    println!("Importing {import_blocks} blocks for chain ID {chain_id}");

    let mut prev_parent_hash: Option<Hash> = None;
    let before = std::time::Instant::now();
    for result in blocks {
        let mut block = result?;

        if verify {
            if block.number < smallest_import_block_number {
                // the block is already in the database, therefore we have to verify with the block
                // in the database and not with the block from the genesis
                block = db.get(chain_id, block.number)?.ok_or_else(|| {
                    format!("Invalid metadata, block {} does not exist", block.number)
                })?;
            }
            // Note: blocks in genesis/ g-file are in reverse order
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
        }

        if block.number < smallest_import_block_number {
            // Skip blocks that are already in the database.
            break;
        }

        // We use put_raw so we can count bytes.
        let number = block.number;
        let protoblock = proto::Block::from(block).encode_to_vec();
        uncompressed_bytes_written += protoblock.len();
        db.put_raw(chain_id, number, &protoblock)?;
        block_count += 1;
        progress_bar.inc(1);
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
    fn inserts_all_blocks_from_snapshot_file_into_db_and_verifies_them() {
        let tmpdir = tempfile::tempdir().unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");
        let num_blocks = 5;
        let chain_id = 62;
        let genesis_data =
            genesis_parser::test_utils::generate_test_genesis(chain_id, num_blocks, &[]);
        std::fs::write(&genesis_file, genesis_data).unwrap();

        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        import(genesis_file.to_str().unwrap(), true).unwrap();

        let db = RocksBlockDb::open_for_reading(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
        for i in 0..num_blocks {
            let block = db.get(chain_id, i as u64).unwrap();
            assert!(block.is_some(), "Block {i} not found in the database");
        }
    }

    #[test]
    fn inserts_missing_blocks_from_snapshot_file_into_db() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());

        let num_blocks = 5;
        let chain_id = 146;

        let mut prev_hash = Hash::default();
        let mut all_blocks = Vec::new();
        for block_number in 0..num_blocks {
            let block = Block {
                number: block_number,
                parent_hash: prev_hash,
                ..Block::default_sonic()
            };
            prev_hash = block.to_header().compute_hash();
            all_blocks.push(block);
        }

        let db_blocks_num = 2;
        let db_blocks = &all_blocks[..db_blocks_num];
        let mut genesis_blocks = all_blocks.clone();

        init(None::<&Path>).unwrap();
        let mut db = RocksBlockDb::open(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
        for block in db_blocks {
            db.put(chain_id, block.clone()).unwrap();
        }
        drop(db);

        // modify the blocks which are part of the genesis file but are not inserted into the db
        // because they are already stored
        for block in genesis_blocks.iter_mut().take(db_blocks_num) {
            block.gas_limit = 1; // modify block so we can check that the existing blocks are not being overwritten
        }
        let genesis_file = tmpdir.path().join("genesis.g");
        let genesis_data =
            genesis_parser::test_utils::generate_test_genesis(chain_id, 0, &genesis_blocks);
        std::fs::write(&genesis_file, genesis_data).unwrap();

        import(genesis_file.to_str().unwrap(), false).unwrap();

        let db = RocksBlockDb::open_for_reading(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
        for block in all_blocks {
            let db_block = db.get(chain_id, block.number).unwrap();
            // check that the missing blocks were inserted and the existing blocks were not
            // modified
            assert_eq!(db_block, Some(block.clone()),);
        }
    }

    #[test]
    fn fails_if_parent_hash_of_block_0_is_not_0_hash() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");

        let extra_blocks = [Block {
            number: 0,
            parent_hash: [1; 32],
            ..Block::default_sonic()
        }];
        let genesis_data = genesis_parser::test_utils::generate_test_genesis(1, 0, &extra_blocks);
        std::fs::write(&genesis_file, genesis_data).unwrap();

        assert!(
                import(genesis_file.to_str().unwrap(), true)
                    .unwrap_err()
                    .to_string()
                    .contains("Block zero must have parent hash 0x0000000000000000000000000000000000000000000000000000000000000000")
            );
    }

    #[test]
    fn fails_if_parent_hash_mismatches() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");
        let extra_blocks = [Block::default_sonic()];
        let genesis_data = genesis_parser::test_utils::generate_test_genesis(1, 1, &extra_blocks);
        std::fs::write(&genesis_file, genesis_data).unwrap();

        assert!(
            import(genesis_file.to_str().unwrap(), true)
                .unwrap_err()
                .to_string()
                .contains("Parent hash mismatch for block 1")
        );
    }

    #[test]
    fn fails_if_parent_hash_of_block_in_db_mismatches() {
        // hash(block_0 in snapshot) == block_1.parent_hash
        // && hash(block_0 in db) != block_1.parent_hash
        let chain_id = 1;
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");
        let genesis_data = genesis_parser::test_utils::generate_test_genesis(chain_id, 2, &[]);
        std::fs::write(&genesis_file, genesis_data).unwrap();
        let mut db = RocksBlockDb::open(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
        db.put(
            chain_id,
            Block {
                state_root: [1; 32], // different state root to ensure different hash
                ..Block::default_sonic()
            },
        )
        .unwrap();
        drop(db);

        assert!(
            import(genesis_file.to_str().unwrap(), true)
                .unwrap_err()
                .to_string()
                .contains("Parent hash mismatch for block 1")
        );
    }

    #[test]
    fn fails_when_metadata_invalid() {
        let tmpdir = tempfile::tempdir().unwrap();

        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        let chain_id = 146;

        let mut db = RocksBlockDb::open(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
        db.put_ranges_of_chain_id(chain_id, &[(0, 1)]).unwrap(); // this data does not exist
        drop(db);

        let genesis_file = tmpdir.path().join("genesis.g");
        let genesis_data = genesis_parser::test_utils::generate_test_genesis(chain_id, 3, &[]);
        std::fs::write(&genesis_file, genesis_data).unwrap();

        assert_eq!(
            import(genesis_file.to_str().unwrap(), true)
                .unwrap_err()
                .to_string(),
            "Invalid metadata, block 1 does not exist"
        );
    }

    #[test]
    fn aborts_on_invalid_snapshot_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");
        let genesis_data = genesis_parser::test_utils::generate_test_genesis(0, 5, &[]);
        let data_len = genesis_data.len();
        let corruption = [0xde, 0xad, 0xbe, 0xef];
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        // Corrupted header
        {
            let mut genesis_data = genesis_data.clone();
            genesis_data[0..corruption.len()].copy_from_slice(&corruption); // Corrupt the first part of the file
            std::fs::write(&genesis_file, genesis_data).unwrap();

            let result = import(genesis_file.to_str().unwrap(), false);
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("invalid header"));
        }

        // Corrupted block
        {
            let mut genesis_data = genesis_data.clone();
            genesis_data[data_len - corruption.len()..].copy_from_slice(&corruption); // Corrupt the last part of the file
            std::fs::write(&genesis_file, genesis_data).unwrap();

            let result = import(genesis_file.to_str().unwrap(), false);
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

        let result = import("somepath", true);
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
