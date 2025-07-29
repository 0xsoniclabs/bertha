use std::{io::Write, path::Path};

use crate::{app_dir::open_app_dir, db::BlockDb};

/// Purges blocks from the local database for a specific chain.
/// If `from` is not provided, it defaults to 0.
/// If `to` is not provided, it defaults to the last local block of the chain.
pub fn purge(
    app_dir: impl AsRef<Path>,
    chain_id: u64,
    from: Option<u64>,
    to: Option<u64>,
    mut writer: impl std::io::Write,
    mut reader: impl std::io::BufRead,
) -> Result<(), Box<dyn std::error::Error>> {
    // Guard the purge command
    let mut input = String::new();
    write!(
        writer,
        "Are you sure you want to purge blocks for chain {chain_id}? (y/n): "
    )?;
    std::io::stdout().flush()?;
    reader.read_line(&mut input)?;
    if matches!(input.trim(), "y" | "Y") {
        let (_cfg, db) = open_app_dir(app_dir, false)?;
        db.delete_range(chain_id, from.unwrap_or(0), to.unwrap_or(u64::MAX))?;
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

    /// Helper function to simulate user confirmation for the purge command.
    fn confirm_purge(bool: bool) -> impl std::io::BufRead {
        let input = if bool { "y\n" } else { "n\n" };
        Cursor::new(input)
    }

    #[test]
    fn fails_if_app_dir_is_not_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        let mut writer = Vec::new();
        let result = purge(
            tmpdir.path(),
            0,
            None,
            None,
            &mut writer,
            confirm_purge(true),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
        assert_eq!(
            String::from_utf8(writer).unwrap(),
            format!("Are you sure you want to purge blocks for chain 0? (y/n): ")
        );
    }

    #[test]
    fn fails_if_no_write_permissions() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        // create database
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        tmpdir.set_permissions(Permissions::WriteOnly).unwrap();

        let mut writer = Vec::new();
        let result = purge(
            tmpdir.path(),
            0,
            None,
            None,
            &mut writer,
            confirm_purge(true),
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
        assert_eq!(
            String::from_utf8(writer).unwrap(),
            format!("Are you sure you want to purge blocks for chain 0? (y/n): ")
        );
    }

    #[test]
    fn can_be_called_with_chain_id_or_chain_id_and_start_or_chain_id_and_start_and_end() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        // From is None, to is None
        {
            let mut writer = Vec::new();
            assert!(
                purge(
                    tmpdir.path(),
                    0,
                    None,
                    None,
                    &mut writer,
                    confirm_purge(true)
                )
                .is_ok()
            );
            assert_eq!(
                String::from_utf8(writer).unwrap(),
                format!("Are you sure you want to purge blocks for chain 0? (y/n): ")
            );
        }
        // From is Some, to is None
        {
            let mut writer = Vec::new();
            assert!(
                purge(
                    tmpdir.path(),
                    0,
                    Some(0),
                    None,
                    &mut writer,
                    confirm_purge(true)
                )
                .is_ok()
            );
            assert_eq!(
                String::from_utf8(writer).unwrap(),
                format!("Are you sure you want to purge blocks for chain 0? (y/n): ")
            );
        }
        // From is Some, to is Some
        {
            let mut writer = Vec::new();
            assert!(
                purge(
                    tmpdir.path(),
                    0,
                    Some(0),
                    Some(1),
                    &mut writer,
                    confirm_purge(true)
                )
                .is_ok()
            );

            assert_eq!(
                String::from_utf8(writer).unwrap(),
                format!("Are you sure you want to purge blocks for chain 0? (y/n): ")
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
            let mut writer = Vec::new();
            purge(
                tmpdir.path(),
                chain_id,
                Some(1),
                Some(2),
                &mut writer,
                confirm_purge(true),
            )
            .unwrap();
            assert_eq!(
                String::from_utf8(writer).unwrap(),
                format!("Are you sure you want to purge blocks for chain {chain_id}? (y/n): ")
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
            let mut writer = Vec::new();
            purge(
                tmpdir.path(),
                chain_id,
                Some(1),
                None,
                &mut writer,
                confirm_purge(true),
            )
            .unwrap();
            assert_eq!(
                String::from_utf8(writer).unwrap(),
                format!("Are you sure you want to purge blocks for chain {chain_id}? (y/n): ")
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
            let mut writer = Vec::new();
            purge(
                tmpdir.path(),
                chain_id,
                None,
                Some(2),
                &mut writer,
                confirm_purge(true),
            )
            .unwrap();
            assert_eq!(
                String::from_utf8(writer).unwrap(),
                format!("Are you sure you want to purge blocks for chain {chain_id}? (y/n): ")
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
            let mut writer = Vec::new();
            purge(
                tmpdir.path(),
                chain_id,
                None,
                None,
                &mut writer,
                confirm_purge(true),
            )
            .unwrap();
            assert_eq!(
                String::from_utf8(writer).unwrap(),
                format!("Are you sure you want to purge blocks for chain {chain_id}? (y/n): ")
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

        let mut writer = Vec::new();
        purge(
            tmpdir.path(),
            0,
            None,
            None,
            &mut writer,
            confirm_purge(false),
        )
        .expect("purge should succeed");
        assert_eq!(db.get_raw(42, 1).unwrap(), Some(vec![1, 2, 3]));

        assert_eq!(
            String::from_utf8(writer).unwrap(),
            format!("Are you sure you want to purge blocks for chain 0? (y/n): ")
        );
    }

    #[test]
    fn guard_is_case_unsensitive() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let set_elem = || {
            let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
            db.put_raw(42, 1, vec![1, 2, 3].as_slice()).unwrap();
        };
        // lowercase 'y'
        {
            set_elem();
            let mut writer = Vec::new();
            purge(tmpdir.path(), 42, None, None, &mut writer, Cursor::new("y"))
                .expect("purge should succeed");
            assert_eq!(
                String::from_utf8(writer).unwrap(),
                format!("Are you sure you want to purge blocks for chain 42? (y/n): ")
            );
            let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
            assert!(db.get(42, 1).unwrap().is_none());
        }
        // uppercase 'Y'
        {
            set_elem();
            let mut writer = Vec::new();
            purge(tmpdir.path(), 42, None, None, &mut writer, Cursor::new("Y"))
                .expect("purge should succeed");
            assert_eq!(
                String::from_utf8(writer).unwrap(),
                format!("Are you sure you want to purge blocks for chain 42? (y/n): ")
            );
            let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
            assert!(db.get(42, 1).unwrap().is_none());
        }
    }
}
