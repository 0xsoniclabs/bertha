use std::path::Path;

use crate::blockdb::{BLOCK_DB_NAME, BlockDb, RocksBlockDb};

pub fn purge(
    chain_id: u64,
    from: Option<u64>,
    to: Option<u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = Path::new("./").join(BLOCK_DB_NAME).canonicalize()?;
    let mut db = RocksBlockDb::open(db_path)?;

    db.delete_range(chain_id, from, to)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use bertha_types::Block;

    use super::*;
    use crate::{
        blockdb::BLOCK_DB_NAME,
        cmd::{ChangeWorkingDir, init},
    };

    #[test]
    fn fails_if_no_write_permissions() {
        let tmpdir = tempfile::tempdir().unwrap();

        // create database
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        // remove write permissions
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = purge(0, None, None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
    }

    #[test]
    fn fails_if_db_does_not_exist() {
        let tmpdir = tempfile::tempdir().unwrap();

        let _cwd = ChangeWorkingDir::new(tmpdir.path());

        let result = purge(0, None, None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No such file or directory")
        );
    }

    #[test]
    fn can_be_called_with_chain_id_or_chain_id_and_start_or_chain_id_and_start_and_end() {
        let tmpdir = tempfile::tempdir().unwrap();

        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        assert!(purge(0, None, None).is_ok());
        assert!(purge(0, Some(0), None).is_ok());
        assert!(purge(0, Some(0), Some(1)).is_ok());
    }

    #[test]
    fn deletes_range_of_blocks() {
        // this test is just supposed to check that the purge command calls
        // BlockRocksDb::delete_range, not that all the corner cases work because they are
        // already tested in BlockRocksDb::delete_range

        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut db = RocksBlockDb::open(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();

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

        purge(chain_id, Some(1), Some(3)).unwrap();

        let db = RocksBlockDb::open(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
        assert!(db.get(chain_id, 0).unwrap().is_some());
        assert!(db.get(chain_id, 1).unwrap().is_none());
        assert!(db.get(chain_id, 2).unwrap().is_none());
        assert!(db.get(chain_id, 3).unwrap().is_some());
    }
}
