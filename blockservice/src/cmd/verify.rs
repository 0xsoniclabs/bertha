use std::path::Path;

use bertha_types::{Hash, HexConvert};
use blockservice::blockdb::{BlockDb, RocksBlockDb};

use crate::BLOCK_DB_NAME;

pub fn verify(
    chain_id: u64,
    block_number: Option<u64>,
    block_hash: Option<Hash>,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = Path::new("./").join(BLOCK_DB_NAME).canonicalize()?;
    let db = RocksBlockDb::open_for_reading(db_path)?;

    let mut errors = 0;

    if let (Some(block_number), Some(expected_hash)) = (block_number, block_hash) {
        if let Some(block) = db.get(chain_id, block_number)? {
            let block_hash = block.to_header().compute_hash();
            if block_hash != expected_hash {
                errors += 1;
                writeln!(
                    writer,
                    "[chain ID {}] block hash verification failed for block {}: expected hash {}, got {}.",
                    chain_id,
                    block_number,
                    expected_hash.to_hex(),
                    block_hash.to_hex()
                )?;
            }
        } else {
            errors += 1;
            writeln!(
                writer,
                "[chain ID {chain_id}] requested block {block_number} does not exit"
            )?;
        }
    }

    // start with the first block if no block number is provided
    let block_number = block_number.unwrap_or_default();
    let mut prev_block_number = block_number;
    let mut prev_block_hash: Option<Hash> = None;
    for entry in db.iterate_with_block_number(chain_id, block_number) {
        let (block_number, block) = entry?;
        if block.number != block_number {
            errors += 1;
            writeln!(
                writer,
                "[chain ID {}] block number mismatch: block number in key = {}, block.number = {}.",
                chain_id, block_number, block.number
            )?;
        }
        if prev_block_number + 1 != block_number {
            prev_block_hash = None; // there was a gap so we have to skip the parent hash check
        }
        if let Some(prev_block_hash) = prev_block_hash {
            if block.parent_hash != prev_block_hash {
                errors += 1;
                writeln!(
                    writer,
                    "[chain ID {}] parent hash verification failed for block {}: expected hash {}, got {}.",
                    chain_id,
                    block_number,
                    prev_block_hash.to_hex(),
                    block.parent_hash.to_hex()
                )?;
            }
        }
        prev_block_number = block_number;
        prev_block_hash = Some(block.to_header().compute_hash());
    }

    if errors == 0 {
        writeln!(
            writer,
            "[chain ID {chain_id}] All blocks verified successfully."
        )?;
    } else {
        writeln!(
            writer,
            "[chain ID {chain_id}] Verification completed with {errors} errors."
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use bertha_types::Block;
    use blockservice::proto;
    use prost::Message;

    use super::*;
    use crate::{
        BLOCK_DB_NAME,
        cmd::{ChangeWorkingDir, init},
    };

    #[test]
    fn fails_if_no_read_permissions() {
        let tmpdir = tempfile::tempdir().unwrap();

        // create database
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        // remove read permissions
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o333)).unwrap();

        let result = verify(0, None, None, std::io::sink());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to open"));
    }

    #[test]
    fn fails_if_db_does_not_exist() {
        let tmpdir = tempfile::tempdir().unwrap();

        let _cwd = ChangeWorkingDir::new(tmpdir.path());

        let result = verify(0, None, None, std::io::sink());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No such file or directory")
        );
    }

    #[test]
    fn checks_hash_of_block() {
        let chain_id = 146;

        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut db = RocksBlockDb::open(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();

        let block = Block::default();
        db.put(chain_id, block.clone()).unwrap();

        // correct hash
        let mut buf = Vec::new();
        let result = verify(
            chain_id,
            Some(block.number),
            Some(block.to_header().compute_hash()),
            &mut buf,
        );
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!("[chain ID {chain_id}] All blocks verified successfully.\n",)
        );

        // incorrect hash
        let hash = Hash::default(); // intentionally wrong hash
        let mut buf = Vec::new();
        let result = verify(chain_id, Some(block.number), Some(hash), &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!(
                "[chain ID {chain_id}] block hash verification failed for block {}: expected hash {}, got {}.\n[chain ID {chain_id}] Verification completed with 1 errors.\n",
                block.number,
                hash.to_hex(),
                block.to_header().compute_hash().to_hex(),
            )
        );
    }

    #[test]
    fn prints_message_if_block_not_found() {
        let chain_id = 146;
        let block_number = 0;

        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        let mut buf = Vec::new();
        let result = verify(
            chain_id,
            Some(block_number),
            Some(Hash::default()),
            &mut buf,
        );
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!(
                "[chain ID {chain_id}] requested block {block_number} does not exit\n[chain ID {chain_id}] Verification completed with 1 errors.\n"
            )
        );
    }

    #[test]
    fn checks_number_of_block() {
        let chain_id = 146;

        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut db = RocksBlockDb::open(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();

        let mut block = Block::default();
        db.put(chain_id, block.clone()).unwrap();

        // block number matches
        let mut buf = Vec::new();
        let result = verify(chain_id, None, None, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!("[chain ID {chain_id}] All blocks verified successfully.\n",)
        );

        // block number mismatches
        // at block number 0, blocknumber (0) and block.number (1) mismatch
        block.number = 1;
        let block_number = 0; // intentionally wrong block number
        let data = proto::Block::from(block.clone()).encode_to_vec();
        db.put_raw(chain_id, block_number, &data).unwrap();

        let mut buf = Vec::new();
        let result = verify(chain_id, None, None, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!(
                "[chain ID {chain_id}] block number mismatch: block number in key = {block_number}, block.number = {}.\n[chain ID {chain_id}] Verification completed with 1 errors.\n",
                block.number
            )
        );
    }

    #[test]
    fn checks_parent_hash_of_block() {
        let chain_id = 146;

        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut db = RocksBlockDb::open(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();

        let block0 = Block::default();
        db.put(chain_id, block0.clone()).unwrap();

        // correct hash
        let mut block1 = Block {
            number: 1,
            parent_hash: block0.to_header().compute_hash(),
            ..Block::default()
        };
        db.put(chain_id, block1.clone()).unwrap();

        let mut buf = Vec::new();
        let result = verify(chain_id, None, None, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!("[chain ID {chain_id}] All blocks verified successfully.\n",)
        );

        // incorrect parent hash
        block1.parent_hash = Hash::default(); // intentionally wrong parent hash
        db.put(chain_id, block1.clone()).unwrap();

        let mut buf = Vec::new();
        let result = verify(chain_id, None, None, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!(
                "[chain ID {chain_id}] parent hash verification failed for block {}: expected hash {}, got {}.\n[chain ID {chain_id}] Verification completed with 1 errors.\n",
                block1.number,
                block0.to_header().compute_hash().to_hex(),
                block1.parent_hash.to_hex()
            )
        );
    }

    #[test]
    fn skips_parent_hash_check_between_disjoint_ranges() {
        let chain_id = 146;

        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut db = RocksBlockDb::open(tmpdir.path().join(BLOCK_DB_NAME)).unwrap();

        let mut block = Block::default();
        db.put(chain_id, block.clone()).unwrap();

        // correct hash
        block.parent_hash = block.to_header().compute_hash();
        block.number = 1;
        db.put(chain_id, block.clone()).unwrap();

        // skip one block and use mismatching parent hash
        block.parent_hash = Hash::default();
        block.number = 3;
        db.put(chain_id, block.clone()).unwrap();

        // correct hash
        block.parent_hash = block.to_header().compute_hash();
        block.number = 4;
        db.put(chain_id, block.clone()).unwrap();

        let mut buf = Vec::new();
        let result = verify(chain_id, None, None, &mut buf);
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!("[chain ID {chain_id}] All blocks verified successfully.\n",)
        );
    }
}
