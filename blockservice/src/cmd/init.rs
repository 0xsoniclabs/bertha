use std::path::Path;

use crate::workspace::create_workspace;

pub fn init(path: Option<impl AsRef<Path>>) -> Result<(), Box<dyn std::error::Error>> {
    let path = path
        .map(|p| p.as_ref().to_path_buf())
        .unwrap_or_else(|| Path::new("./").to_path_buf());
    let path = path.canonicalize()?;

    create_workspace(&path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cmd::ChangeWorkingDir, workspace::open_workspace};

    #[test]
    fn initializes_workspace() {
        // No args: Init in current working directory
        {
            let tmpdir = tempfile::tempdir().unwrap();
            let _cwd = ChangeWorkingDir::new(tmpdir.path());
            init(None::<&Path>).unwrap();
            assert!(open_workspace(tmpdir.path(), false).is_ok());
        }

        // Optional arg: Init in specified directory
        {
            let tmpdir = tempfile::tempdir().unwrap();
            init(Some(tmpdir.path().to_str().unwrap())).unwrap();
            assert!(open_workspace(tmpdir.path(), false).is_ok());
        }
    }
}
