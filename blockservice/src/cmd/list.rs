use std::path::Path;

use crate::{app_dir::open_app_dir, db::BlockDb};

pub fn list(
    chain_id: Option<u64>,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let app_dir = Path::new("./").canonicalize()?;
    let db = open_app_dir(app_dir, true)?;

    let chain_ids = match chain_id {
        Some(chain_id) => vec![chain_id],
        None => db.get_chain_ids()?,
    };

    for chain_id in chain_ids {
        let ranges = db.get_ranges_of_chain_id(chain_id)?;
        if ranges.is_empty() {
            writeln!(writer, "[chain ID {chain_id}] no blocks")?;
        }
        for (start, end) in ranges {
            writeln!(writer, "[chain ID {chain_id}] {start} - {end}")?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::{
        app_dir::BLOCK_DB_NAME,
        cmd::{ChangeWorkingDir, init},
        db::RocksBlockDb,
    };

    #[test]
    fn fails_if_app_dir_is_not_initialized() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());

        let result = list(None, std::io::sink());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no database found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }

    #[test]
    fn fails_if_no_read_permissions() {
        let tmpdir = tempfile::tempdir().unwrap();

        // create database
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        // remove read permissions
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o333)).unwrap();

        let result = list(None, std::io::sink());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to open"));
    }

    #[test]
    fn prints_message_for_each_range() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        // no blocks for chain id
        let mut buf = Vec::new();
        let result = list(Some(1), &mut buf);
        assert!(result.is_ok());
        assert_eq!(String::from_utf8(buf).unwrap(), "[chain ID 1] no blocks\n");

        // block ranges for chain id
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        let db = RocksBlockDb::open(db_path.clone()).unwrap();
        db.put_ranges_of_chain_id(1, &[(2, 4), (6, 8)]).unwrap();
        drop(db);

        let mut buf = Vec::new();
        let result = list(Some(1), &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[chain ID 1] 2 - 4\n[chain ID 1] 6 - 8\n"
        );

        // block ranges for multiple chain ids
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        let db = RocksBlockDb::open(db_path.clone()).unwrap();
        db.put_ranges_of_chain_id(3, &[(3, 5)]).unwrap();
        db.put_chain_ids(&[1, 3]).unwrap();
        drop(db);

        let mut buf = Vec::new();
        let result = list(None, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[chain ID 1] 2 - 4\n[chain ID 1] 6 - 8\n[chain ID 3] 3 - 5\n"
        );
    }
}
