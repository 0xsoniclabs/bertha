use std::path::{Path, PathBuf};

use crate::{Error, config::Config, db::RocksBlockDb};

const CONFIG_FILE_NAME: &str = "blockservice.toml";
pub const BLOCK_DB_NAME: &str = ".blockdb";

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum AppDirError {
    #[error("no {0} found at {1} - did you forget to run init?")]
    NotFound(&'static str, PathBuf),
    #[error("failed to initialize application directory at {0}: {1}")]
    CreateFailed(PathBuf, String),
}

/// Initializes a new blockservice application directory at the given path.
///
/// The application directory consists of a configuration file and a block database.
///
/// If either a configuration file or block database already exists, they will not be overwritten.
/// This allows to at least partially recover from a corrupted application directory.
///
/// If both already exist, an error is returned.
pub fn init_app_dir(path: impl AsRef<Path>, mut writer: impl std::io::Write) -> Result<(), Error> {
    let path = path.as_ref().to_path_buf().canonicalize().map_err(|e| {
        Error::AppDir(AppDirError::CreateFailed(
            path.as_ref().to_path_buf(),
            e.to_string(),
        ))
    })?;

    // Check if application directory already exists
    if open_app_dir(&path, true).is_ok() {
        return Err(Error::AppDir(AppDirError::CreateFailed(
            path,
            "already exists".to_owned(),
        )));
    }

    writeln!(
        writer,
        "Initializing new blockservice directory at: {}",
        path.display()
    )?;

    // Create config file if it does not exist
    let cfg_path = path.join(CONFIG_FILE_NAME);
    if !cfg_path.exists() {
        writeln!(
            writer,
            "Creating new configuration at: {}",
            cfg_path.display()
        )?;
        Config::create_default(cfg_path.clone())
            .map_err(|e| AppDirError::CreateFailed(path.clone(), e.to_string()))?;
    } else {
        writeln!(
            writer,
            "Found existing configuration at: {}",
            cfg_path.display()
        )?;
    }

    // Create block database if it does not exist
    let db_path = path.join(BLOCK_DB_NAME);
    if !db_path.exists() {
        writeln!(
            writer,
            "Creating new block database at: {}",
            db_path.display()
        )?;
        RocksBlockDb::create(db_path)?;
    } else {
        writeln!(
            writer,
            "Found existing block database at: {}",
            db_path.display()
        )?;
    }

    Ok(())
}

/// Opens a blockservice application directory at the given path, returning its config and database.
///
/// The block database can be opened in read-only mode if `readonly_db` is set to `true`.
///
/// Returns an error if either the configuration file or the block database does not exist.
pub fn open_app_dir(
    path: impl AsRef<Path>,
    readonly_db: bool,
) -> Result<(Config, RocksBlockDb), Error> {
    let path = path.as_ref().to_path_buf().canonicalize()?;

    let cfg_path = path.join(CONFIG_FILE_NAME);
    if !cfg_path.exists() {
        return Err(Error::AppDir(AppDirError::NotFound(CONFIG_FILE_NAME, path)));
    }

    let db_path = path.join(BLOCK_DB_NAME);
    if !db_path.exists() {
        return Err(Error::AppDir(AppDirError::NotFound("database", path)));
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

    use super::*;
    use crate::{
        config::ChainConfig,
        db::BlockDb,
        utils::test_dir::{Permissions, TestDir},
    };

    #[test]
    fn init_app_dir_creates_db_and_config_file() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let mut writer = Vec::new();
        init_app_dir(tmpdir.path(), &mut writer).unwrap();
        assert!(tmpdir.path().join(BLOCK_DB_NAME).exists());
        assert!(tmpdir.path().join(CONFIG_FILE_NAME).exists());

        assert_eq!(
            String::from_utf8(writer).unwrap(),
            format!(
                "Initializing new blockservice directory at: {}\nCreating new configuration at: {}\nCreating new block database at: {}\n",
                tmpdir.path().display(),
                tmpdir.path().join(CONFIG_FILE_NAME).display(),
                tmpdir.path().join(BLOCK_DB_NAME).display()
            )
        );
    }

    #[test]
    fn init_app_dir_fails_if_directory_does_not_exist() {
        let path = PathBuf::from("/non/existent/path");
        let mut writer = Vec::new();
        let result = init_app_dir(&path, &mut writer);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Error::AppDir(AppDirError::CreateFailed(
                path,
                "No such file or directory (os error 2)".to_owned()
            ))
        );
    }

    #[test]
    fn init_app_dir_fails_if_already_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let mut writer = Vec::new();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let result = init_app_dir(tmpdir.path(), &mut writer);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Error::AppDir(AppDirError::CreateFailed(
                tmpdir.path().to_path_buf(),
                "already exists".to_owned()
            ))
        );
        assert!(writer.is_empty());
    }

    #[test]
    fn init_app_dir_fails_if_no_write_permissions() {
        let tmpdir = TestDir::try_new(Permissions::ReadOnly).unwrap();

        let mut writer = Vec::new();
        let result = init_app_dir(tmpdir.path(), &mut writer);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Error::AppDir(AppDirError::CreateFailed(
                tmpdir.path().to_path_buf(),
                "I/O error: Permission denied (os error 13)".to_owned()
            ))
        );

        assert_eq!(
            String::from_utf8(writer).unwrap(),
            format!(
                "Initializing new blockservice directory at: {}\nCreating new configuration at: {}\n",
                tmpdir.path().display(),
                tmpdir.path().join(CONFIG_FILE_NAME).display()
            )
        );
    }

    #[test]
    fn init_app_dir_creates_config_file_if_db_exists_but_config_does_not() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        {
            let db = RocksBlockDb::create(&db_path).unwrap();
            db.put_raw(123, 456, vec![1, 2, 3].as_slice()).unwrap();
        }
        let mut writer = Vec::new();
        init_app_dir(tmpdir.path(), &mut writer).unwrap();
        assert!(tmpdir.path().join(CONFIG_FILE_NAME).exists());

        assert_eq!(
            String::from_utf8(writer).unwrap(),
            format!(
                "Initializing new blockservice directory at: {}\nCreating new configuration at: {}\nFound existing block database at: {}\n",
                tmpdir.path().display(),
                tmpdir.path().join(CONFIG_FILE_NAME).display(),
                db_path.display()
            )
        );

        // DB was not overwritten
        let db = RocksBlockDb::open(db_path).unwrap();
        let res = db.get_raw(123, 456).unwrap();
        assert_eq!(res, Some(vec![1, 2, 3]));
    }

    #[test]
    fn init_app_dir_creates_db_if_config_exists_but_db_does_not() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let cfg_path = tmpdir.path().join(CONFIG_FILE_NAME);
        {
            let mut cfg = Config::create_default(cfg_path.clone()).unwrap();
            cfg.add_chain(ChainConfig {
                name: "Test Chain".to_owned(),
                ..ChainConfig::new(123)
            })
            .unwrap();
        }
        let mut writer = Vec::new();
        init_app_dir(tmpdir.path(), &mut writer).unwrap();
        assert!(tmpdir.path().join(BLOCK_DB_NAME).exists());
        assert_eq!(
            String::from_utf8(writer).unwrap(),
            format!(
                "Initializing new blockservice directory at: {}\nFound existing configuration at: {}\nCreating new block database at: {}\n",
                tmpdir.path().display(),
                cfg_path.display(),
                tmpdir.path().join(BLOCK_DB_NAME).display()
            )
        );
        // Config was not overwritten
        let config = Config::load(cfg_path).unwrap();
        assert_eq!(config.get_chain_config(123).unwrap().name, "Test Chain");
    }

    #[test]
    fn open_app_dir_returns_config_and_db() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        {
            // Manually initialize app dir
            let mut cfg = Config::create_default(tmpdir.path().join(CONFIG_FILE_NAME)).unwrap();
            cfg.add_chain(ChainConfig {
                name: "Test Chain".to_owned(),
                ..ChainConfig::new(123)
            })
            .unwrap();
            let db = RocksBlockDb::create(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
            db.put_raw(123, 456, vec![1, 2, 3].as_slice()).unwrap();
        }

        let (cfg, db) = open_app_dir(tmpdir.path(), false).unwrap();
        assert_eq!(cfg.get_chain_config(123).unwrap().name, "Test Chain");
        let res = db.get_raw(123, 456).unwrap();
        assert_eq!(res, Some(vec![1, 2, 3]));
    }

    #[test]
    fn open_app_dir_fails_if_directory_does_not_exist() {
        let path = PathBuf::from("/non/existent/path");

        let result = open_app_dir(&path, false);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Error::Io("No such file or directory (os error 2)".to_owned())
        );
    }

    #[test]
    fn open_app_dir_fails_if_config_does_not_exist() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        RocksBlockDb::create(db_path).unwrap();

        let result = open_app_dir(tmpdir.path(), false);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Error::AppDir(AppDirError::NotFound(
                CONFIG_FILE_NAME,
                tmpdir.path().to_path_buf()
            ))
        );
    }

    #[test]
    fn open_app_dir_fails_if_db_does_not_exist() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        Config::create_default(tmpdir.path().join(CONFIG_FILE_NAME)).unwrap();

        let result = open_app_dir(tmpdir.path(), false);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Error::AppDir(AppDirError::NotFound(
                "database",
                tmpdir.path().to_path_buf()
            ))
        );
    }
}
