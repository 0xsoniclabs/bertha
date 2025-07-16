use std::path::Path;

use crate::app_dir::init_app_dir;

pub fn init(path: Option<impl AsRef<Path>>) -> Result<(), Box<dyn std::error::Error>> {
    let path = path
        .map(|p| p.as_ref().to_path_buf())
        .unwrap_or_else(|| Path::new("./").to_path_buf());
    let path = path.canonicalize()?;

    init_app_dir(&path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app_dir::open_app_dir, cmd::ChangeWorkingDir};

    #[test]
    fn initializes_app_dir() {
        // No args: Init in current working directory
        {
            let tmpdir = tempfile::tempdir().unwrap();
            let _cwd = ChangeWorkingDir::new(tmpdir.path());
            init(None::<&Path>).unwrap();
            assert!(open_app_dir(tmpdir.path(), false).is_ok());
        }

        // Optional arg: Init in specified directory
        {
            let tmpdir = tempfile::tempdir().unwrap();
            init(Some(tmpdir.path().to_str().unwrap())).unwrap();
            assert!(open_app_dir(tmpdir.path(), false).is_ok());
        }
    }
}
