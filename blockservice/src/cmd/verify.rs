// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

use std::{cmp, collections::HashSet, path::Path};

use bertha_types::{Hash, HexConvert};
use serde::Deserialize;

use crate::{
    app_dir::open_app_dir,
    cmd::{CancelIndicator, make_progress_bar},
    db::{BlockDb, IterationDirection},
};

pub fn verify(
    app_dir: impl AsRef<Path>,
    chain_id: u64,
    block_number: Option<u64>,
    block_hash: Option<Hash>,
    cancellation_token: &impl CancelIndicator,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (_cfg, db) = open_app_dir(app_dir, true)?;

    let blocks_with_failed_bundles: HashSet<u64> = match db.get_failed_bundles(chain_id)? {
        Some(data) => serde_json::from_slice::<Vec<FailedBundle>>(&data)
            .map_err(|err| format!("failed to parse stored failed bundles: {err}"))?
            .into_iter()
            .map(|failed_bundle| failed_bundle.block)
            .collect(),
        None => HashSet::new(),
    };

    let mut errors = 0;
    let mut warned = false;

    if let (Some(block_number), Some(expected_hash)) = (block_number, block_hash) {
        if let Some(block) = db.get(chain_id, block_number)? {
            check_block_hash(
                chain_id,
                block_number,
                block.to_header().compute_hash(),
                expected_hash,
                "the given hash",
                &blocks_with_failed_bundles,
                &mut warned,
                &mut errors,
                &mut writer,
            )?;
        } else {
            errors += 1;
            writeln!(
                writer,
                "[chain ID {chain_id}] requested block {block_number} does not exit"
            )?;
        }
    }

    // start with the first block if no block number is provided
    let block_number = block_number.unwrap_or_default();
    let total_blocks = db
        .get_ranges_of_chain_id(chain_id)?
        .into_iter()
        .map(|range| {
            let start = cmp::max(*range.start(), block_number);
            let end = *range.end();
            if start > end {
                0
            } else {
                end.saturating_sub(start).saturating_add(1)
            }
        })
        .sum();

    let progress_bar = make_progress_bar(total_blocks)?;

    let mut prev_block_number = block_number;
    let mut prev_block_hash: Option<Hash> = None;
    for entry in db.iterate(chain_id, block_number, IterationDirection::Forward) {
        if cancellation_token.is_cancelled() {
            writeln!(writer, "Verification cancelled.")?;
            break;
        }

        let (block_number, block) = entry?;
        if block.number != block_number {
            errors += 1;
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
            check_block_hash(
                chain_id,
                prev_block_number,
                prev_block_hash,
                block.parent_hash,
                format_args!("the parent hash in block {block_number}"),
                &blocks_with_failed_bundles,
                &mut warned,
                &mut errors,
                &mut writer,
            )?;
        }
        prev_block_number = block_number;
        prev_block_hash = Some(block.to_header().compute_hash());
        progress_bar.inc(1);
    }
    progress_bar.finish();

    if errors == 0 {
        writeln!(
            writer,
            "[chain ID {chain_id}] Blocks verified successfully."
        )?;
    } else {
        writeln!(
            writer,
            "[chain ID {chain_id}] Verification completed with {errors} errors."
        )?;
    }

    Ok(())
}

/// An entry of the stored failed bundles, of which only the block is read.
#[derive(Deserialize)]
struct FailedBundle {
    block: u64,
}

/// Checks `block_hash` of the given block against `expected_hash`, taken from `expected_source`,
/// and reports a mismatch, counted in `errors`. A mismatch of a block with failed bundles is
/// tolerated, with a warning about the first one.
#[allow(clippy::too_many_arguments)]
fn check_block_hash(
    chain_id: u64,
    block_number: u64,
    block_hash: Hash,
    expected_hash: Hash,
    expected_source: impl std::fmt::Display,
    blocks_with_failed_bundles: &HashSet<u64>,
    warned: &mut bool,
    errors: &mut u64,
    mut writer: impl std::io::Write,
) -> std::io::Result<()> {
    if block_hash == expected_hash {
        return Ok(());
    }
    if blocks_with_failed_bundles.contains(&block_number) {
        if !*warned {
            *warned = true;
            writeln!(
                writer,
                "[chain ID {chain_id}] warning: tolerated hash mismatch of block {block_number}, which contains failed bundles; further tolerated hash mismatches are not reported."
            )?;
        }
        return Ok(());
    }
    *errors += 1;
    writeln!(
        writer,
        "[chain ID {chain_id}] hash verification failed for block {block_number}: hash is {}, but {expected_source} is {}.",
        block_hash.to_hex(),
        expected_hash.to_hex()
    )
}

#[cfg(test)]
mod tests {

    use bertha_types::Block;
    use mockall::Sequence;
    use prost::Message;
    use rstest::rstest;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::{
        app_dir::init_app_dir,
        cmd::MockCancelIndicator,
        db::proto,
        utils::test_dir::{Permissions, TestDir},
    };

    #[test]
    fn fails_if_app_dir_is_not_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        let result = verify(
            tmpdir.path(),
            0,
            None,
            None,
            &CancellationToken::new(),
            std::io::sink(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }

    #[test]
    fn fails_if_no_read_permissions() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        // create database
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        // remove read permissions
        tmpdir.set_permissions(Permissions::WriteOnly).unwrap();

        let result = verify(
            tmpdir.path(),
            0,
            None,
            None,
            &CancellationToken::new(),
            std::io::sink(),
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
    }

    #[rstest]
    #[case::correct_hash(true)]
    #[case::incorrect_hash(false)]
    fn checks_hash_of_block(#[case] correct_hash: bool) {
        let chain_id = 146;

        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (_, mut db) = open_app_dir(tmpdir.path(), false).unwrap();

        let block = Block::default();
        db.put(chain_id, block.clone()).unwrap();

        let hash = if correct_hash {
            block.to_header().compute_hash()
        } else {
            Hash::default() // intentionally wrong hash
        };

        let mut output = Vec::new();
        let result = verify(
            tmpdir.path(),
            chain_id,
            Some(block.number),
            Some(hash),
            &CancellationToken::new(),
            &mut output,
        );
        assert!(result.is_ok());

        if correct_hash {
            assert_eq!(
                output,
                format!("[chain ID {chain_id}] Blocks verified successfully.\n").as_bytes()
            );
        } else {
            assert_eq!(output, format!(
                "[chain ID {chain_id}] hash verification failed for block {}: hash is {}, but the given hash is {}.\n[chain ID {chain_id}] Verification completed with 1 errors.\n",
                block.number,
                block.to_header().compute_hash().to_hex(),
                hash.to_hex(),
            ).as_bytes());
        };
    }

    #[test]
    fn prints_message_if_block_not_found() {
        let chain_id = 146;
        let block_number = 0;

        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let mut buf = Vec::new();
        let result = verify(
            tmpdir.path(),
            chain_id,
            Some(block_number),
            Some(Hash::default()),
            &CancellationToken::new(),
            &mut buf,
        );
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!(
                "[chain ID {chain_id}] requested block {block_number} does not exit\n[chain ID {chain_id}] Verification completed with 1 errors.\n"
            )
        );
    }

    #[rstest]
    #[case::matching_number(true)]
    #[case::mismatching_number(false)]
    fn checks_number_of_block(#[case] matching_number: bool) {
        let chain_id = 146;

        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (_, mut db) = open_app_dir(tmpdir.path(), false).unwrap();

        let mut block = Block::default();
        let block_number = if matching_number {
            db.put(chain_id, block.clone()).unwrap();
            block.number
        } else {
            // at block number 0, blocknumber (0) and block.number (1) mismatch
            block.number = 1;
            let block_number = 0; // intentionally wrong block number
            let data = proto::Block::from(block.clone()).encode_to_vec();
            db.put_bytes(chain_id, block_number, &data).unwrap();
            block_number
        };

        let mut output = Vec::new();
        let result = verify(
            tmpdir.path(),
            chain_id,
            None,
            None,
            &CancellationToken::new(),
            &mut output,
        );
        assert!(result.is_ok());

        if matching_number {
            assert_eq!(
                output,
                format!("[chain ID {chain_id}] Blocks verified successfully.\n").as_bytes()
            );
        } else {
            assert_eq!(
                output,
                format!(
                    "[chain ID {chain_id}] block number mismatch: block number in key = {block_number}, block.number = {}.\n\
                    [chain ID {chain_id}] Verification completed with 1 errors.\n",
                    block.number
                ).as_bytes()
            );
        };
    }

    #[rstest]
    #[case::correct_parent_hash(true)]
    #[case::incorrect_parent_hash(false)]
    fn checks_parent_hash_of_block(#[case] correct_parent_hash: bool) {
        let chain_id = 146;

        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (_, mut db) = open_app_dir(tmpdir.path(), false).unwrap();

        let block0 = Block::default();
        db.put(chain_id, block0.clone()).unwrap();

        // correct hash
        let mut block1 = Block {
            number: 1,
            parent_hash: block0.to_header().compute_hash(),
            ..Block::default()
        };

        if !correct_parent_hash {
            // intentionally wrong parent hash
            block1.parent_hash = Hash::default();
        }
        db.put(chain_id, block1.clone()).unwrap();

        let mut output = Vec::new();
        let result = verify(
            tmpdir.path(),
            chain_id,
            None,
            None,
            &CancellationToken::new(),
            &mut output,
        );
        assert!(result.is_ok());

        if correct_parent_hash {
            assert_eq!(
                output,
                format!("[chain ID {chain_id}] Blocks verified successfully.\n").as_bytes()
            );
        } else {
            assert_eq!(output, format!(
                "[chain ID {chain_id}] hash verification failed for block {}: hash is {}, but the parent hash in block {} is {}.\n[chain ID {chain_id}] Verification completed with 1 errors.\n",
                block0.number,
                block0.to_header().compute_hash().to_hex(),
                block1.number,
                block1.parent_hash.to_hex()
            ).as_bytes());
        };
    }

    #[rstest]
    #[case::block_with_failed_bundles(true)]
    #[case::block_without_failed_bundles(false)]
    fn tolerates_hash_mismatches_only_of_blocks_with_failed_bundles(
        #[case] has_failed_bundles: bool,
    ) {
        let chain_id = 146;

        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (_, mut db) = open_app_dir(tmpdir.path(), false).unwrap();

        // The hash of block 2 is not checked, as it has no successor.
        let failed_bundles: &[u8] = if has_failed_bundles {
            br#"[{"block": 0, "pos": 0, "tx": "0x01"}, {"block": 1, "pos": 0, "tx": "0x01"}]"#
        } else {
            br#"[{"block": 2, "pos": 0, "tx": "0x01"}]"#
        };
        db.put_failed_bundles(chain_id, failed_bundles).unwrap();
        // The parent hashes of blocks 1 and 2 stand for the actual hashes of their parents.
        let blocks: Vec<_> = (0..3)
            .map(|number| Block {
                number,
                ..Block::default()
            })
            .collect();
        for block in &blocks {
            db.put(chain_id, block.clone()).unwrap();
        }
        drop(db);

        let mut output = Vec::new();
        verify(
            tmpdir.path(),
            chain_id,
            Some(0),
            Some(Hash::default()),
            &CancellationToken::new(),
            &mut output,
        )
        .unwrap();

        let hash = |number: usize| blocks[number].to_header().compute_hash().to_hex();
        let expected = if has_failed_bundles {
            format!(
                "[chain ID {chain_id}] warning: tolerated hash mismatch of block 0, which contains failed bundles; further tolerated hash mismatches are not reported.\n\
                [chain ID {chain_id}] Blocks verified successfully.\n"
            )
        } else {
            format!(
                "[chain ID {chain_id}] hash verification failed for block 0: hash is {}, but the given hash is {zero}.\n\
                [chain ID {chain_id}] hash verification failed for block 0: hash is {}, but the parent hash in block 1 is {zero}.\n\
                [chain ID {chain_id}] hash verification failed for block 1: hash is {}, but the parent hash in block 2 is {zero}.\n\
                [chain ID {chain_id}] Verification completed with 3 errors.\n",
                hash(0),
                hash(0),
                hash(1),
                zero = Hash::default().to_hex(),
            )
        };
        assert_eq!(String::from_utf8(output).unwrap(), expected);
    }

    #[test]
    fn fails_on_invalid_stored_failed_bundles() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (_, mut db) = open_app_dir(tmpdir.path(), false).unwrap();
        db.put_failed_bundles(146, b"not-json").unwrap();
        drop(db);

        let result = verify(
            tmpdir.path(),
            146,
            None,
            None,
            &CancellationToken::new(),
            std::io::sink(),
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("failed to parse stored failed bundles")
        );
    }

    #[test]
    fn skips_parent_hash_check_between_disjoint_ranges() {
        let chain_id = 146;

        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (_, mut db) = open_app_dir(tmpdir.path(), false).unwrap();

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
        let result = verify(
            tmpdir.path(),
            chain_id,
            None,
            None,
            &CancellationToken::new(),
            &mut buf,
        );
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!("[chain ID {chain_id}] Blocks verified successfully.\n",)
        );
    }

    #[test]
    fn stops_verification_when_cancelled() {
        let chain_id = 146;

        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (_, mut db) = open_app_dir(tmpdir.path(), false).unwrap();

        let block0 = Block {
            number: 0,
            parent_hash: Hash::default(),
            ..Block::default()
        };
        let block1 = Block {
            number: 1,
            parent_hash: block0.to_header().compute_hash(),
            ..Block::default()
        };
        db.put(chain_id, block0).unwrap();
        db.put(chain_id, block1).unwrap();
        drop(db);

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

        let mut output = Vec::new();
        let result = verify(tmpdir.path(), chain_id, None, None, &token, &mut output);
        assert!(result.is_ok());

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Verification cancelled."));
        assert!(output.contains("Blocks verified successfully"));
    }
}
