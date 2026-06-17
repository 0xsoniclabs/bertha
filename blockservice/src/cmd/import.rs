// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

use std::{fs::File, io::BufReader, path::Path};

use bertha_types::{Block, Hash, HexConvert};
use genesis_parser::{Era1FileReader, EraDir, EraFileReader, GFile};
use prost::Message;

use crate::{
    app_dir::open_app_dir,
    cmd::{CancelIndicator, make_progress_bar},
    config::{ChainConfig, Config},
    db::{BlockDb, BlockDbBatch, RocksBlockDb, proto},
};

const BATCH_SIZE: usize = 100_000_000; // in bytes

/// Imports blocks from a directory containing `.era1` files into the database located in `app_dir`,
/// and optionally verifies the parent hashes.
pub fn import_era1(
    app_dir: impl AsRef<Path>,
    era_dir_path: impl AsRef<Path>,
    chain_id: u64,
    verify: bool,
    cancel_indicator: &impl CancelIndicator,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    writeln!(
        writer,
        "WARNING: `.era1` file import is still experimental and might store invalid data."
    )?;

    let (cfg, mut db) = open_app_dir(app_dir, false)?;

    let era_dir = EraDir::<Era1FileReader>::open(era_dir_path, chain_id)?;
    let blocks = era_dir.blocks();

    import(
        cfg,
        &mut db,
        blocks,
        chain_id,
        verify,
        cancel_indicator,
        &mut writer,
    )
}

/// Imports blocks from a directory containing `.era` files into the database located in `app_dir`.
pub fn import_era(
    app_dir: impl AsRef<Path>,
    era_dir_path: impl AsRef<Path>,
    chain_id: u64,
    cancel_indicator: &impl CancelIndicator,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    writeln!(
        writer,
        "WARNING: `.era` file import is still experimental and might store invalid data."
    )?;

    let (cfg, mut db) = open_app_dir(app_dir, false)?;

    let era_dir = EraDir::<EraFileReader>::open(era_dir_path, chain_id)?;
    let blocks = era_dir.blocks();

    import(
        cfg,
        &mut db,
        blocks,
        chain_id,
        false,
        cancel_indicator,
        &mut writer,
    )
}

/// Imports blocks from a `.g` file into the database located in `app_dir` and optionally verifies
/// the parent hashes.
pub fn import_gfile(
    app_dir: impl AsRef<Path>,
    gfile_path: impl AsRef<Path>,
    verify: bool,
    cancel_indicator: &impl CancelIndicator,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (cfg, mut db) = open_app_dir(app_dir, false)?;

    let file = File::open(&gfile_path)?;
    let reader = BufReader::new(file);
    let mut genesis = GFile::parse(reader)?;
    let chain_id = genesis.chain_id();
    let blocks = genesis.blocks();

    import(
        cfg,
        &mut db,
        blocks,
        chain_id,
        verify,
        cancel_indicator,
        &mut writer,
    )
}

fn import(
    mut cfg: Config,
    db: &mut RocksBlockDb,
    blocks: impl Iterator<Item = Result<Block, genesis_parser::Error>>,
    chain_id: u64,
    verify: bool,
    cancel_indicator: &impl CancelIndicator,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut blocks = blocks.peekable();

    let total_blocks = match blocks.peek() {
        None => 0,
        Some(Ok(block)) => block.number + 1,
        // through peek only a reference to the error is available, so next is used which returns
        // the exact same value but as an owned value
        Some(Err(_)) => return Err(blocks.next().unwrap().unwrap_err().into()),
    };

    writeln!(
        writer,
        "Genesis file contains {total_blocks} blocks for chain ID {chain_id}"
    )?;

    if cfg.get_chain_config(chain_id).is_none() {
        writeln!(
            writer,
            "Creating new entry for chain ID {chain_id} in the configuration"
        )?;
        cfg.add_chain(ChainConfig::new(chain_id))?;
    }

    // Determine until which block we have to import blocks, and which range we already have in the
    // DB. To keep things simple, we only skip a range if it starts at block 0.
    // We do this because if we would skip also ranges after that, we would have to parse the blocks
    // anyway because there is no way to seek in RLP. Also parent hash validation would require
    // loading blocks from the db.
    // And the primary use case for this partial import is that if you imported a genesis in the
    // past and there is a newer one, we should only import the new blocks.
    let ranges = db.get_ranges_of_chain_id(chain_id)?;
    let mut smallest_import_block_number = 0; // this is the smallest block number we have to import
    if let Some(range) = ranges.first()
        && *range.start() == 0
    {
        smallest_import_block_number = *range.end() + 1;
    }

    let import_blocks = total_blocks.saturating_sub(smallest_import_block_number);

    if import_blocks == 0 {
        writeln!(
            writer,
            "All blocks are already in the database, nothing to import"
        )?;
        return Ok(());
    }

    if import_blocks != total_blocks {
        writeln!(
            writer,
            "Skipping {} blocks that are already in the database",
            total_blocks - import_blocks
        )?;
    }

    writeln!(
        writer,
        "Importing {import_blocks} blocks for chain ID {chain_id}"
    )?;

    let mut uncompressed_bytes_written = 0;
    let mut block_count = 0;
    let progress_bar = make_progress_bar(import_blocks)?;

    let mut batch = db.batch();
    let mut prev_parent_hash: Option<Hash> = None;
    let before = std::time::Instant::now();
    for result in blocks {
        if cancel_indicator.is_cancelled() {
            writeln!(writer, "Import cancelled.")?;
            break;
        }

        let mut block = result?;

        if verify {
            if block.number < smallest_import_block_number {
                // the block is already in the database, therefore we have to verify with the block
                // in the database and not with the block from the genesis
                block = db.get(chain_id, block.number)?.ok_or_else(|| {
                    format!("Invalid metadata, block {} does not exist", block.number)
                })?;
            }
            // Note: The blocks in the `blocks` iterator are expected to be in descending order
            // (w.r.t. block number).
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

        // We use put_bytes so we can count bytes.
        let number = block.number;
        let protoblock = proto::Block::from(block).encode_to_vec();
        uncompressed_bytes_written += protoblock.len();

        if batch.size() >= BATCH_SIZE {
            db.write_batch(batch)?;
            batch = db.batch();
        }
        batch.put_bytes(chain_id, number, &protoblock);

        block_count += 1;
        progress_bar.inc(1);
    }
    db.write_batch(batch)?;
    let elapsed = before.elapsed();
    progress_bar.finish();
    writeln!(
        writer,
        "Wrote {} blocks, total uncompressed size: {} MiB, elapsed: {}s, throughput: {:.1} MiB/s",
        block_count,
        uncompressed_bytes_written / (1024 * 1024),
        elapsed.as_secs(),
        uncompressed_bytes_written as f64 / (1024.0 * 1024.0) / elapsed.as_secs_f64()
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use core::slice;

    use bertha_types::Block;
    use mockall::Sequence;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::{
        BlockRange,
        app_dir::init_app_dir,
        cmd::MockCancelIndicator,
        db::{
            CHAIN_IDS_KEY, KvDb, make_block_ranges_key, serialize_block_ranges, serialize_chain_ids,
        },
        utils::{
            ranges::subtract_ranges,
            test_dir::{Permissions, TestDir},
        },
    };

    #[test]
    fn inserts_all_blocks_from_snapshot_file_into_db_and_verifies_them() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");
        let num_blocks = 5;
        let chain_id = 62;
        let genesis_data =
            genesis_parser::test_utils::generate_test_genesis(chain_id, num_blocks, &[]);
        std::fs::write(&genesis_file, genesis_data).unwrap();

        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let mut writer = Vec::new();
        import_gfile(
            tmpdir.path(),
            genesis_file.to_str().unwrap(),
            true,
            &CancellationToken::new(),
            &mut writer,
        )
        .unwrap();

        assert!(String::from_utf8(writer).unwrap().contains(indoc::indoc! {"
            Genesis file contains 5 blocks for chain ID 62
            Creating new entry for chain ID 62 in the configuration
            Importing 5 blocks for chain ID 62
            Wrote 5 blocks, total uncompressed size: 0 MiB, elapsed: 0s, throughput: "
        }));
        let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
        for i in 0..num_blocks {
            let block = db.get(chain_id, i as u64).unwrap();
            assert!(block.is_some(), "Block {i} not found in the database");
        }
    }

    #[rstest::rstest]
    #[case::skips_stored_blocks(0..=2, 0..=4, Some(3..=4))]
    #[case::skips_import_when_all_blocks_stored(0..=5, 0..=5, None)]
    #[case::does_not_skip_stored_blocks_if_not_starting_at_zero(1..=2, 0..=4, Some(0..=4))]
    fn import_skips_blocks_already_in_db(
        #[case] db_blocks_range: BlockRange,
        #[case] available_blocks_range: BlockRange,
        #[case] import_blocks_range: Option<BlockRange>,
    ) {
        let chain_id = 146;

        let db_blocks: Vec<_> = db_blocks_range
            .clone()
            .map(|i| Block {
                number: i,
                ..Block::default()
            })
            .collect();
        let available_blocks: Vec<_> = available_blocks_range
            .clone()
            .map(|i| {
                Block {
                    number: i,
                    difficulty: 1, // non-default difficulty to distinguish from blocks stored in DB
                    ..Block::default()
                }
            })
            .collect();

        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (cfg, mut db) = open_app_dir(tmpdir.path(), false).unwrap();
        for block in &db_blocks {
            db.put(chain_id, block.clone()).unwrap();
        }

        let mut writer = Vec::new();
        import(
            cfg,
            &mut db,
            available_blocks.clone().into_iter().map(Ok).rev(),
            chain_id,
            false,
            &CancellationToken::new(),
            &mut writer,
        )
        .unwrap();

        let output = String::from_utf8(writer).unwrap();
        let mut expected_output = String::new();
        expected_output += &indoc::formatdoc! {"
            Genesis file contains {} blocks for chain ID 146
            Creating new entry for chain ID 146 in the configuration
            ",
            available_blocks_range.clone().count()
        };

        if let Some(import_blocks_range) = &import_blocks_range {
            let skip = subtract_ranges(
                available_blocks_range.clone(),
                slice::from_ref(import_blocks_range),
            )
            .into_iter()
            .map(Iterator::count)
            .sum::<usize>();
            if skip > 0 {
                expected_output += &indoc::formatdoc! {"
                    Skipping {skip} blocks that are already in the database
                "};
            }
            let import_blocks_count = import_blocks_range.clone().count();
            expected_output += &indoc::formatdoc! {"
                Importing {import_blocks_count} blocks for chain ID 146
                Wrote {import_blocks_count} blocks, total uncompressed size: 0 MiB, elapsed: 0s, throughput: ",
            };
        } else {
            expected_output += &indoc::indoc! {"
                All blocks are already in the database, nothing to import"
            };
        }
        assert!(
            output.starts_with(&expected_output),
            "got output:\n{output}\nexpected output:\n{expected_output}"
        );

        // Verify existing blocks that are not reimported are not overwritten and imported blocks
        // are stored in the database
        if let Some(import_blocks_range) = import_blocks_range {
            // Existing non-imported blocks should remain unchanged
            for block in db_blocks {
                if !import_blocks_range.contains(&block.number) {
                    assert_eq!(db.get(chain_id, block.number).unwrap(), Some(block));
                }
            }
            // Imported blocks should be stored in the database and might overwrite existing blocks
            for block_number in import_blocks_range {
                assert_eq!(
                    db.get(chain_id, block_number).unwrap(),
                    Some(Block {
                        number: block_number,
                        difficulty: 1, /* non-default difficulty to distinguish from blocks
                                        * stored in DB */
                        ..Default::default()
                    })
                );
            }
        } else {
            // Existing non-imported blocks should remain unchanged
            for block in db_blocks {
                assert_eq!(db.get(chain_id, block.number).unwrap(), Some(block));
            }
        }
    }

    #[test]
    fn fails_if_parent_hash_of_block_0_is_not_0_hash() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");

        let extra_blocks = [Block {
            number: 0,
            parent_hash: [1; 32],
            ..Block::default_sonic()
        }];
        let genesis_data = genesis_parser::test_utils::generate_test_genesis(1, 0, &extra_blocks);
        std::fs::write(&genesis_file, genesis_data).unwrap();

        let mut writer = Vec::new();
        assert!(
            import_gfile(tmpdir.path(),genesis_file.to_str().unwrap(), true, &CancellationToken::new(), &mut writer)
                .unwrap_err()
                .to_string()
                .contains("Block zero must have parent hash 0x0000000000000000000000000000000000000000000000000000000000000000")
        );

        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains(indoc::indoc! {"
            Genesis file contains 1 blocks for chain ID 1
            Creating new entry for chain ID 1 in the configuration
            Importing 1 blocks for chain ID 1"
        }));
    }

    #[test]
    fn fails_if_parent_hash_mismatches() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");
        let extra_blocks = [Block::default_sonic()];
        let genesis_data = genesis_parser::test_utils::generate_test_genesis(1, 1, &extra_blocks);
        std::fs::write(&genesis_file, genesis_data).unwrap();

        let mut writer = Vec::new();
        assert!(
            import_gfile(
                tmpdir.path(),
                genesis_file.to_str().unwrap(),
                true,
                &CancellationToken::new(),
                &mut writer
            )
            .unwrap_err()
            .to_string()
            .contains("Parent hash mismatch for block 1")
        );

        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains(indoc::indoc! {"
            Genesis file contains 1 blocks for chain ID 1
            Creating new entry for chain ID 1 in the configuration
            Importing 1 blocks for chain ID 1"
        }));
    }

    #[test]
    fn fails_if_parent_hash_of_block_in_db_mismatches() {
        // hash(block_0 in snapshot) == block_1.parent_hash
        // && hash(block_0 in db) != block_1.parent_hash
        let chain_id = 1;
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");
        let genesis_data = genesis_parser::test_utils::generate_test_genesis(chain_id, 2, &[]);
        std::fs::write(&genesis_file, genesis_data).unwrap();
        let (_, mut db) = open_app_dir(tmpdir.path(), false).unwrap();
        db.put(
            chain_id,
            Block {
                state_root: [1; 32], // different state root to ensure different hash
                ..Block::default_sonic()
            },
        )
        .unwrap();
        drop(db);

        let mut writer = Vec::new();
        assert!(
            import_gfile(
                tmpdir.path(),
                genesis_file.to_str().unwrap(),
                true,
                &CancellationToken::new(),
                &mut writer
            )
            .unwrap_err()
            .to_string()
            .contains("Parent hash mismatch for block 1")
        );

        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains(indoc::indoc! {"
            Genesis file contains 2 blocks for chain ID 1
            Creating new entry for chain ID 1 in the configuration
            Skipping 1 blocks that are already in the database
            Importing 1 blocks for chain ID 1"
        }));
    }

    #[test]
    fn fails_when_metadata_invalid() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let chain_id = 146;

        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
        // Write invalid metadata: claim blocks 0..=1 exist without storing them
        db.kv_db()
            .put_raw(
                &make_block_ranges_key(chain_id),
                &serialize_block_ranges([0..=1]),
            )
            .unwrap();
        db.kv_db()
            .put_raw(&CHAIN_IDS_KEY, &serialize_chain_ids([chain_id]))
            .unwrap();
        drop(db);

        let genesis_file = tmpdir.path().join("genesis.g");
        let genesis_data = genesis_parser::test_utils::generate_test_genesis(chain_id, 3, &[]);
        std::fs::write(&genesis_file, genesis_data).unwrap();

        let mut writer = Vec::new();
        assert_eq!(
            import_gfile(
                tmpdir.path(),
                genesis_file.to_str().unwrap(),
                true,
                &CancellationToken::new(),
                &mut writer
            )
            .unwrap_err()
            .to_string(),
            "Invalid metadata, block 1 does not exist"
        );

        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains(indoc::indoc! {"
            Genesis file contains 3 blocks for chain ID 146
            Creating new entry for chain ID 146 in the configuration
            Skipping 2 blocks that are already in the database
            Importing 1 blocks for chain ID 146"
        }));
    }

    #[test]
    fn aborts_on_invalid_snapshot_file() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");
        let genesis_data = genesis_parser::test_utils::generate_test_genesis(0, 5, &[]);
        let data_len = genesis_data.len();
        let corruption = [0xde, 0xad, 0xbe, 0xef];
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        // Corrupted header
        {
            let mut genesis_data = genesis_data.clone();
            genesis_data[0..corruption.len()].copy_from_slice(&corruption); // Corrupt the first part of the file
            std::fs::write(&genesis_file, genesis_data).unwrap();

            let mut writer = Vec::new();
            let result = import_gfile(
                tmpdir.path(),
                genesis_file.to_str().unwrap(),
                false,
                &CancellationToken::new(),
                &mut writer,
            );
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("invalid header"));
            assert!(writer.is_empty());
        }

        // Corrupted block
        {
            let mut genesis_data = genesis_data.clone();
            genesis_data[data_len - corruption.len()..].copy_from_slice(&corruption); // Corrupt the last part of the file
            std::fs::write(&genesis_file, genesis_data).unwrap();

            let mut writer = Vec::new();
            let result = import_gfile(
                tmpdir.path(),
                genesis_file.to_str().unwrap(),
                false,
                &CancellationToken::new(),
                &mut writer,
            );
            assert!(result.is_err());
            let result_str = result.unwrap_err().to_string();
            assert!(result_str.contains("corrupt gzip stream"));
            assert!(writer.is_empty());
        }
    }

    #[test]
    fn fails_if_app_dir_is_not_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        let mut writer = Vec::new();
        let result = import_gfile(
            tmpdir.path(),
            "somepath",
            true,
            &CancellationToken::new(),
            &mut writer,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
        assert!(writer.is_empty());
    }

    #[test]
    fn fails_if_no_write_permissions() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        // Create a read-only database
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        tmpdir.set_permissions(Permissions::ReadOnly).unwrap();

        let mut writer = Vec::new();
        let result = import_gfile(
            tmpdir.path(),
            "somepath",
            true,
            &CancellationToken::new(),
            &mut writer,
        );
        // We expect an error because we cannot write to the database
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
        assert!(writer.is_empty());
    }

    #[test]
    fn stops_import_and_flushes_batch_when_cancelled() {
        let chain_id = 146;

        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (config, mut db) = open_app_dir(tmpdir.path(), false).unwrap();

        let mut token = MockCancelIndicator::new();
        let mut seq = Sequence::new();
        token
            .expect_is_cancelled()
            .times(1)
            .return_const(false)
            .in_sequence(&mut seq);
        token
            .expect_is_cancelled()
            .times(1)
            .return_const(true)
            .in_sequence(&mut seq);

        let mut i = 0;
        let blocks = std::iter::from_fn(|| {
            let block = Block {
                number: i,
                ..Block::default_sonic()
            };
            i += 1;
            Some(Ok(block))
        });

        let mut writer = Vec::new();
        let result = import(
            config,
            &mut db,
            blocks,
            chain_id,
            false,
            &token,
            &mut writer,
        );
        assert!(result.is_ok());

        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains("Import cancelled."));

        // Only the first block should have been imported.
        assert!(db.get(chain_id, 0).unwrap().is_some());
        assert!(db.get(chain_id, 1).unwrap().is_none());
        assert!(db.get(chain_id, 2).unwrap().is_none());
    }
}
