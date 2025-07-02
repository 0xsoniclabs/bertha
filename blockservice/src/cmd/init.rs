use std::path::Path;

use crate::blockdb::{BLOCK_DB_NAME, RocksBlockDb};

pub fn init(path: Option<impl AsRef<Path>>) -> Result<(), Box<dyn std::error::Error>> {
    let path = path
        .map(|p| p.as_ref().to_path_buf())
        .unwrap_or_else(|| Path::new("./").to_path_buf());
    let path = path.canonicalize()?;
    let path = path.join(BLOCK_DB_NAME);
    println!("Initializing new block database at: {}", path.display());
    RocksBlockDb::create(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::cmd::ChangeWorkingDir;

    #[test]
    fn creates_db() {
        // No args: Create in current working directory
        {
            let tmpdir = tempfile::tempdir().unwrap();
            let _cwd = ChangeWorkingDir::new(tmpdir.path());
            init(None::<&Path>).unwrap();
            assert!(tmpdir.path().join(BLOCK_DB_NAME).exists());
        }

        // Optional arg: Create in specified directory
        {
            let tmpdir = tempfile::tempdir().unwrap();
            init(Some(tmpdir.path().to_str().unwrap())).unwrap();
            assert!(Path::new(&format!("{}/.blockdb", tmpdir.path().display())).exists());
        }
    }

    #[test]
    fn fails_if_db_already_exists() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        let result = init(None::<&Path>);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("error_if_exists is true")
        );
    }

    #[test]
    fn fails_if_no_write_permissions() {
        let tmpdir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(tmpdir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();

        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        let result = init(None::<&Path>);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
    }
}
