use std::path::Path;

use crate::blockdb::{BLOCK_DB_NAME, BlockDb, RocksBlockDb};

pub fn view(
    chain_id: u64,
    block_number: u64,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = Path::new("./").join(BLOCK_DB_NAME).canonicalize()?;
    let db = RocksBlockDb::open_for_reading(db_path)?;

    let block = db.get(chain_id, block_number)?;
    match block {
        Some(block) => {
            writeln!(writer, "{}", serde_json::to_string_pretty(&block)?)?;
        }
        None => writeln!(
            writer,
            "[chain ID {chain_id}] block {block_number} not found",
        )?,
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use bertha_types::Block;

    use super::*;
    use crate::cmd::{ChangeWorkingDir, init};

    #[test]
    fn fails_if_no_read_permissions() {
        let tmpdir = tempfile::tempdir().unwrap();

        // create database
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        // remove read permissions
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o333)).unwrap();

        let result = view(0, 1, std::io::sink());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to open"));
    }

    #[test]
    fn fails_if_db_does_not_exist() {
        let tmpdir = tempfile::tempdir().unwrap();

        let _cwd = ChangeWorkingDir::new(tmpdir.path());

        let result = view(1, 0, std::io::sink());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No such file or directory")
        );
    }

    #[test]
    fn prints_block_if_exists() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        let chain_id = 1;
        let block = Block::default();
        let mut db = RocksBlockDb::open(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
        db.put(chain_id, block.clone()).unwrap();

        let mut buf = Vec::new();
        let result = view(chain_id, block.number, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            serde_json::to_string_pretty(&block).unwrap() + "\n"
        );
    }

    #[test]
    fn prints_error_message_if_not_exists() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        let mut buf = Vec::new();
        let result = view(1, 0, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[chain ID 1] block 0 not found\n"
        );
    }
}
