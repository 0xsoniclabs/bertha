// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

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
