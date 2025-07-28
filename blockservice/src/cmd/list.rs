use std::path::Path;

use crate::{app_dir::open_app_dir, config::ChainConfig, db::BlockDb, grpc::RpcClient};

pub async fn list(
    app_dir: impl AsRef<Path>,
    chain_id: Option<u64>,
    url: Option<String>,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if matches!(chain_id, Some(0)) {
        return Err("chain ID cannot be 0".into());
    }
    let (cfg, db) = open_app_dir(app_dir, true)?;
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
        let chain_ids = if let Some(chain_id) = chain_id {
            vec![chain_id]
        } else {
            let mut chain_ids = [db.get_chain_ids()?, cfg.get_chain_ids()]
                .concat()
                .into_iter()
                .collect::<Vec<_>>();
            chain_ids.sort();
            chain_ids.dedup();
            chain_ids
        };
        chain_ids
            .into_iter()
            .map(|chain_id| {
                db.get_ranges_of_chain_id(chain_id)
                    .map(|ranges| (chain_id, ranges))
            })
            .collect::<Result<Vec<_>, _>>()?
    };

    for (chain_id, ranges) in chain_ranges {
        let chain_cfg = cfg
            .get_chain_config(chain_id)
            .unwrap_or(ChainConfig::new(chain_id));

        writeln!(writer, "{}", chain_cfg.pretty_name())?;

        if ranges.is_empty() {
            writeln!(writer, "└── no blocks")?;
        }
        for (i, range) in ranges.iter().enumerate() {
            let (start, end) = (range.start(), range.end());
            let symbol = if i == ranges.len() - 1 {
                "└──"
            } else {
                "├──"
            };
            writeln!(writer, "{symbol} {start} - {end}")?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::{
        app_dir::{BLOCK_DB_NAME, init_app_dir},
        config::ChainConfig,
        db::RocksBlockDb,
        grpc::{
            proto_rpc::{BlockRange, ChainRange, ChainRanges},
            test_utils::{MockRpcServer, TestServer},
        },
        utils::test_dir::{Permissions, TestDir},
    };

    #[tokio::test]
    async fn fails_if_app_dir_is_not_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        let result = list(tmpdir.path(), None, None, std::io::sink()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }

    #[tokio::test]
    async fn fails_if_no_read_permissions() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        // create database
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        // remove read permissions
        tmpdir.set_permissions(Permissions::WriteOnly).unwrap();

        let result = list(tmpdir.path(), None, None, std::io::sink()).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
    }

    #[tokio::test]
    async fn fails_for_invalid_server_url() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let url = "invalid-url".to_string();

        let result = list(tmpdir.path(), None, Some(url), std::io::sink()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("transport error"));
    }

    #[tokio::test]
    async fn fails_on_server_error() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let mut mock_server = MockRpcServer::new();
        mock_server
            .expect_list()
            .returning(|_| Err(tonic::Status::internal("Server error")));
        let server = TestServer::new(mock_server).await;

        let result = list(
            tmpdir.path(),
            None,
            Some(server.address.clone()),
            std::io::sink(),
        )
        .await;

        let err = result.expect_err("Fetch should fail with server error");
        assert!(err.to_string().contains("Server error"));
    }

    #[tokio::test]
    async fn fails_for_chain_id_zero() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let result = list(tmpdir.path(), Some(0), None, std::io::sink()).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "chain ID cannot be 0");
    }

    #[tokio::test]
    async fn prints_message_for_each_range() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let chain_cfg = ChainConfig {
            name: "Test Chain".to_string(),
            ..ChainConfig::new(1)
        };
        let (mut cfg, _) = open_app_dir(tmpdir.path(), true).unwrap();
        cfg.add_chain(chain_cfg.clone()).unwrap();

        // no blocks for chain id
        let mut buf = Vec::new();
        let result = list(tmpdir.path(), Some(1), None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] Test Chain: (no description)
                └── no blocks
               ",
            }
        );

        // block ranges for chain id
        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
        db.put_ranges_of_chain_id(1, &[2..=4, 6..=8]).unwrap();
        drop(db);

        let mut buf = Vec::new();
        let result = list(tmpdir.path(), Some(1), None, &mut buf).await;
        assert!(result.is_ok());

        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] Test Chain: (no description)
                ├── 2 - 4
                └── 6 - 8
                "
            }
        );

        // block ranges for multiple chain ids
        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
        db.put_ranges_of_chain_id(3, &[3..=5]).unwrap();
        db.put_chain_ids(&[1, 3]).unwrap();
        drop(db);

        let mut buf = Vec::new();
        let result = list(tmpdir.path(), None, None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] Test Chain: (no description)
                ├── 2 - 4
                └── 6 - 8
                [3] (no name): (no description)
                └── 3 - 5
                ",
            }
        );
    }

    #[tokio::test]
    async fn prints_message_for_all_chains_in_db_and_config_file() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        // Add chain 32 to config file only
        let chain_cfg = ChainConfig {
            name: "Test Chain".to_string(),
            description: "A test chain".to_string(),
            ..ChainConfig::new(32)
        };
        let (mut cfg, _) = open_app_dir(tmpdir.path(), true).unwrap();
        cfg.add_chain(chain_cfg.clone()).unwrap();

        // Add ranges for chain 7 w/o adding to config file
        let db_path = tmpdir.path().join(BLOCK_DB_NAME);
        let db = RocksBlockDb::open(db_path.clone()).unwrap();
        db.put_ranges_of_chain_id(7, &[2..=4, 6..=8]).unwrap();
        db.put_chain_ids(&[7]).unwrap();
        drop(db);

        let mut buf = Vec::new();
        let result = list(tmpdir.path(), None, None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [7] (no name): (no description)
                ├── 2 - 4
                └── 6 - 8
                [32] Test Chain: A test chain
                └── no blocks
                ",
            }
        );
    }

    #[tokio::test]
    async fn prints_message_for_each_remote_range() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

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
            let result = list(
                tmpdir.path(),
                Some(1),
                Some(server.address.clone()),
                &mut buf,
            )
            .await;
            assert!(result.is_ok());
            assert_eq!(
                String::from_utf8(buf).unwrap(),
                indoc::indoc! {"
                    [1] (no name): (no description)
                    └── no blocks
                    "
                }
            );
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
            let result = list(tmpdir.path(), None, Some(server.address.clone()), &mut buf).await;
            assert!(result.is_ok());
            assert_eq!(
                String::from_utf8(buf).unwrap(),
                indoc::indoc! {"
                    [1] (no name): (no description)
                    ├── 2 - 4
                    └── 6 - 8
                    "
                }
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
            let result = list(tmpdir.path(), None, Some(server.address.clone()), &mut buf).await;
            assert!(result.is_ok());
            assert_eq!(
                String::from_utf8(buf).unwrap(),
                indoc::indoc! {"
                    [1] (no name): (no description)
                    ├── 2 - 4
                    └── 6 - 8
                    [3] (no name): (no description)
                    └── 3 - 5
                    "
                }
            );
        }
    }
}
