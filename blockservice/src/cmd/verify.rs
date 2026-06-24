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

use std::{cmp, path::Path};

use bertha_types::{Hash, HexConvert};

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

    let mut errors = 0;

    if let (Some(block_number), Some(expected_hash)) = (block_number, block_hash) {
        if let Some(block) = db.get(chain_id, block_number)? {
            let block_hash = block.to_header().compute_hash();
            if block_hash != expected_hash {
                errors += 1;
                writeln!(
                    writer,
                    "[chain ID {}] block hash verification failed for block {}: expected hash {}, got {}.",
                    chain_id,
                    block_number,
                    expected_hash.to_hex(),
                    block_hash.to_hex()
                )?;
            }
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
        if let Some(prev_block_hash) = prev_block_hash
            && block.parent_hash != prev_block_hash
        {
            errors += 1;
            writeln!(
                writer,
                "[chain ID {}] parent hash verification failed for block {}: expected hash {}, got {}.",
                chain_id,
                block_number,
                prev_block_hash.to_hex(),
                block.parent_hash.to_hex()
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
                "[chain ID {chain_id}] block hash verification failed for block {}: expected hash {}, got {}.\n[chain ID {chain_id}] Verification completed with 1 errors.\n",
                block.number,
                hash.to_hex(),
                block.to_header().compute_hash().to_hex(),
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
                "[chain ID {chain_id}] parent hash verification failed for block {}: expected hash {}, got {}.\n[chain ID {chain_id}] Verification completed with 1 errors.\n",
                block1.number,
                block0.to_header().compute_hash().to_hex(),
                block1.parent_hash.to_hex()
            ).as_bytes());
        };
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
