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

/// Reads the contents of `file` and stores them as the upgrade heights for `chain_id` in the
/// block database located at `app_dir`.
pub fn import_upgrade_heights(
    app_dir: impl AsRef<Path>,
    chain_id: u64,
    file: impl AsRef<Path>,
    writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (_cfg, mut db) = open_app_dir(app_dir, false)?;
    import_upgrade_heights_internal(&mut db, chain_id, file, writer)
}

/// Reads the contents of `file` and stores them as the corrections for `chain_id` in the
/// block database located at `app_dir`.
pub fn import_corrections(
    app_dir: impl AsRef<Path>,
    chain_id: u64,
    file: impl AsRef<Path>,
    writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (_cfg, mut db) = open_app_dir(app_dir, false)?;
    import_corrections_internal(&mut db, chain_id, file, writer)
}

/// Reads the contents of `file` and stores them as the upgrade heights for `chain_id` in the
/// block database.
fn import_upgrade_heights_internal(
    db: &mut impl BlockDb,
    chain_id: u64,
    file: impl AsRef<Path>,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = std::fs::read(file)?;
    db.put_upgrade_heights(chain_id, &data)?;
    writeln!(
        writer,
        "Upgrade heights for chain ID {chain_id} imported successfully."
    )?;
    Ok(())
}

/// Reads the contents of `file` and stores them as the corrections for `chain_id` in the
/// block database.
fn import_corrections_internal(
    db: &mut impl BlockDb,
    chain_id: u64,
    file: impl AsRef<Path>,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = std::fs::read(file)?;
    db.put_corrections(chain_id, &data)?;
    writeln!(
        writer,
        "Corrections for chain ID {chain_id} imported successfully."
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use mockall::predicate::eq;

    use super::*;
    use crate::{
        app_dir::init_app_dir,
        db::MockBlockDb,
        utils::test_dir::{Permissions, TestDir},
    };

    #[test]
    fn import_upgrade_heights_fails_if_app_dir_is_not_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        let result = import_upgrade_heights(tmpdir.path(), 1, "/dev/null", std::io::sink());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }

    #[test]
    fn import_upgrade_heights_internal_stores_file_contents() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let data = b"upgrade-heights";
        let file = tmpdir.path().join("upgrade_heights.json");
        std::fs::write(&file, data).unwrap();

        let mut db = MockBlockDb::new();
        db.expect_put_upgrade_heights()
            .with(eq(1u64), eq(data.to_vec()))
            .return_once(|_, _| Ok(()));

        let mut buf = Vec::new();
        import_upgrade_heights_internal(&mut db, 1, &file, &mut buf).unwrap();
        assert_eq!(
            buf,
            b"Upgrade heights for chain ID 1 imported successfully.\n"
        );
    }

    #[test]
    fn import_corrections_fails_if_app_dir_is_not_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        let result = import_corrections(tmpdir.path(), 1, "/dev/null", std::io::sink());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }

    #[test]
    fn import_corrections_internal_stores_file_contents() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let data = b"corrections";
        let file = tmpdir.path().join("corrections.json");
        std::fs::write(&file, data).unwrap();

        let mut db = MockBlockDb::new();
        db.expect_put_corrections()
            .with(eq(1u64), eq(data.to_vec()))
            .return_once(|_, _| Ok(()));

        let mut buf = Vec::new();
        import_corrections_internal(&mut db, 1, &file, &mut buf).unwrap();
        assert_eq!(buf, b"Corrections for chain ID 1 imported successfully.\n");
    }
}
