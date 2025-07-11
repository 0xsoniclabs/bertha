use std::path::{Path, PathBuf};

use crate::{Error, db::RocksBlockDb};

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
/// A workspace currently only contains the block database.
///
/// If both the workspace already exists, an error is returned.
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

    // Create block database
    let db_path = path.join(BLOCK_DB_NAME);
    println!("Creating new block database at: {}", db_path.display());
    RocksBlockDb::create(db_path)?;

    Ok(())
}

/// Opens a blockservice workspace at the given path, returning its database.
///
/// The block database can be opened in read-only mode if `readonly_db` is set to `true`.
///
/// Returns an error if the the database does not exist.
pub fn open_workspace(path: impl AsRef<Path>, readonly_db: bool) -> Result<RocksBlockDb, Error> {
    let path = path.as_ref().to_path_buf().canonicalize()?;

    let db_path = path.join(BLOCK_DB_NAME);
    if !db_path.exists() {
        return Err(Error::Workspace(WorkspaceError::NotFound("database", path)));
    }

    let db = if readonly_db {
        RocksBlockDb::open_for_reading(db_path)?
    } else {
        RocksBlockDb::open(db_path)?
    };

    Ok(db)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::db::BlockDb;

    #[test]
    fn create_workspace_creates_db() {
        let tmpdir = tempfile::tempdir().unwrap();
        create_workspace(tmpdir.path()).unwrap();
        assert!(tmpdir.path().join(BLOCK_DB_NAME).exists());
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
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
    }

    #[test]
    fn open_workspaces_returns_db() {
        let tmpdir = tempfile::tempdir().unwrap();
        {
            // Manually create workspace
            let mut db = RocksBlockDb::create(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();
            db.put_raw(123, 456, vec![1, 2, 3].as_slice()).unwrap();
        }

        let db = open_workspace(tmpdir.path(), false).unwrap();
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
    fn open_workspace_fails_db_does_not_exist() {
        let tmpdir = tempfile::tempdir().unwrap();

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
