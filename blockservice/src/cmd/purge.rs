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
    writer: impl std::io::Write,
    reader: &impl InputReader,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (_, mut db) = open_app_dir(app_dir, false)?;

    purge_internal(&mut db, chain_id, from, to, writer, reader)
}

fn purge_internal(
    db: &mut impl BlockDb,
    chain_id: u64,
    from: Option<u64>,
    to: Option<u64>,
    mut writer: impl std::io::Write,
    reader: &impl InputReader,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

    use mockall::predicate::eq;
    use rstest::rstest;

    use super::*;
    use crate::{
        app_dir::init_app_dir,
        db::MockBlockDb,
        error::Error,
        utils::test_dir::{Permissions, TestDir},
    };

    static CONFIRM_PURGE: Cursor<&'static str> = Cursor::new("y\n");
    static DENY_PURGE: Cursor<&'static str> = Cursor::new("n\n");

    #[test]
    fn purge_fails_if_app_dir_is_not_initialized() {
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
    fn purge_fails_if_no_write_permissions() {
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
    fn purge_internal_fails_for_invalid_stored_chain_ids() {
        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(42))
            .return_once(|_| {
                Err(Error::StorageLayer(
                    "invalid block ranges length".to_string(),
                ))
            });

        let err = purge_internal(&mut db, 42, None, None, std::io::sink(), &CONFIRM_PURGE)
            .expect_err("purge should fail");
        assert!(
            err.to_string()
                .contains("error in underlying storage layer: invalid block ranges length")
        );
    }

    #[test]
    fn purge_internal_fails_for_invalid_range() {
        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(1))
            .return_once(|_| Ok(vec![0..=5]));

        let mut output = Vec::new();
        let result = purge_internal(&mut db, 1, Some(2), Some(1), &mut output, &CONFIRM_PURGE);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "invalid range: 'from' (2) must be less than or equal to 'to' (1)"
        );
        assert!(output.is_empty());
    }

    #[rstest]
    #[case::full_range(None, None, 0, 3, 4)]
    #[case::with_start(Some(1), None, 1, 3, 3)]
    #[case::with_end(None, Some(2), 0, 2, 3)]
    #[case::with_start_and_end(Some(1), Some(2), 1, 2, 2)]
    fn purge_internal_deletes_range_of_blocks(
        #[case] from: Option<u64>,
        #[case] to: Option<u64>,
        #[case] expected_from: u64,
        #[case] expected_to: u64,
        #[case] expected_count: u64,
    ) {
        // this test is just supposed to check that the purge command calls
        // BlockDb::delete_range, not that all the corner cases work because they are
        // already tested in BlockRocksDb::delete_range

        let chain_id = 146;
        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(chain_id))
            .return_once(move |_| Ok(vec![0..=3]));
        db.expect_delete_range()
            .with(eq(chain_id), eq(expected_from), eq(expected_to))
            .return_once(|_, _, _| Ok(()));

        let mut output = Vec::new();
        purge_internal(&mut db, chain_id, from, to, &mut output, &CONFIRM_PURGE).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!(
                "Purging {expected_count} blocks in range {expected_from} - {expected_to} for chain ID {chain_id}. Are you sure you want to continue? (y/n): Blocks successfully purged\n",
            )
        );
    }

    #[test]
    fn purge_internal_cancel_operation_if_user_does_not_confirm() {
        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(42))
            .return_once(|_| Ok(vec![1..=1]));
        db.expect_delete_range().never();

        let mut output = Vec::new();
        purge_internal(&mut db, 42, None, None, &mut output, &DENY_PURGE)
            .expect("purge should succeed");
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "Purging 1 blocks in range 0 - 1 for chain ID 42. Are you sure you want to continue? (y/n): "
        );
    }

    #[test]
    fn purge_internal_cancel_operation_if_db_is_empty() {
        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(42))
            .return_once(|_| Ok(vec![]));

        let mut output = Vec::new();
        purge_internal(&mut db, 42, None, None, &mut output, &CONFIRM_PURGE)
            .expect("purge should succeed");
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "No blocks found for chain ID 42\n"
        );
    }

    #[test]
    fn purge_internal_cancel_operation_if_range_to_purge_is_empty() {
        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(42))
            .return_once(|_| Ok(vec![1..=1]));

        let mut output = Vec::new();
        purge_internal(
            &mut db,
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
    fn purge_internal_guard_is_case_unsensitive(#[case] confirmation: &str) {
        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(42))
            .return_once(move |_| Ok(vec![1..=1]));
        db.expect_delete_range()
            .with(eq(42), eq(0u64), eq(1u64))
            .return_once(|_, _, _| Ok(()));

        purge_internal(
            &mut db,
            42,
            None,
            None,
            std::io::sink(),
            &Cursor::new(confirmation),
        )
        .unwrap();
    }
}
