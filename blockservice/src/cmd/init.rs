use std::path::Path;

use crate::app_dir::init_app_dir;

pub fn init(
    app_dir: impl AsRef<Path>,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    init_app_dir(app_dir, &mut writer)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app_dir::open_app_dir,
        utils::test_dir::{Permissions, TestDir},
    };

    #[test]
    fn initializes_app_dir() {
        // No args: Init in current working directory
        {
            let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
            init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
            assert!(open_app_dir(tmpdir.path(), false).is_ok());
        }

        // Optional arg: Init in specified directory
        {
            let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
            init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
            assert!(open_app_dir(tmpdir.path(), false).is_ok());
        }
    }
}
