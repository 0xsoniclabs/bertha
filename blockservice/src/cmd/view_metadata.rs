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

use crate::{app_dir::open_app_dir, db::BlockDb};

/// Writes the upgrade heights stored in the block database for `chain_id` to `writer`.
pub fn view_upgrade_heights(
    app_dir: impl AsRef<Path>,
    chain_id: u64,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (_cfg, db) = open_app_dir(app_dir, true)?;

    match db.get_upgrade_heights(chain_id)? {
        Some(data) => writeln!(writer, "{}", String::from_utf8_lossy(&data))?,
        None => writeln!(writer, "[chain ID {chain_id}] no upgrade heights found")?,
    }

    Ok(())
}

/// Writes the corrections stored in the block database for `chain_id` to `writer`.
pub fn view_corrections(
    app_dir: impl AsRef<Path>,
    chain_id: u64,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (_cfg, db) = open_app_dir(app_dir, true)?;

    match db.get_corrections(chain_id)? {
        Some(data) => writeln!(writer, "{}", String::from_utf8_lossy(&data))?,
        None => writeln!(writer, "[chain ID {chain_id}] no corrections found")?,
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app_dir::init_app_dir,
        utils::test_dir::{Permissions, TestDir},
    };

    #[test]
    fn view_upgrade_heights_fails_if_app_dir_is_not_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        let result = view_upgrade_heights(tmpdir.path(), 1, std::io::sink());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }

    #[test]
    fn view_upgrade_heights_writes_not_found_if_absent() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let mut buf = Vec::new();
        view_upgrade_heights(tmpdir.path(), 1, &mut buf).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[chain ID 1] no upgrade heights found\n"
        );
    }

    #[test]
    fn view_upgrade_heights_writes_stored_data() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let data = b"upgrade-heights";
        let (_, db) = crate::app_dir::open_app_dir(tmpdir.path(), false).unwrap();
        db.put_upgrade_heights(1, data).unwrap();

        let mut buf = Vec::new();
        view_upgrade_heights(tmpdir.path(), 1, &mut buf).unwrap();
        let data_with_newline = [data.as_slice(), b"\n"].concat();
        assert_eq!(buf, data_with_newline);
    }

    #[test]
    fn view_corrections_fails_if_app_dir_is_not_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        let result = view_corrections(tmpdir.path(), 1, std::io::sink());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }

    #[test]
    fn view_corrections_writes_not_found_if_absent() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let mut buf = Vec::new();
        view_corrections(tmpdir.path(), 1, &mut buf).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[chain ID 1] no corrections found\n"
        );
    }

    #[test]
    fn view_corrections_writes_stored_data() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let data = b"corrections";
        let (_, db) = crate::app_dir::open_app_dir(tmpdir.path(), false).unwrap();
        db.put_corrections(1, data).unwrap();

        let mut buf = Vec::new();
        view_corrections(tmpdir.path(), 1, &mut buf).unwrap();
        let data_with_newline = [data.as_slice(), b"\n"].concat();
        assert_eq!(buf, data_with_newline);
    }
}
