use std::path::{Path, PathBuf};

use crate::{Error, config::Config, db::RocksBlockDb};

const CONFIG_FILE_NAME: &str = "blockservice.toml";
pub const BLOCK_DB_NAME: &str = ".blockdb";

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum WorkspaceError {
    #[error("no {0} found at {1} - did you forget to run init?")]
    NotFound(&'static str, PathBuf),
    #[error("failed to create workspace at {0}: {1}")]
    CreateFailed(PathBuf, String),
}

/// Creates a new blockservice workspace at the given path.
///
/// A workspace consists of a configuration file and a block database.
///
/// If either a configuration file or block database already exists, they will not be overwritten.
/// This allows to at least partially recover from a corrupted workspace.
///
/// If both already exist, an error is returned.
pub fn create_workspace(path: impl AsRef<Path>) -> Result<(), Error> {
    let path = path.as_ref().to_path_buf().canonicalize().map_err(|e| {
        Error::Workspace(WorkspaceError::CreateFailed(
            path.as_ref().to_path_buf(),
            e.to_string(),
        ))
    })?;

    // Check if workspace already exists
    if open_workspace(&path, true).is_ok() {
        return Err(Error::Workspace(WorkspaceError::CreateFailed(
            path,
            "already exists".to_owned(),
        )));
    }

    println!(
        "Initializing new blockservice workspace at: {}",
        path.display()
    );

    // Create config file if it does not exist
    let cfg_path = path.join(CONFIG_FILE_NAME);
    if !cfg_path.exists() {
        println!("Creating new configuration at: {}", cfg_path.display());
        Config::create_default(cfg_path.clone())
            .map_err(|e| WorkspaceError::CreateFailed(path.clone(), e.to_string()))?;
    } else {
        println!("Found existing configuration at: {}", cfg_path.display());
    }

    // Create block database if it does not exist
    let db_path = path.join(BLOCK_DB_NAME);
    if !db_path.exists() {
        println!("Creating new block database at: {}", db_path.display());
        RocksBlockDb::create(db_path)?;
    } else {
        println!("Found existing block database at: {}", db_path.display());
    }

    Ok(())
}

/// Opens a blockservice workspace at the given path, returning its config and database.
///
/// The block database can be opened in read-only mode if `readonly_db` is set to `true`.
///
/// Returns an error if either the configuration file or the block does not exist.
pub fn open_workspace(
    path: impl AsRef<Path>,
    readonly_db: bool,
) -> Result<(Config, RocksBlockDb), Error> {
    let path = path.as_ref().to_path_buf().canonicalize()?;

    let cfg_path = path.join(CONFIG_FILE_NAME);
    if !cfg_path.exists() {
        return Err(Error::Workspace(WorkspaceError::NotFound(
            CONFIG_FILE_NAME,
            path,
        )));
    }

    let db_path = path.join(BLOCK_DB_NAME);
    if !db_path.exists() {
        return Err(Error::Workspace(WorkspaceError::NotFound("database", path)));
    }

    let cfg = Config::load(cfg_path)?;

    let db = if readonly_db {
        RocksBlockDb::open_for_reading(db_path)?
    } else {
        RocksBlockDb::open(db_path)?
    };

    Ok((cfg, db))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::{config::ChainConfig, db::BlockDb};

    #[test]
    fn create_workspace_creates_db_and_config_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        create_workspace(tmpdir.path()).unwrap();
        assert!(tmpdir.path().join(BLOCK_DB_NAME).exists());
        assert!(tmpdir.path().join(CONFIG_FILE_NAME).exists());
    }

    #[test]
    fn create_workspace_fails_if_directory_does_not_exist() {
        let path = PathBuf::from("/non/existent/path");
        let result = create_workspace(&path);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Error::Workspace(WorkspaceError::CreateFailed(
                path,
                "No such file or directory (os error 2)".to_owned()
            ))
        );
    }

    #[test]
    fn create_workspace_fails_if_already_exists() {
        let tmpdir = tempfile::tempdir().unwrap();
        create_workspace(tmpdir.path()).unwrap();

        let result = create_workspace(tmpdir.path());
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Error::Workspace(WorkspaceError::CreateFailed(
                tmpdir.path().to_path_buf(),
                "already exists".to_owned()
            ))
        );
    }

    #[test]
    fn create_workspace_fails_if_no_write_permissions() {
        let tmpdir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(tmpdir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = create_workspace(tmpdir.path());
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Error::Workspace(WorkspaceError::CreateFailed(
                tmpdir.path().to_path_buf(),
                "I/O error: Permission denied (os error 13)".to_owned()
            ))
        );
    }

    #[test]
    fn create_workspace_creates_config_file_if_db_exists_but_config_does_not() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        {
            let mut db = RocksBlockDb::create(&db_path).unwrap();
            db.put_raw(123, 456, vec![1, 2, 3].as_slice()).unwrap();
        }
        create_workspace(tmpdir.path()).unwrap();
        assert!(tmpdir.path().join(CONFIG_FILE_NAME).exists());
        // DB was not overwritten
        let db = RocksBlockDb::open(db_path).unwrap();
        let res = db.get_raw(123, 456).unwrap();
        assert_eq!(res, Some(vec![1, 2, 3]));
    }

    #[test]
    fn create_workspace_creates_db_if_config_exists_but_db_does_not() {
        let tmpdir = tempfile::tempdir().unwrap();
        let cfg_path = tmpdir.path().join(CONFIG_FILE_NAME);
        {
            let mut cfg = Config::create_default(cfg_path.clone()).unwrap();
            cfg.add_chain(ChainConfig {
                id: 123,
                name: "Test Chain".to_owned(),
                description: "A test chain".to_owned(),
            })
            .unwrap();
        }
        create_workspace(tmpdir.path()).unwrap();
        assert!(tmpdir.path().join(BLOCK_DB_NAME).exists());
        // Config was not overwritten
        let config = Config::load(cfg_path).unwrap();
        assert_eq!(config.get_chain_config(123).name, "Test Chain");
    }

    #[test]
    fn open_workspaces_returns_config_and_db() {
        let tmpdir = tempfile::tempdir().unwrap();
        {
            // Manually create workspace
            let mut cfg = Config::create_default(tmpdir.path().join(CONFIG_FILE_NAME)).unwrap();
            cfg.add_chain(ChainConfig {
                id: 123,
                name: "Test Chain".to_owned(),
                description: "A test chain".to_owned(),
            })
            .unwrap();
            let mut db = RocksBlockDb::create(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
            db.put_raw(123, 456, vec![1, 2, 3].as_slice()).unwrap();
        }

        let (cfg, db) = open_workspace(tmpdir.path(), false).unwrap();
        assert_eq!(cfg.get_chain_config(123).name, "Test Chain");
        let res = db.get_raw(123, 456).unwrap();
        assert_eq!(res, Some(vec![1, 2, 3]));
    }

    #[test]
    fn open_workspace_fails_if_directory_does_not_exist() {
        let path = PathBuf::from("/non/existent/path");

        let result = open_workspace(&path, false);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Error::Io("No such file or directory (os error 2)".to_owned())
        );
    }

    #[test]
    fn open_workspace_fails_if_config_does_not_exist() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        RocksBlockDb::create(db_path).unwrap();

        let result = open_workspace(tmpdir.path(), false);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Error::Workspace(WorkspaceError::NotFound(
                CONFIG_FILE_NAME,
                tmpdir.path().to_path_buf()
            ))
        );
    }

    #[test]
    fn open_workspace_fails_db_does_not_exist() {
        let tmpdir = tempfile::tempdir().unwrap();
        Config::create_default(tmpdir.path().join(CONFIG_FILE_NAME)).unwrap();

        let result = open_workspace(tmpdir.path(), false);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Error::Workspace(WorkspaceError::NotFound(
                "database",
                tmpdir.path().to_path_buf()
            ))
        );
    }
}
