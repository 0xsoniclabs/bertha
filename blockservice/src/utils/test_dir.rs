// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

use std::{
    fs::{self},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

/// An enum representing different UNIX file permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permissions {
    ReadWrite,
    ReadOnly,
    WriteOnly,
}

impl From<Permissions> for std::fs::Permissions {
    fn from(permission: Permissions) -> Self {
        std::fs::Permissions::from_mode(permission.mode())
    }
}

impl Permissions {
    /// Returns the UNIX mode for the given permissions.
    /// Note that the execute bit is always set.
    pub fn mode(self) -> u32 {
        match self {
            Permissions::ReadWrite => 0o777,
            Permissions::ReadOnly => 0o555,
            Permissions::WriteOnly => 0o333,
        }
    }
}

/// A utility struct for creating and managing temporary test directories with specific permissions.
/// On drop, it will delete the directory and its contents.
pub struct TestDir {
    dir: PathBuf,
}

impl TestDir {
    /// Creates a new temporary `TestDir` with the specified permissions.
    pub fn try_new(permission: Permissions) -> std::io::Result<Self> {
        let dir = tempfile::tempdir()?;
        set_permissions(dir.path(), permission)?;
        Ok(Self { dir: dir.keep() })
    }

    /// Returns the path of the test directory.
    pub fn path(&self) -> &std::path::Path {
        &self.dir
    }

    /// Recursively sets the permissions of the test directory and its contents.
    pub fn set_permissions(&self, permission: Permissions) -> std::io::Result<()> {
        set_permissions(&self.dir, permission)
    }
}

impl Drop for TestDir {
    /// Deletes the test directory and its contents
    fn drop(&mut self) {
        set_permissions(&self.dir, Permissions::ReadWrite).unwrap_or_else(|e| {
            eprintln!(
                "Failed to set permissions for test directory {}: {}",
                self.dir.display(),
                e
            );
        });
        fs::remove_dir_all(&self.dir).unwrap_or_else(|e| {
            eprintln!(
                "Failed to remove test directory {}: {}",
                self.dir.display(),
                e
            );
        });
    }
}

/// Recursively set permissions for a directory and its contents
pub fn set_permissions(dir: &Path, permission: Permissions) -> std::io::Result<()> {
    // First make root directory readable so we can list its contents.
    fs::set_permissions(dir, Permissions::ReadOnly.into())?;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            set_permissions(&entry.path(), permission)?;
        } else {
            fs::set_permissions(entry.path(), permission.into())?;
        }
    }
    // Set permissions for the root directory
    fs::set_permissions(dir, permission.into())?;
    Ok(())
}

#[cfg(test)]
mod tests {

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(Permissions::ReadWrite, 0o777)]
    #[case(Permissions::ReadOnly, 0o555)]
    #[case(Permissions::WriteOnly, 0o333)]
    fn permissions_mode_returns_correct_mode(#[case] permission: Permissions, #[case] mode: u32) {
        assert_eq!(permission.mode(), mode);
    }

    #[rstest]
    #[case(0o777, Permissions::ReadWrite)]
    #[case(0o555, Permissions::ReadOnly)]
    #[case(0o333, Permissions::WriteOnly)]
    fn std_fs_permissions_from_permissions_returns_correct_permissions(
        #[case] mode: u32,
        #[case] permission: Permissions,
    ) {
        assert_eq!(
            std::fs::Permissions::from_mode(mode),
            std::fs::Permissions::from(permission)
        );
    }

    #[test]
    fn set_permissions_sets_permissions_recursively() {
        let test_dir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let dir_path = test_dir.path();
        fs::create_dir(dir_path.join("subdir")).unwrap();

        set_permissions(dir_path, Permissions::WriteOnly).unwrap();

        let permissions = fs::metadata(dir_path).unwrap().permissions();
        assert_eq!(permissions.mode() & 0o777, Permissions::WriteOnly.mode());
        let subdir_permissions = fs::metadata(dir_path.join("subdir")).unwrap().permissions();
        assert_eq!(
            subdir_permissions.mode() & 0o777,
            Permissions::WriteOnly.mode()
        );
    }

    #[test]
    fn set_permissions_sets_permissions_of_write_only_directory() {
        let test_dir = TestDir::try_new(Permissions::WriteOnly).unwrap();
        let dir_path = test_dir.path();
        fs::create_dir(dir_path.join("subdir")).unwrap();

        set_permissions(dir_path, Permissions::ReadOnly).unwrap();

        let permissions = fs::metadata(dir_path).unwrap().permissions();
        assert_eq!(permissions.mode() & 0o777, Permissions::ReadOnly.mode());
        let subdir_permissions = fs::metadata(dir_path.join("subdir")).unwrap().permissions();
        assert_eq!(
            subdir_permissions.mode() & 0o777,
            Permissions::ReadOnly.mode()
        );
    }

    #[test]
    fn set_permissions_fails_for_non_existent_directory() {
        let non_existent_path = PathBuf::from("non_existent_dir");
        let result = set_permissions(&non_existent_path, Permissions::ReadWrite)
            .expect_err("set_permissions should fail for non-existent directory");
        assert!(result.kind() == std::io::ErrorKind::NotFound);
    }

    #[test]
    fn path_returns_path() {
        let test_dir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        assert!(test_dir.path().exists());
    }

    #[rstest]
    #[case(Permissions::ReadOnly)]
    #[case(Permissions::WriteOnly)]
    #[case(Permissions::ReadWrite)]
    fn try_new_sets_permissions(#[case] permission: Permissions) {
        let test_dir = TestDir::try_new(permission).unwrap();
        let permissions = fs::metadata(test_dir.path()).unwrap().permissions();
        assert_eq!(permissions.mode() & 0o777, permission.mode());
    }

    #[rstest]
    #[case(Permissions::ReadOnly)]
    #[case(Permissions::WriteOnly)]
    #[case(Permissions::ReadWrite)]
    fn drop_deletes_directory_and_subdirectories(#[case] permission: Permissions) {
        let init_dir = |permission: Permissions| {
            // Create it as read write to be able to create subdirectories
            let test_dir = TestDir::try_new(Permissions::ReadWrite).unwrap();
            assert!(test_dir.path().exists());
            fs::create_dir(test_dir.path().join("subdir")).unwrap();
            // Apply the specified permissions
            set_permissions(test_dir.path(), permission).unwrap();
            test_dir
        };

        let test_dir = init_dir(permission);
        let path = test_dir.path().to_path_buf();
        drop(test_dir);
        assert!(!path.exists());
    }
}
