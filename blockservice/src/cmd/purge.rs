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
) -> Result<(), Box<dyn std::error::Error>> {
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
    let from = from.unwrap_or(*block_ranges.first().unwrap().start());
    let to = to.unwrap_or(*block_ranges.last().unwrap().end());
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

    // Read a character from stdin without the need for Enter
    reader.get_reader().read_line(&mut input)?;
    if matches!(input.trim(), "y" | "Y") {
        db.delete_range(chain_id, from, to)?;
        writeln!(writer, "Blocks successfully purged")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, vec};

    use bertha_types::Block;

    use super::*;
    use crate::{
        app_dir::init_app_dir,
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
        db.put_metadata_raw(42, vec![0].as_slice()).unwrap(); // Invalid metadata length
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
                .contains("error in underlying storage layer: invalid ranges for chain ID 42")
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
    fn can_be_called_with_chain_id_or_chain_id_and_start_or_chain_id_and_start_and_end() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let init_db = || {
            let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
            db.put(1, Block::default_sonic()).unwrap();
        };
        let check_empty_db = || {
            let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
            assert!(db.get(1, 0).unwrap().is_none());
        };

        // From is None, to is None
        {
            init_db();
            let mut output = Vec::new();
            assert!(purge(tmpdir.path(), 1, None, None, &mut output, &CONFIRM_PURGE).is_ok());
            check_empty_db();
            assert_eq!(
                String::from_utf8(output).unwrap(),
                format!(
                    "Purging 1 blocks in range 0 - 0 for chain ID 1. Are you sure you want to continue? (y/n): Blocks successfully purged\n"
                )
            );
        }
        // From is Some, to is None
        {
            init_db();
            let mut output = Vec::new();
            assert!(purge(tmpdir.path(), 1, Some(0), None, &mut output, &CONFIRM_PURGE).is_ok());
            check_empty_db();
            assert_eq!(
                String::from_utf8(output).unwrap(),
                format!(
                    "Purging 1 blocks in range 0 - 0 for chain ID 1. Are you sure you want to continue? (y/n): Blocks successfully purged\n"
                )
            );
        }
        // From is Some, to is Some
        {
            init_db();
            let mut output = Vec::new();
            assert!(
                purge(
                    tmpdir.path(),
                    1,
                    Some(0),
                    Some(1),
                    &mut output,
                    &CONFIRM_PURGE
                )
                .is_ok()
            );
            check_empty_db();
            assert_eq!(
                String::from_utf8(output).unwrap(),
                format!(
                    "Purging 1 blocks in range 0 - 1 for chain ID 1. Are you sure you want to continue? (y/n): Blocks successfully purged\n"
                )
            );
        }
    }

    #[test]
    fn deletes_range_of_blocks() {
        // this test is just supposed to check that the purge command calls
        // BlockRocksDb::delete_range, not that all the corner cases work because they are
        // already tested in BlockRocksDb::delete_range

        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let chain_id = 146;
        let init_db = || {
            let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();

            let mut block = Block::default();
            db.put(chain_id, block.clone()).unwrap();
            block.number = 1;
            db.put(chain_id, block.clone()).unwrap();
            block.number = 2;
            db.put(chain_id, block.clone()).unwrap();
            block.number = 3;
            db.put(chain_id, block.clone()).unwrap();
        };

        // from is Some, to is Some
        {
            init_db();
            let mut output = Vec::new();
            purge(
                tmpdir.path(),
                chain_id,
                Some(1),
                Some(2),
                &mut output,
                &CONFIRM_PURGE,
            )
            .expect("purge should succeed");
            assert_eq!(
                String::from_utf8(output).unwrap(),
                "Purging 2 blocks in range 1 - 2 for chain ID 146. Are you sure you want to continue? (y/n): Blocks successfully purged\n"
            );

            let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
            assert!(db.get(chain_id, 0).unwrap().is_some());
            assert!(db.get(chain_id, 1).unwrap().is_none());
            assert!(db.get(chain_id, 2).unwrap().is_none());
            assert!(db.get(chain_id, 3).unwrap().is_some());
        }
        // from is Some, to is None
        {
            init_db();
            let mut output = Vec::new();
            purge(
                tmpdir.path(),
                chain_id,
                Some(1),
                None,
                &mut output,
                &CONFIRM_PURGE,
            )
            .expect("purge should succeed");
            assert_eq!(
                String::from_utf8(output).unwrap(),
                "Purging 3 blocks in range 1 - 3 for chain ID 146. Are you sure you want to continue? (y/n): Blocks successfully purged\n"
            );
            let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
            assert!(db.get(chain_id, 0).unwrap().is_some());
            assert!(db.get(chain_id, 1).unwrap().is_none());
            assert!(db.get(chain_id, 2).unwrap().is_none());
            assert!(db.get(chain_id, 3).unwrap().is_none());
        }
        // from is None, to is Some
        {
            init_db();
            let mut output = Vec::new();
            purge(
                tmpdir.path(),
                chain_id,
                None,
                Some(2),
                &mut output,
                &CONFIRM_PURGE,
            )
            .expect("purge should succeed");
            assert_eq!(
                String::from_utf8(output).unwrap(),
                "Purging 3 blocks in range 0 - 2 for chain ID 146. Are you sure you want to continue? (y/n): Blocks successfully purged\n"
            );
            let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
            assert!(db.get(chain_id, 0).unwrap().is_none());
            assert!(db.get(chain_id, 1).unwrap().is_none());
            assert!(db.get(chain_id, 2).unwrap().is_none());
            assert!(db.get(chain_id, 3).unwrap().is_some());
        }
        // from is None, to is None
        {
            init_db();
            let mut output = Vec::new();
            purge(
                tmpdir.path(),
                chain_id,
                None,
                None,
                &mut output,
                &CONFIRM_PURGE,
            )
            .expect("purge should succeed");
            assert_eq!(
                String::from_utf8(output).unwrap(),
                "Purging 4 blocks in range 0 - 3 for chain ID 146. Are you sure you want to continue? (y/n): Blocks successfully purged\n"
            );
            let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
            assert!(db.get(chain_id, 0).unwrap().is_none());
            assert!(db.get(chain_id, 1).unwrap().is_none());
            assert!(db.get(chain_id, 2).unwrap().is_none());
            assert!(db.get(chain_id, 3).unwrap().is_none());
        }
    }

    #[test]
    fn cancel_operation_if_user_does_not_confirm() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
        db.put_raw(42, 1, vec![1, 2, 3].as_slice()).unwrap();
        drop(db);

        let mut output = Vec::new();
        purge(tmpdir.path(), 42, None, None, &mut output, &DENY_PURGE)
            .expect("purge should succeed");
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "Purging 1 blocks in range 1 - 1 for chain ID 42. Are you sure you want to continue? (y/n): "
        );

        let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
        assert_eq!(db.get_raw(42, 1).unwrap(), Some(vec![1, 2, 3]));
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

    #[test]
    fn guard_is_case_unsensitive() {
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
        // lowercase 'y'
        {
            set_elem();
            let mut output = Vec::new();
            purge(
                tmpdir.path(),
                42,
                None,
                None,
                &mut output,
                &Cursor::new("y"),
            )
            .expect("purge should succeed");
            let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
            let elem = db.get(42, 1).unwrap();
            assert!(elem.is_none());
            assert!(
                open_app_dir(tmpdir.path(), true)
                    .unwrap()
                    .1
                    .get(42, 1)
                    .unwrap()
                    .is_none()
            );
            let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
            assert!(db.get(42, 1).unwrap().is_none());
        }
        // uppercase 'Y'
        {
            set_elem();
            let mut output = Vec::new();
            purge(
                tmpdir.path(),
                42,
                None,
                None,
                &mut output,
                &Cursor::new("Y"),
            )
            .expect("purge should succeed");

            assert!(
                open_app_dir(tmpdir.path(), true)
                    .unwrap()
                    .1
                    .get(42, 1)
                    .unwrap()
                    .is_none()
            );
            let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
            assert!(db.get(42, 1).unwrap().is_none());
        }
    }
}
