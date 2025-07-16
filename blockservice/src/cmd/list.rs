use std::path::Path;

use crate::{app_dir::open_app_dir, db::BlockDb, grpc::RpcClient};

pub async fn list(
    chain_id: Option<u64>,
    url: Option<String>,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let chain_ranges = if let Some(url) = url {
        let mut client = RpcClient::try_new(url).await?;
        let remote_ranges = client.list(chain_id).await?;
        remote_ranges
            .chain_ranges
            .into_iter()
            .map(|chain_range| {
                (
                    chain_range.chain_id,
                    chain_range
                        .block_ranges
                        .into_iter()
                        .map(From::from)
                        .collect(),
                )
            })
            .collect()
    } else {
        let app_dir = Path::new("./").canonicalize()?;
        let db = open_app_dir(app_dir, true)?;

        let chain_ids = match chain_id {
            Some(chain_id) => vec![chain_id],
            None => db.get_chain_ids()?,
        };
        chain_ids
            .into_iter()
            .map(|chain_id| {
                db.get_ranges_of_chain_id(chain_id)
                    .map(|ranges| (chain_id, ranges))
            })
            .collect::<Result<Vec<_>, _>>()?
    };

    if chain_ranges.is_empty() {
        writeln!(writer, "no blocks in database")?;
    }
    for (chain_id, ranges) in chain_ranges {
        if ranges.is_empty() {
            writeln!(writer, "[chain ID {chain_id}] no blocks")?;
        }
        for range in ranges {
            writeln!(
                writer,
                "[chain ID {chain_id}] {} - {}",
                range.start(),
                range.end()
            )?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::{
        app_dir::BLOCK_DB_NAME,
        cmd::{ChangeWorkingDir, init},
        db::RocksBlockDb,
        grpc::{
            proto_rpc::{BlockRange, ChainRange, ChainRanges},
            test_utils::{MockRpcServer, TestServer},
        },
    };

    #[tokio::test]
    async fn fails_if_app_dir_is_not_initialized() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());

        let result = list(None, None, std::io::sink()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no database found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }

    #[tokio::test]
    async fn fails_if_no_read_permissions() {
        let tmpdir = tempfile::tempdir().unwrap();

        // create database
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        // remove read permissions
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o333)).unwrap();

        let result = list(None, None, std::io::sink()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to open"));
    }

    #[tokio::test]
    async fn fails_for_invalid_server_url() {
        let url = "invalid-url".to_string();
        let result = list(None, Some(url), std::io::sink()).await;
        let err = result.expect_err("Fetch should fail with invalid url");

        assert_eq!(err.to_string(), "transport error");
    }

    #[tokio::test]
    async fn fails_on_server_error() {
        let mut mock_server = MockRpcServer::new();
        mock_server
            .expect_list()
            .returning(|_| Err(tonic::Status::internal("Server error")));
        let server = TestServer::new(mock_server).await;

        let result = list(None, Some(server.address.clone()), std::io::sink()).await;

        let err = result.expect_err("Fetch should fail with server error");
        assert!(err.to_string().contains("Server error"));
    }

    #[tokio::test]
    async fn prints_message_for_each_range() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();

        // no blocks for chain id
        let mut buf = Vec::new();
        let result = list(Some(1), None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(String::from_utf8(buf).unwrap(), "[chain ID 1] no blocks\n");

        // block ranges for chain id
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        let db = RocksBlockDb::open(db_path.clone()).unwrap();
        db.put_ranges_of_chain_id(1, &[2..=4, 6..=8]).unwrap();
        drop(db);

        let mut buf = Vec::new();
        let result = list(Some(1), None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[chain ID 1] 2 - 4\n[chain ID 1] 6 - 8\n"
        );

        // block ranges for multiple chain ids
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        let db = RocksBlockDb::open(db_path.clone()).unwrap();
        db.put_ranges_of_chain_id(3, &[3..=5]).unwrap();
        db.put_chain_ids(&[1, 3]).unwrap();
        drop(db);

        let mut buf = Vec::new();
        let result = list(None, None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[chain ID 1] 2 - 4\n[chain ID 1] 6 - 8\n[chain ID 3] 3 - 5\n"
        );
    }

    #[tokio::test]
    async fn prints_message_for_each_remote_range() {
        {
            // no blocks for chain id
            let list_response = ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 1,
                    block_ranges: vec![],
                }],
            };
            let mut mock_server = MockRpcServer::new();
            mock_server.expect_list().returning({
                let list_response = list_response.clone();
                move |_| Ok(tonic::Response::new(list_response.clone()))
            });
            let server = TestServer::new(mock_server).await;

            let mut buf = Vec::new();
            let result = list(Some(1), Some(server.address.clone()), &mut buf).await;
            assert!(result.is_ok());
            assert_eq!(String::from_utf8(buf).unwrap(), "[chain ID 1] no blocks\n");
        }
        {
            // block ranges for chain id
            let list_response = ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 1,
                    block_ranges: vec![
                        BlockRange { from: 2, to: 4 },
                        BlockRange { from: 6, to: 8 },
                    ],
                }],
            };
            let mut mock_server = MockRpcServer::new();
            mock_server.expect_list().returning({
                let list_response = list_response.clone();
                move |_| Ok(tonic::Response::new(list_response.clone()))
            });
            let server = TestServer::new(mock_server).await;

            let mut buf = Vec::new();
            let result = list(None, Some(server.address.clone()), &mut buf).await;
            assert!(result.is_ok());
            assert_eq!(
                String::from_utf8(buf).unwrap(),
                "[chain ID 1] 2 - 4\n[chain ID 1] 6 - 8\n"
            );
        }
        {
            // block ranges for multiple chain ids
            let list_response = ChainRanges {
                chain_ranges: vec![
                    ChainRange {
                        chain_id: 1,
                        block_ranges: vec![
                            BlockRange { from: 2, to: 4 },
                            BlockRange { from: 6, to: 8 },
                        ],
                    },
                    ChainRange {
                        chain_id: 3,
                        block_ranges: vec![BlockRange { from: 3, to: 5 }],
                    },
                ],
            };
            let mut mock_server = MockRpcServer::new();
            mock_server.expect_list().returning({
                let list_response = list_response.clone();
                move |_| Ok(tonic::Response::new(list_response.clone()))
            });
            let server = TestServer::new(mock_server).await;

            let mut buf = Vec::new();
            let result = list(None, Some(server.address.clone()), &mut buf).await;
            assert!(result.is_ok());
            assert_eq!(
                String::from_utf8(buf).unwrap(),
                "[chain ID 1] 2 - 4\n[chain ID 1] 6 - 8\n[chain ID 3] 3 - 5\n"
            );
        }
    }
}
