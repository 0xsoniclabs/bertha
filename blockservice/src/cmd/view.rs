use std::path::Path;

use crate::{app_dir::open_app_dir, db::BlockDb};

pub fn view(
    app_dir: impl AsRef<Path>,
    chain_id: u64,
    block_number: u64,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (_cfg, db) = open_app_dir(app_dir, true)?;

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
    use std::os::unix::fs::PermissionsExt;

    use bertha_types::Block;

    use super::*;
    use crate::app_dir::{BLOCK_DB_NAME, init_app_dir};

    #[test]
    fn fails_if_app_dir_is_not_initialized() {
        let tmpdir = tempfile::tempdir().unwrap();

        let result = view(tmpdir.path(), 1, 0, std::io::sink());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }

    #[test]
    fn fails_if_no_read_permissions() {
        let tmpdir = tempfile::tempdir().unwrap();

        // create database
        init_app_dir(tmpdir.path()).unwrap();

        // remove read permissions
        std::fs::set_permissions(tmpdir.path(), std::fs::Permissions::from_mode(0o333)).unwrap();
        std::fs::set_permissions(
            tmpdir.path().join(BLOCK_DB_NAME),
            std::fs::Permissions::from_mode(0o333),
        )
        .unwrap();

        let result = view(tmpdir.path(), 0, 1, std::io::sink());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to open"));
    }

    #[test]
    fn prints_block_if_exists() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();

        let chain_id = 1;
        let block = Block::default();
        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
        db.put(chain_id, block.clone()).unwrap();

        let mut buf = Vec::new();
        let result = view(tmpdir.path(), chain_id, block.number, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            serde_json::to_string_pretty(&block).unwrap() + "\n"
        );
    }

    #[test]
    fn prints_error_message_if_not_exists() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();

        let mut buf = Vec::new();
        let result = view(tmpdir.path(), 1, 0, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[chain ID 1] block 0 not found\n"
        );
    }
}
