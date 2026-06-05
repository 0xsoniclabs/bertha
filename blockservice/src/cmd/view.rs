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

/// Writes the pretty-printed JSON representation of the block with the specified `block_number` for
/// `chain_id` stored in the block database to `writer`.
pub fn view(
    app_dir: impl AsRef<Path>,
    chain_id: u64,
    block_number: u64,
    writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (_cfg, db) = open_app_dir(app_dir, true)?;
    view_internal(&db, chain_id, block_number, writer)
}

/// Writes the pretty-printed JSON representation of the block with the specified `block_number` for
/// `chain_id` stored in the block database to `writer`.
fn view_internal(
    db: &impl BlockDb,
    chain_id: u64,
    block_number: u64,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let block = db.get(chain_id, block_number)?;
    match block {
        Some(block) => {
            writeln!(writer, "{}", serde_json::to_string_pretty(&block)?)?;
        }
        None => writeln!(
            writer,
            "[chain ID {chain_id}] block {block_number} not found",
        )?,
    }

    Ok(())
}

#[cfg(test)]
mod tests {

    use bertha_types::Block;
    use mockall::predicate::eq;

    use super::*;
    use crate::{
        app_dir::init_app_dir,
        db::MockBlockDb,
        utils::test_dir::{Permissions, TestDir},
    };

    #[test]
    fn view_fails_if_app_dir_is_not_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        let result = view(tmpdir.path(), 1, 0, std::io::sink());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }

    #[test]
    fn view_fails_if_no_read_permissions() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        // create database
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        // remove read permissions
        tmpdir.set_permissions(Permissions::WriteOnly).unwrap();

        let result = view(tmpdir.path(), 0, 1, std::io::sink());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
    }

    #[test]
    fn view_internal_writes_block_if_exists() {
        let chain_id = 1;
        let block = Block::default();

        let mut db = MockBlockDb::new();
        db.expect_get()
            .with(eq(chain_id), eq(block.number))
            .return_once({
                let block = block.clone();
                move |_, _| Ok(Some(block))
            });

        let mut buf = Vec::new();
        let result = view_internal(&db, chain_id, block.number, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            serde_json::to_string_pretty(&block).unwrap() + "\n"
        );
    }

    #[test]
    fn view_internal_writes_error_message_if_not_exists() {
        let chain_id = 1;
        let block_number = 0;

        let mut db = MockBlockDb::new();
        db.expect_get()
            .with(eq(chain_id), eq(block_number))
            .return_once(|_, _| Ok(None));

        let mut buf = Vec::new();
        let result = view_internal(&db, chain_id, block_number, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[chain ID 1] block 0 not found\n"
        );
    }
}
