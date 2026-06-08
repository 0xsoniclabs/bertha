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

use std::{io::BufRead, path::Path};

use crate::{
    app_dir::open_app_dir,
    db::BlockDb,
    utils::{InputReader, ranges::intersect_ranges},
};

/// Purges blocks from the local database for a specific chain.
/// If `from` is not provided, it defaults to 0.
/// If `to` is not provided, it defaults to the last local block of the chain.
pub fn purge(
    app_dir: impl AsRef<Path>,
    chain_id: u64,
    from: Option<u64>,
    to: Option<u64>,
    mut writer: impl std::io::Write,
    reader: &impl InputReader,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if chain_id == 0 {
        return Err("chain ID cannot be 0".into());
    }
    // Guard the purge command
    let (_, db) = open_app_dir(app_dir, false)?;
    let block_ranges = db.get_ranges_of_chain_id(chain_id)?;
    // Nothing to do
    if block_ranges.is_empty() {
        writeln!(writer, "No blocks found for chain ID {chain_id}")?;
        return Ok(());
    }

    // Safe to unwrap as it's not empty
    let from = from.unwrap_or(0);
    let to = to.unwrap_or(*block_ranges.last().unwrap().end());
    if from > to {
        return Err(format!(
            "invalid range: 'from' ({from}) must be less than or equal to 'to' ({to})",
        )
        .into());
    }

    let num_blocks_to_purge = intersect_ranges(from..=to, &block_ranges)
        .into_iter()
        .map(|range| range.end() - range.start() + 1)
        .sum::<u64>();
    if num_blocks_to_purge == 0 {
        writeln!(
            writer,
            "No blocks to purge found in range {from} - {to} for chain ID {chain_id}"
        )?;
        return Ok(());
    }

    let mut input = String::new();
    write!(
        writer,
        "Purging {num_blocks_to_purge} blocks in range {from} - {to} for chain ID {chain_id}. Are you sure you want to continue? (y/n): ",
    )?;
    writer.flush()?;

    // Read a character from stdin
    reader.get_reader().read_line(&mut input)?;
    if matches!(input.trim(), "y" | "Y") {
        db.delete_range(chain_id, from, to)?;
        writeln!(writer, "Blocks successfully purged")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use bertha_types::Block;
    use rstest::rstest;

    use super::*;
    use crate::{
        app_dir::init_app_dir,
        db::{BlockDb, CHAIN_IDS_KEY, KvDb, make_block_ranges_key, serialize_chain_ids},
        utils::test_dir::{Permissions, TestDir},
    };

    static CONFIRM_PURGE: Cursor<&'static str> = Cursor::new("y\n");
    static DENY_PURGE: Cursor<&'static str> = Cursor::new("n\n");

    #[test]
    fn fails_if_app_dir_is_not_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        let mut output = Vec::new();
        let result = purge(tmpdir.path(), 1, None, None, &mut output, &CONFIRM_PURGE);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
        assert!(output.is_empty());
    }

    #[test]
    fn fails_if_no_write_permissions() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        // create database
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        tmpdir.set_permissions(Permissions::WriteOnly).unwrap();

        let mut output = Vec::new();
        let result = purge(tmpdir.path(), 1, None, None, &mut output, &CONFIRM_PURGE);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
        assert!(output.is_empty());
    }

    #[test]
    fn fails_for_invalid_stored_chain_ids() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
        db.kv_db()
            .put_raw(&make_block_ranges_key(42), &[0]) // invalid value for block ranges
            .unwrap();
        db.kv_db()
            .put_raw(&CHAIN_IDS_KEY, &serialize_chain_ids([42]))
            .unwrap();
        drop(db);

        let err = purge(
            tmpdir.path(),
            42,
            None,
            None,
            std::io::sink(),
            &CONFIRM_PURGE,
        )
        .expect_err("purge should fail");
        assert!(
            err.to_string()
                .contains("error in underlying storage layer: invalid block ranges length")
        );
    }

    #[tokio::test]
    async fn fails_for_chain_id_zero() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let mut writer = Vec::new();
        let result = purge(tmpdir.path(), 0, None, None, &mut writer, &CONFIRM_PURGE);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "chain ID cannot be 0");
    }

    #[test]
    fn fails_for_invalid_range() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
        db.put(1, Block::default_sonic()).unwrap();
        drop(db);

        let mut output = Vec::new();
        let result = purge(
            tmpdir.path(),
            1,
            Some(2),
            Some(1),
            &mut output,
            &CONFIRM_PURGE,
        );
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "invalid range: 'from' (2) must be less than or equal to 'to' (1)"
        );
        assert!(output.is_empty());
    }

    #[rstest]
    #[case::full_range(None, None)]
    #[case::with_start(Some(1), None)]
    #[case::with_end(None, Some(2))]
    #[case::with_start_and_end(Some(1), Some(2))]
    fn deletes_range_of_blocks(#[case] from: Option<u64>, #[case] to: Option<u64>) {
        // this test is just supposed to check that the purge command calls
        // BlockRocksDb::delete_range, not that all the corner cases work because they are
        // already tested in BlockRocksDb::delete_range

        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let chain_id = 146;
        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();

        let mut block = Block::default();
        db.put(chain_id, block.clone()).unwrap();
        block.number = 1;
        db.put(chain_id, block.clone()).unwrap();
        block.number = 2;
        db.put(chain_id, block.clone()).unwrap();
        block.number = 3;
        db.put(chain_id, block).unwrap();
        drop(db);

        let mut output = Vec::new();
        purge(
            tmpdir.path(),
            chain_id,
            from,
            to,
            &mut output,
            &CONFIRM_PURGE,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!(
                "Purging {} blocks in range {} - {} for chain ID 146. Are you sure you want to continue? (y/n): Blocks successfully purged\n",
                to.unwrap_or(3) - from.unwrap_or(0) + 1,
                from.unwrap_or(0),
                to.unwrap_or(3)
            )
        );

        let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
        assert_eq!(
            db.get(chain_id, 0).unwrap().is_none(),
            (from.unwrap_or(0)..=to.unwrap_or(3)).contains(&0)
        );
        assert_eq!(
            db.get(chain_id, 1).unwrap().is_none(),
            (from.unwrap_or(0)..=to.unwrap_or(3)).contains(&1)
        );
        assert_eq!(
            db.get(chain_id, 2).unwrap().is_none(),
            (from.unwrap_or(0)..=to.unwrap_or(3)).contains(&2)
        );
        assert_eq!(
            db.get(chain_id, 3).unwrap().is_none(),
            (from.unwrap_or(0)..=to.unwrap_or(3)).contains(&3)
        );
    }

    #[test]
    fn cancel_operation_if_user_does_not_confirm() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
        db.put_bytes(42, 1, &[1, 2, 3]).unwrap();
        drop(db);

        let mut output = Vec::new();
        purge(tmpdir.path(), 42, None, None, &mut output, &DENY_PURGE)
            .expect("purge should succeed");
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "Purging 1 blocks in range 0 - 1 for chain ID 42. Are you sure you want to continue? (y/n): "
        );

        let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
        assert_eq!(db.get_bytes(42, 1).unwrap(), Some(vec![1, 2, 3]));
    }

    #[test]
    fn cancel_operation_if_db_is_empty() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let mut output = Vec::new();
        purge(tmpdir.path(), 42, None, None, &mut output, &CONFIRM_PURGE)
            .expect("purge should succeed");
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "No blocks found for chain ID 42\n"
        );
    }

    #[test]
    fn cancel_operation_if_range_to_purge_is_empty() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
        db.put(
            42,
            Block {
                number: 1,
                ..Default::default()
            },
        )
        .unwrap();
        drop(db);

        let mut output = Vec::new();
        purge(
            tmpdir.path(),
            42,
            Some(3),
            Some(4), // range is not in the local db
            &mut output,
            &CONFIRM_PURGE,
        )
        .expect("purge should succeed");
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "No blocks to purge found in range 3 - 4 for chain ID 42\n"
        );
    }

    #[rstest]
    #[case::lowercase("y")]
    #[case::uppercase("Y")]
    fn guard_is_case_unsensitive(#[case] confirmation: &str) {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let set_elem = || {
            let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
            db.put(
                42,
                Block {
                    number: 1,
                    ..Default::default()
                },
            )
            .unwrap();
        };
        set_elem();
        let mut output = Vec::new();
        purge(
            tmpdir.path(),
            42,
            None,
            None,
            &mut output,
            &Cursor::new(confirmation),
        )
        .unwrap();

        let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
        assert!(db.get(42, 1).unwrap().is_none());
    }
}
