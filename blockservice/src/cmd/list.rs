use std::path::Path;

use crate::{db::BlockDb, workspace::open_workspace};

pub fn list(
    chain_id: Option<u64>,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace_path = Path::new("./").canonicalize()?;
    let (cfg, db) = open_workspace(workspace_path, true)?;

    let mut chain_ids = match chain_id {
        Some(chain_id) => vec![chain_id],
        None => [db.get_chain_ids()?, cfg.get_chain_ids()]
            .concat()
            .into_iter()
            .collect::<Vec<_>>(),
    };
    chain_ids.sort();
    chain_ids.dedup();

    for chain_id in chain_ids {
        let ranges = db.get_ranges_of_chain_id(chain_id)?;
        let chain_cfg = cfg.get_chain_config(chain_id);

        writeln!(writer, "{}", chain_cfg.pretty_name())?;

        if ranges.is_empty() {
            writeln!(writer, "\tno blocks")?;
        }
        for (start, end) in ranges {
            writeln!(writer, "\t{start} - {end}")?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::{
        cmd::{ChangeWorkingDir, init},
        config::ChainConfig,
        db::RocksBlockDb,
        workspace::{BLOCK_DB_NAME, create_workspace},
    };

    #[test]
    fn fails_if_workspace_does_not_exist() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());

        let result = list(None, std::io::sink());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
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
        create_workspace(tmpdir.path()).unwrap();

        let chain_cfg = ChainConfig {
            id: 1,
            name: "Test Chain".to_string(),
            description: "A test chain".to_string(),
        };
        let (mut cfg, _db) = open_workspace(tmpdir.path(), true).unwrap();
        drop(_db);
        cfg.add_chain(chain_cfg.clone()).unwrap();

        // no blocks for chain id
        let mut buf = Vec::new();
        let result = list(Some(1), &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!("{}\n\tno blocks\n", chain_cfg.pretty_name())
        );

        // block ranges for chain id
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        let mut db = RocksBlockDb::open(db_path.clone()).unwrap();
        db.put_ranges_of_chain_id(1, &[(2, 4), (6, 8)]).unwrap();
        drop(db);

        let mut buf = Vec::new();
        let result = list(Some(1), &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!("{}\n\t2 - 4\n\t6 - 8\n", chain_cfg.pretty_name())
        );

        // block ranges for multiple chain ids
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        let mut db = RocksBlockDb::open(db_path.clone()).unwrap();
        db.put_ranges_of_chain_id(3, &[(3, 5)]).unwrap();
        db.put_chain_ids(&[1, 3]).unwrap();
        drop(db);

        let mut buf = Vec::new();
        let result = list(None, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!(
                "{}\n\t2 - 4\n\t6 - 8\n{}\n\t3 - 5\n",
                chain_cfg.pretty_name(),
                cfg.get_chain_config(3).pretty_name()
            )
        );
    }

    // TODO: Includes chains from config file
    // => But does not print them twice if they also in db!
}
