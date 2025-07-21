use std::{io::Write, path::Path};

use crate::{app_dir::open_app_dir, db::BlockDb};

pub fn purge(
    app_dir: impl AsRef<Path>,
    chain_id: u64,
    from: Option<u64>,
    to: Option<u64>,
    mut reader: impl std::io::BufRead,
) -> Result<(), Box<dyn std::error::Error>> {
    // Guard the purge command
    let mut input = String::new();
    print!("Are you sure you want to purge blocks for chain {chain_id}? (y/n): ");
    std::io::stdout().flush()?;
    reader.read_line(&mut input)?;
    match input.trim() {
        "y" | "Y" => {
            let db = open_app_dir(app_dir, false)?;
            db.delete_range(chain_id, from, to)?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, os::unix::fs::PermissionsExt, vec};

    use bertha_types::Block;

    use super::*;
    use crate::app_dir::{BLOCK_DB_NAME, init_app_dir};

    /// Helper function to simulate user confirmation for the purge command.
    fn confirm_purge(bool: bool) -> impl std::io::BufRead {
        let input = if bool { "y\n" } else { "n\n" };
        Cursor::new(input)
    }

    #[test]
    fn fails_if_app_dir_is_not_initialized() {
        let tmpdir = tempfile::tempdir().unwrap();

        // set the stdin
        let result = purge(tmpdir.path(), 0, None, None, confirm_purge(true));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no database found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }

    #[test]
    fn fails_if_no_write_permissions() {
        let tmpdir = tempfile::tempdir().unwrap();

        // create database
        init_app_dir(tmpdir.path()).unwrap();

        // remove write permissions
        std::fs::set_permissions(
            tmpdir.path().join(BLOCK_DB_NAME),
            std::fs::Permissions::from_mode(0o555),
        )
        .unwrap();

        let result = purge(tmpdir.path(), 0, None, None, confirm_purge(true));
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
    }

    #[test]
    fn can_be_called_with_chain_id_or_chain_id_and_start_or_chain_id_and_start_and_end() {
        let tmpdir = tempfile::tempdir().unwrap();

        init_app_dir(tmpdir.path()).unwrap();

        assert!(purge(tmpdir.path(), 0, None, None, confirm_purge(true)).is_ok());
        assert!(purge(tmpdir.path(), 0, Some(0), None, confirm_purge(true)).is_ok());
        assert!(purge(tmpdir.path(), 0, Some(0), Some(1), confirm_purge(true)).is_ok());
    }

    #[test]
    fn deletes_range_of_blocks() {
        // this test is just supposed to check that the purge command calls
        // BlockRocksDb::delete_range, not that all the corner cases work because they are
        // already tested in BlockRocksDb::delete_range

        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();
        let db = open_app_dir(tmpdir.path(), false).unwrap();

        let chain_id = 146;
        let mut block = Block::default();
        db.put(chain_id, block.clone()).unwrap();
        block.number = 1;
        db.put(chain_id, block.clone()).unwrap();
        block.number = 2;
        db.put(chain_id, block.clone()).unwrap();
        block.number = 3;
        db.put(chain_id, block.clone()).unwrap();

        drop(db); // close the database to ensure that the purge command can open it

        purge(
            tmpdir.path(),
            chain_id,
            Some(1),
            Some(2),
            confirm_purge(true),
        )
        .unwrap();

        let db = open_app_dir(tmpdir.path(), false).unwrap();
        assert!(db.get(chain_id, 0).unwrap().is_some());
        assert!(db.get(chain_id, 1).unwrap().is_none());
        assert!(db.get(chain_id, 2).unwrap().is_none());
        assert!(db.get(chain_id, 3).unwrap().is_some());
    }

    #[test]
    fn cancel_operation_if_user_does_not_confirm() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();

        let result = purge(tmpdir.path(), 0, None, None, confirm_purge(false));
        assert!(result.is_ok());
    }

    #[test]
    fn guard_is_case_unsensitive() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();

        let set_elem = || {
            let db = open_app_dir(tmpdir.path(), false).unwrap();
            db.put_raw(42, 1, vec![1, 2, 3].as_slice()).unwrap();
        };
        // lowercase 'y'
        {
            set_elem();
            purge(tmpdir.path(), 42, None, None, Cursor::new("y")).expect("purge should succeed");
            assert!(
                open_app_dir(tmpdir.path(), true)
                    .unwrap()
                    .get(42, 1)
                    .unwrap()
                    .is_none()
            );
        }
        // uppercase 'Y'
        {
            set_elem();
            purge(tmpdir.path(), 42, None, None, Cursor::new("Y")).expect("purge should succeed");
            assert!(
                open_app_dir(tmpdir.path(), true)
                    .unwrap()
                    .get(42, 1)
                    .unwrap()
                    .is_none()
            );
        }
    }
}
