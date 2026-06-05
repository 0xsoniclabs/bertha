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
    db::{BlockDb, BlockDbBatch, proto},
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
    writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (cfg, mut db) = open_app_dir(app_dir, false)?;
    import_era1_internal(
        cfg,
        &mut db,
        era_dir_path,
        chain_id,
        verify,
        cancel_indicator,
        writer,
    )
}

/// Imports blocks from a directory containing `.era` files into the database located in `app_dir`.
pub fn import_era(
    app_dir: impl AsRef<Path>,
    era_dir_path: impl AsRef<Path>,
    chain_id: u64,
    cancel_indicator: &impl CancelIndicator,
    writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (cfg, mut db) = open_app_dir(app_dir, false)?;
    import_era_internal(
        cfg,
        &mut db,
        era_dir_path,
        chain_id,
        cancel_indicator,
        writer,
    )
}

/// Imports blocks from a `.g` file into the database located in `app_dir` and optionally verifies
/// the parent hashes.
pub fn import_gfile(
    app_dir: impl AsRef<Path>,
    gfile_path: impl AsRef<Path>,
    verify: bool,
    cancel_indicator: &impl CancelIndicator,
    writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (cfg, mut db) = open_app_dir(app_dir, false)?;
    import_gfile_internal(cfg, &mut db, gfile_path, verify, cancel_indicator, writer)
}

/// Imports blocks from a directory containing `.era1` files into the database and optionally
/// verifies the parent hashes.
fn import_era1_internal(
    cfg: Config,
    db: &mut impl BlockDb,
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

    let era_dir = EraDir::<Era1FileReader>::open(era_dir_path, chain_id)?;
    let blocks = era_dir.blocks();

    import(
        cfg,
        db,
        blocks,
        chain_id,
        verify,
        cancel_indicator,
        &mut writer,
    )
}

/// Imports blocks from a directory containing `.era` files into the database.
fn import_era_internal(
    cfg: Config,
    db: &mut impl BlockDb,
    era_dir_path: impl AsRef<Path>,
    chain_id: u64,
    cancel_indicator: &impl CancelIndicator,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    writeln!(
        writer,
        "WARNING: `.era` file import is still experimental and might store invalid data."
    )?;

    let era_dir = EraDir::<EraFileReader>::open(era_dir_path, chain_id)?;
    let blocks = era_dir.blocks();

    import(
        cfg,
        db,
        blocks,
        chain_id,
        false,
        cancel_indicator,
        &mut writer,
    )
}

/// Imports blocks from a `.g` file into the database and optionally verifies the parent hashes.
fn import_gfile_internal(
    cfg: Config,
    db: &mut impl BlockDb,
    gfile_path: impl AsRef<Path>,
    verify: bool,
    cancel_indicator: &impl CancelIndicator,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let file = File::open(&gfile_path)?;
    let reader = BufReader::new(file);
    let mut genesis = GFile::parse(reader)?;
    let chain_id = genesis.chain_id();
    let blocks = genesis.blocks();

    import(
        cfg,
        db,
        blocks,
        chain_id,
        verify,
        cancel_indicator,
        &mut writer,
    )
}

fn import(
    mut cfg: Config,
    db: &mut impl BlockDb,
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

    writeln!(writer, "Importing {import_blocks} blocks")?;

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
    use bertha_types::Block;
    use mockall::{Sequence, predicate::eq};
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::{
        BlockRange,
        app_dir::{init_app_dir, open_app_dir},
        cmd::MockCancelIndicator,
        db::{MockBlockDb, MockBlockDbBatch},
        utils::test_dir::{Permissions, TestDir},
    };

    type EmptyResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    fn test_config() -> (TestDir, Config) {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (cfg, _) = open_app_dir(tmpdir.path(), false).unwrap();
        (tmpdir, cfg)
    }

    #[rstest::rstest]
    #[case::import_era1(|path: &Path, w: &mut Vec<u8>| import_era1(path, "somepath", 123, true, &CancellationToken::new(), w))]
    #[case::import_era(|path: &Path, w: &mut Vec<u8>| import_era(path, "somepath", 123, &CancellationToken::new(), w))]
    #[case::import_gfile(|path: &Path, w: &mut Vec<u8>| import_gfile(path, "somepath", true, &CancellationToken::new(), w))]
    fn import_function_fails_if_app_dir_is_not_initialized(
        #[case] run: fn(&Path, &mut Vec<u8>) -> EmptyResult,
    ) {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        let mut writer = Vec::new();
        let result = run(tmpdir.path(), &mut writer);
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
        assert!(writer.is_empty());
    }

    #[test]
    fn import_gfile_internal_puts_all_blocks_from_snapshot_file_into_db_and_verifies_them() {
        let chain_id = 123;
        let num_blocks = 5;
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");
        let genesis_data =
            genesis_parser::test_utils::generate_test_genesis(chain_id, num_blocks, &[]);
        std::fs::write(&genesis_file, genesis_data).unwrap();

        // Use a real tmpdir for the config (add_chain writes to disk)
        let tmpdir_cfg = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir_cfg.path(), std::io::sink()).unwrap();
        let (cfg, _) = open_app_dir(tmpdir_cfg.path(), false).unwrap();

        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(chain_id))
            .return_once(|_| Ok(vec![]));
        db.expect_batch().return_once(move || {
            let mut batch = crate::db::MockBlockDbBatch::new();
            batch.expect_size().return_const(0usize);
            batch
                .expect_put_bytes()
                .withf(move |cid, _, _| *cid == chain_id)
                .times(num_blocks)
                .returning(|_, _, _| ());
            batch
        });
        db.expect_write_batch().return_once(|_| Ok(()));

        let mut writer = Vec::new();
        import_gfile_internal(
            cfg,
            &mut db,
            genesis_file.to_str().unwrap(),
            false,
            &CancellationToken::new(),
            &mut writer,
        )
        .unwrap();

        let expected_output = indoc::formatdoc! {"
            Genesis file contains 5 blocks for chain ID {chain_id}
            Creating new entry for chain ID {chain_id} in the configuration
            Importing 5 blocks
            Wrote 5 blocks, total uncompressed size: 0 MiB, elapsed: 0s, throughput: "
        };
        assert!(
            String::from_utf8(writer)
                .unwrap()
                .contains(&expected_output)
        );
    }

    #[rstest::rstest]
    #[case::corrupted_header(true, "invalid header")]
    #[case::corrupted_block_data(false, "corrupt gzip stream")]
    fn import_gfile_internal_returns_error_on_invalid_snapshot_file(
        #[case] corrupt_at_start: bool,
        #[case] expected_error: &str,
    ) {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let genesis_file = tmpdir.path().join("genesis.g");
        let mut genesis_data = genesis_parser::test_utils::generate_test_genesis(0, 5, &[]);
        let corruption = [0xde, 0xad, 0xbe, 0xef];

        if corrupt_at_start {
            genesis_data[0..corruption.len()].copy_from_slice(&corruption);
        } else {
            let len = genesis_data.len();
            genesis_data[(len - corruption.len())..len].copy_from_slice(&corruption);
        }
        std::fs::write(&genesis_file, genesis_data).unwrap();

        let mut db = MockBlockDb::new();

        let mut writer = Vec::new();
        let result = import_gfile_internal(
            Config::default(),
            &mut db,
            genesis_file.to_str().unwrap(),
            false,
            &CancellationToken::new(),
            &mut writer,
        );
        assert!(result.unwrap_err().to_string().contains(expected_error));
        assert!(writer.is_empty());
    }

    #[test]
    fn import_succeeds_with_verification() {
        let chain_id = 123;
        let (_tmpdir, cfg) = test_config();

        let block_0 = Block::default_sonic();
        let block_0_hash = block_0.to_header().compute_hash();
        let block_1 = Block {
            number: 1,
            parent_hash: block_0_hash,
            ..Block::default_sonic()
        };
        let block_1_hash = block_1.to_header().compute_hash();
        let block_2 = Block {
            number: 2,
            parent_hash: block_1_hash,
            ..Block::default_sonic()
        };
        let blocks = [Ok(block_2), Ok(block_1), Ok(block_0)];

        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(chain_id))
            .return_once(|_| Ok(vec![]));
        db.expect_batch().return_once(|| {
            let mut batch = MockBlockDbBatch::new();
            batch.expect_size().return_const(0usize);
            batch.expect_put_bytes().times(3).returning(|_, _, _| ());
            batch
        });
        db.expect_write_batch().return_once(|_| Ok(()));

        let mut writer = Vec::new();
        import(
            cfg,
            &mut db,
            blocks.into_iter(),
            chain_id,
            true,
            &CancellationToken::new(),
            &mut writer,
        )
        .unwrap();

        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains("Importing 3 blocks"));
        assert!(output.contains("Wrote 3 blocks"));
    }

    #[rstest::rstest]
    #[case::block_0_nonzero_parent_hash(
        vec![],
        vec![Ok(Block { number: 0, parent_hash: [1; 32], ..Block::default_sonic() })],
        "Block zero must have parent hash 0x0000000000000000000000000000000000000000000000000000000000000000",
        "Importing 1 blocks",
    )]
    #[case::parent_hash_mismatch(
        vec![],
        vec![
            Ok(Block { number: 1, parent_hash: [0; 32], ..Block::default_sonic() }),
            Ok(Block { number: 0, ..Block::default_sonic() })
        ],
        "Parent hash mismatch for block 1",
        "Importing 2 blocks",
    )]
    #[case::parent_hash_of_block_in_db_mismatches(
        vec![Block::default_sonic()],
        {
            let block_0 = Block { difficulty: 1, ..Block::default_sonic() };
            let block_0_hash = block_0.to_header().compute_hash();
            vec![
                Ok(Block { number: 1, parent_hash: block_0_hash, ..Block::default_sonic() }),
                Ok(block_0),
            ]
        },
        "Parent hash mismatch for block 1",
        "Importing 1 blocks",
    )]
    fn import_fails_verification(
        #[case] db_blocks: Vec<Block>,
        #[case] import_blocks: Vec<Result<Block, genesis_parser::Error>>,
        #[case] expected_error: &str,
        #[case] expected_output_contains: &str,
    ) {
        let chain_id = 123;
        let (_tmpdir, cfg) = test_config();

        let mut db = MockBlockDb::new();
        let ranges = if db_blocks.is_empty() {
            vec![]
        } else {
            let min = db_blocks.iter().map(|b| b.number).min().unwrap();
            let max = db_blocks.iter().map(|b| b.number).max().unwrap();
            vec![min..=max]
        };
        db.expect_get_ranges_of_chain_id()
            .with(eq(chain_id))
            .return_once(move |_| Ok(ranges));
        db.expect_batch().return_once(|| {
            let mut batch = MockBlockDbBatch::new();
            batch.expect_size().returning(|| 0);
            batch.expect_put_bytes().returning(|_, _, _| ());
            batch
        });
        for block in db_blocks {
            db.expect_get()
                .with(eq(chain_id), eq(block.number))
                .return_once(move |_, _| Ok(Some(block)));
        }

        let mut writer = Vec::new();
        let result = import(
            cfg,
            &mut db,
            import_blocks.into_iter(),
            chain_id,
            true,
            &CancellationToken::new(),
            &mut writer,
        );
        assert!(result.unwrap_err().to_string().contains(expected_error));

        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains(expected_output_contains));
    }

    #[test]
    fn import_fails_if_no_write_permissions() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (cfg, _) = open_app_dir(tmpdir.path(), false).unwrap();
        tmpdir.set_permissions(Permissions::ReadOnly).unwrap();

        let blocks = [Ok(Block {
            number: 0,
            ..Block::default_sonic()
        })];

        let mut db = MockBlockDb::new();
        let mut writer = Vec::new();
        let result = import(
            cfg,
            &mut db,
            blocks.into_iter(),
            1,
            false,
            &CancellationToken::new(),
            &mut writer,
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
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
        let chain_id = 123;
        let (_tmpdir, cfg) = test_config();

        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(chain_id))
            .return_once(move |_| Ok(vec![db_blocks_range]));
        if let Some(range) = &import_blocks_range {
            let count = range.clone().count();
            db.expect_batch().return_once(move || {
                let mut batch = MockBlockDbBatch::new();
                batch.expect_size().returning(|| 0);
                batch
                    .expect_put_bytes()
                    .times(count)
                    .returning(|_, _, _| ());
                batch
            });
            db.expect_write_batch().return_once(|_| Ok(()));
        }

        let mut writer = Vec::new();
        import(
            cfg,
            &mut db,
            available_blocks_range
                .clone()
                .map(|i| {
                    Ok(Block {
                        number: i,
                        difficulty: 1,
                        ..Block::default()
                    })
                })
                .rev(),
            chain_id,
            false,
            &CancellationToken::new(),
            &mut writer,
        )
        .unwrap();

        let output = String::from_utf8(writer).unwrap();
        if import_blocks_range.is_some() {
            assert!(output.contains("Importing"));
            assert!(output.contains("Wrote"));
        } else {
            assert!(output.contains("All blocks are already in the database, nothing to import"));
        }
    }

    #[test]
    fn import_fails_when_metadata_invalid() {
        let chain_id = 123;
        let (_tmpdir, cfg) = test_config();

        let mut db = MockBlockDb::new();
        // Claim blocks 0..=1 exist without storing them
        db.expect_get_ranges_of_chain_id()
            .with(eq(chain_id))
            .return_once(|_| Ok(vec![0..=1]));
        db.expect_batch().return_once(|| {
            let mut batch = MockBlockDbBatch::new();
            batch.expect_size().returning(|| 0);
            batch.expect_put_bytes().returning(|_, _, _| ());
            batch
        });
        db.expect_get()
            .with(eq(chain_id), eq(1))
            .return_once(|_, _| Ok(None));

        // 3 blocks in descending order; blocks 0..=1 are "already in db" per metadata
        let blocks = [2, 1, 0].map(|i| {
            Ok(Block {
                number: i,
                ..Block::default_sonic()
            })
        });

        let mut writer = Vec::new();
        assert_eq!(
            import(
                cfg,
                &mut db,
                blocks.into_iter(),
                chain_id,
                true,
                &CancellationToken::new(),
                &mut writer
            )
            .unwrap_err()
            .to_string(),
            "Invalid metadata, block 1 does not exist"
        );

        let output = String::from_utf8(writer).unwrap();
        let expected_output = indoc::formatdoc! {"
            Genesis file contains 3 blocks for chain ID {chain_id}
            Creating new entry for chain ID {chain_id} in the configuration
            Skipping 2 blocks that are already in the database
            Importing 1 blocks"
        };
        assert!(output.contains(&expected_output));
    }

    #[test]
    fn import_stops_import_and_flushes_batch_when_cancelled() {
        let chain_id = 123;
        let (_tmpdir, cfg) = test_config();

        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(chain_id))
            .return_once(|_| Ok(vec![]));
        db.expect_batch().return_once(|| {
            let mut batch = MockBlockDbBatch::new();
            batch.expect_size().returning(|| 0);
            batch.expect_put_bytes().times(1).returning(|_, _, _| ());
            batch
        });
        db.expect_write_batch().return_once(|_| Ok(()));

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
        let result = import(cfg, &mut db, blocks, chain_id, false, &token, &mut writer);
        assert!(result.is_ok());

        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains("Import cancelled."));
    }
}
