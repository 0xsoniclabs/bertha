use std::path::Path;

use crate::app_dir::init_app_dir;

pub fn init(app_dir: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
    init_app_dir(app_dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_dir::open_app_dir;

    #[test]
    fn initializes_app_dir() {
        // No args: Init in current working directory
        {
            let tmpdir = tempfile::tempdir().unwrap();
            init_app_dir(tmpdir.path()).unwrap();
            assert!(open_app_dir(tmpdir.path(), false).is_ok());
        }

        // Optional arg: Init in specified directory
        {
            let tmpdir = tempfile::tempdir().unwrap();
            init_app_dir(tmpdir.path()).unwrap();
            assert!(open_app_dir(tmpdir.path(), false).is_ok());
        }
    }
}
