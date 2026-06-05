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

use crate::{
    BlockRange,
    app_dir::open_app_dir,
    config::{ChainConfig, Config},
    db::BlockDb,
    grpc::RpcClient,
};

pub async fn list(
    app_dir: impl AsRef<Path>,
    chain_id: Option<u64>,
    url: Option<String>,
    writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (cfg, db) = open_app_dir(app_dir, true)?;
    list_internal(cfg, &db, chain_id, url, writer).await
}

async fn list_internal(
    cfg: Config,
    db: &impl BlockDb,
    chain_id: Option<u64>,
    url: Option<String>,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if matches!(chain_id, Some(0)) {
        return Err("chain ID cannot be 0".into());
    }
    let auth_token = cfg.get_auth_token().cloned();
    let chain_listings: Vec<(u64, Vec<BlockRange>, bool, bool)> = if let Some(url) = url {
        let mut client = RpcClient::try_new(url, auth_token).await?;
        let chain_listings = client.list(chain_id).await?;
        chain_listings
            .chain_listings
            .into_iter()
            .map(|chain_range| {
                (
                    chain_range.chain_id,
                    chain_range
                        .block_ranges
                        .into_iter()
                        .map(From::from)
                        .collect(),
                    chain_range.has_upgrade_heights,
                    chain_range.has_corrections,
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
                let ranges = db.get_ranges_of_chain_id(chain_id)?;
                let has_upgrade_heights = db.get_upgrade_heights(chain_id)?.is_some();
                let has_corrections = db.get_corrections(chain_id)?.is_some();
                Ok((chain_id, ranges, has_upgrade_heights, has_corrections))
            })
            .collect::<Result<_, crate::error::Error>>()?
    };

    for (chain_id, ranges, has_upgrade_heights, has_corrections) in chain_listings {
        let chain_cfg = cfg
            .get_chain_config(chain_id)
            .unwrap_or(ChainConfig::new(chain_id));

        writeln!(writer, "{}", chain_cfg.pretty_name())?;

        let has_upgrade_heights_str = if has_upgrade_heights { "yes" } else { "no" };
        let has_corrections_str = if has_corrections { "yes" } else { "no" };
        writeln!(writer, "├── upgrade heights: {has_upgrade_heights_str}")?;
        writeln!(writer, "├── corrections: {has_corrections_str}")?;

        if ranges.is_empty() {
            writeln!(writer, "└── no blocks")?;
        } else {
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
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use mockall::predicate::eq;
    use tonic::metadata::{Ascii, MetadataValue};

    use super::*;
    use crate::{
        app_dir::{init_app_dir, open_app_dir},
        config::ChainConfig,
        db::MockBlockDb,
        grpc::{
            auth::{self, AUTHORIZATION_HEADER_NAME},
            proto_rpc::{self, BlockRange, ChainListing, ChainListings},
            test_utils::{MockRpcServer, TestServer},
        },
        test_templates::auth_token,
        utils::test_dir::{Permissions, TestDir},
    };

    #[tokio::test]
    async fn list_fails_if_app_dir_is_not_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        let result = list(tmpdir.path(), None, None, std::io::sink()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }

    #[tokio::test]
    async fn list_fails_if_no_read_permissions() {
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
    async fn list_internal_fails_for_invalid_server_url() {
        let url = "invalid-url".to_string();
        let db = MockBlockDb::new();

        let result = list_internal(Config::default(), &db, None, Some(url), std::io::sink()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("transport error"));
    }

    #[tokio::test]
    async fn list_internal_fails_on_server_error() {
        let mut mock_server = MockRpcServer::new();
        mock_server
            .expect_list()
            .returning(|_| Err(tonic::Status::internal("Server error")));
        let server = TestServer::new(mock_server).await;

        let db = MockBlockDb::new();
        let result = list_internal(
            Config::default(),
            &db,
            None,
            Some(server.address.clone()),
            std::io::sink(),
        )
        .await;

        let err = result.expect_err("Fetch should fail with server error");
        assert!(err.to_string().contains("Server error"));
    }

    #[tokio::test]
    async fn list_internal_fails_for_chain_id_zero() {
        let db = MockBlockDb::new();
        let result = list_internal(Config::default(), &db, Some(0), None, std::io::sink()).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "chain ID cannot be 0");
    }

    #[tokio::test]
    async fn list_internal_print_availability_of_upgrade_heights_and_corrections() {
        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(1u64))
            .return_once(|_| Ok(vec![]));
        db.expect_get_upgrade_heights()
            .with(eq(1u64))
            .return_once(|_| Ok(None));
        db.expect_get_corrections()
            .with(eq(1u64))
            .return_once(|_| Ok(None));

        let mut buf = Vec::new();
        let result = list_internal(Config::default(), &db, Some(1), None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] (no name): (no description)
                ├── upgrade heights: no
                ├── corrections: no
                └── no blocks
                ",
            }
        );

        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(1u64))
            .return_once(|_| Ok(vec![]));
        db.expect_get_upgrade_heights()
            .with(eq(1u64))
            .return_once(|_| Ok(Some(b"upgrade-heights".to_vec())));
        db.expect_get_corrections()
            .with(eq(1u64))
            .return_once(|_| Ok(Some(b"corrections".to_vec())));

        let mut buf = Vec::new();
        let result = list_internal(Config::default(), &db, Some(1), None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] (no name): (no description)
                ├── upgrade heights: yes
                ├── corrections: yes
                └── no blocks
                ",
            }
        );
    }

    #[tokio::test]
    async fn list_internal_prints_all_stored_ranges() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(1u64))
            .return_once(|_| Ok(vec![]));
        db.expect_get_upgrade_heights()
            .with(eq(1u64))
            .return_once(|_| Ok(None));
        db.expect_get_corrections()
            .with(eq(1u64))
            .return_once(|_| Ok(None));

        let mut buf = Vec::new();
        let result = list_internal(Config::default(), &db, Some(1), None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] (no name): (no description)
                ├── upgrade heights: no
                ├── corrections: no
                └── no blocks
               ",
            }
        );
    }

    #[tokio::test]
    async fn list_internal_prints_block_ranges_for_chain() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let chain_cfg = ChainConfig::new(1);
        let (mut cfg, _) = open_app_dir(tmpdir.path(), true).unwrap();
        cfg.add_chain(chain_cfg.clone()).unwrap();

        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(1u64))
            .return_once(|_| Ok(vec![2..=4, 6..=8]));
        db.expect_get_upgrade_heights()
            .with(eq(1u64))
            .return_once(|_| Ok(None));
        db.expect_get_corrections()
            .with(eq(1u64))
            .return_once(|_| Ok(None));

        let mut buf = Vec::new();
        let result = list_internal(cfg, &db, Some(1), None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] (no name): (no description)
                ├── upgrade heights: no
                ├── corrections: no
                ├── 2 - 4
                └── 6 - 8
                "
            }
        );
    }

    #[tokio::test]
    async fn list_internal_prints_block_ranges_for_multiple_chains() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let chain_cfg = ChainConfig::new(1);
        let (mut cfg, _) = open_app_dir(tmpdir.path(), true).unwrap();
        cfg.add_chain(chain_cfg.clone()).unwrap();

        let mut db = MockBlockDb::new();
        db.expect_get_chain_ids().return_once(|| Ok(vec![3]));
        db.expect_get_ranges_of_chain_id()
            .with(eq(1u64))
            .return_once(|_| Ok(vec![2..=4, 6..=8]));
        db.expect_get_upgrade_heights()
            .with(eq(1u64))
            .return_once(|_| Ok(None));
        db.expect_get_corrections()
            .with(eq(1u64))
            .return_once(|_| Ok(None));
        db.expect_get_ranges_of_chain_id()
            .with(eq(3u64))
            .return_once(|_| Ok(vec![3..=5]));
        db.expect_get_upgrade_heights()
            .with(eq(3u64))
            .return_once(|_| Ok(None));
        db.expect_get_corrections()
            .with(eq(3u64))
            .return_once(|_| Ok(None));

        let mut buf = Vec::new();
        let result = list_internal(cfg, &db, None, None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] (no name): (no description)
                ├── upgrade heights: no
                ├── corrections: no
                ├── 2 - 4
                └── 6 - 8
                [3] (no name): (no description)
                ├── upgrade heights: no
                ├── corrections: no
                └── 3 - 5
                ",
            }
        );
    }

    #[tokio::test]
    async fn list_internal_uses_config_file_name_and_description_if_available() {
        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(1u64))
            .return_once(|_| Ok(vec![]));
        db.expect_get_upgrade_heights()
            .with(eq(1u64))
            .return_once(|_| Ok(None));
        db.expect_get_corrections()
            .with(eq(1u64))
            .return_once(|_| Ok(None));

        let mut buf = Vec::new();
        let result = list_internal(Config::default(), &db, Some(1), None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] (no name): (no description)
                ├── upgrade heights: no
                ├── corrections: no
                └── no blocks
                ",
            }
        );

        let chain_cfg = ChainConfig {
            name: "Test Chain".to_string(),
            description: "A test chain".to_string(),
            ..ChainConfig::new(1)
        };
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (mut cfg, _) = open_app_dir(tmpdir.path(), false).unwrap();
        cfg.add_chain(chain_cfg).unwrap();

        let mut db = MockBlockDb::new();
        db.expect_get_ranges_of_chain_id()
            .with(eq(1u64))
            .return_once(|_| Ok(vec![]));
        db.expect_get_upgrade_heights()
            .with(eq(1u64))
            .return_once(|_| Ok(None));
        db.expect_get_corrections()
            .with(eq(1u64))
            .return_once(|_| Ok(None));

        let mut buf = Vec::new();
        let result = list_internal(cfg, &db, Some(1), None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] Test Chain: A test chain
                ├── upgrade heights: no
                ├── corrections: no
                └── no blocks
                ",
            }
        );
    }

    #[tokio::test]
    async fn list_internal_prints_message_for_all_chains_in_db_and_config_file() {
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

        let mut db = MockBlockDb::new();
        // chain 7 is in db but not in config
        db.expect_get_chain_ids().return_once(|| Ok(vec![7]));
        db.expect_get_ranges_of_chain_id()
            .with(eq(7u64))
            .return_once(|_| Ok(vec![2..=4, 6..=8]));
        db.expect_get_upgrade_heights()
            .with(eq(7u64))
            .return_once(|_| Ok(None));
        db.expect_get_corrections()
            .with(eq(7u64))
            .return_once(|_| Ok(None));
        db.expect_get_ranges_of_chain_id()
            .with(eq(32u64))
            .return_once(|_| Ok(vec![]));
        db.expect_get_upgrade_heights()
            .with(eq(32u64))
            .return_once(|_| Ok(None));
        db.expect_get_corrections()
            .with(eq(32u64))
            .return_once(|_| Ok(None));

        let mut buf = Vec::new();
        let result = list_internal(cfg, &db, None, None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [7] (no name): (no description)
                ├── upgrade heights: no
                ├── corrections: no
                ├── 2 - 4
                └── 6 - 8
                [32] Test Chain: A test chain
                ├── upgrade heights: no
                ├── corrections: no
                └── no blocks
                ",
            }
        );
    }

    #[tokio::test]
    async fn list_internal_prints_message_for_each_remote_range() {
        {
            // no blocks for chain id
            let list_response = ChainListings {
                chain_listings: vec![ChainListing {
                    chain_id: 1,
                    block_ranges: vec![],
                    has_upgrade_heights: false,
                    has_corrections: false,
                }],
            };
            let mut mock_server = MockRpcServer::new();
            mock_server
                .expect_list()
                .returning(move |_| Ok(tonic::Response::new(list_response.clone())));
            let server = TestServer::new(mock_server).await;

            let db = MockBlockDb::new();
            let mut buf = Vec::new();
            let result = list_internal(
                Config::default(),
                &db,
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
                    ├── upgrade heights: no
                    ├── corrections: no
                    └── no blocks
                    "
                }
            );
        }
        {
            // metadata and block ranges for chain id
            let list_response = ChainListings {
                chain_listings: vec![ChainListing {
                    chain_id: 1,
                    block_ranges: vec![
                        BlockRange { from: 2, to: 4 },
                        BlockRange { from: 6, to: 8 },
                    ],
                    has_upgrade_heights: true,
                    has_corrections: true,
                }],
            };
            let mut mock_server = MockRpcServer::new();
            mock_server
                .expect_list()
                .returning(move |_| Ok(tonic::Response::new(list_response.clone())));
            let server = TestServer::new(mock_server).await;

            let db = MockBlockDb::new();
            let mut buf = Vec::new();
            let result = list_internal(
                Config::default(),
                &db,
                None,
                Some(server.address.clone()),
                &mut buf,
            )
            .await;
            assert!(result.is_ok());
            assert_eq!(
                String::from_utf8(buf).unwrap(),
                indoc::indoc! {"
                    [1] (no name): (no description)
                    ├── upgrade heights: yes
                    ├── corrections: yes
                    ├── 2 - 4
                    └── 6 - 8
                    "
                }
            );
        }
        {
            // block ranges for multiple chain ids
            let list_response = ChainListings {
                chain_listings: vec![
                    ChainListing {
                        chain_id: 1,
                        block_ranges: vec![
                            BlockRange { from: 2, to: 4 },
                            BlockRange { from: 6, to: 8 },
                        ],
                        has_upgrade_heights: true,
                        has_corrections: false,
                    },
                    ChainListing {
                        chain_id: 3,
                        block_ranges: vec![BlockRange { from: 3, to: 5 }],
                        has_upgrade_heights: false,
                        has_corrections: true,
                    },
                ],
            };
            let mut mock_server = MockRpcServer::new();
            mock_server
                .expect_list()
                .returning(move |_| Ok(tonic::Response::new(list_response.clone())));
            let server = TestServer::new(mock_server).await;

            let db = MockBlockDb::new();
            let mut buf = Vec::new();
            let result = list_internal(
                Config::default(),
                &db,
                None,
                Some(server.address.clone()),
                &mut buf,
            )
            .await;
            assert!(result.is_ok());
            assert_eq!(
                String::from_utf8(buf).unwrap(),
                indoc::indoc! {"
                    [1] (no name): (no description)
                    ├── upgrade heights: yes
                    ├── corrections: no
                    ├── 2 - 4
                    └── 6 - 8
                    [3] (no name): (no description)
                    ├── upgrade heights: no
                    ├── corrections: yes
                    └── 3 - 5
                    "
                }
            );
        }
    }

    #[rstest_reuse::apply(auth_token)]
    #[tokio::test]
    async fn list_internal_provides_auth_token_when_supplied(auth_token: Option<MetadataValue<Ascii>>) {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (mut cfg, _db) = open_app_dir(tmpdir.path(), true).unwrap();

        cfg.set_auth_token(auth_token.clone()).unwrap();

        let mut mock_server = MockRpcServer::new();
        mock_server
            .expect_list()
            .withf({
                let auth_token = auth_token.clone();
                move |request| {
                    if auth_token.is_some() {
                        let req_token = request.metadata().get(AUTHORIZATION_HEADER_NAME);
                        auth_token.as_ref() == req_token
                    } else {
                        true
                    }
                }
            })
            .returning(|_| {
                Ok(tonic::Response::new(ChainListings {
                    chain_listings: vec![ChainListing {
                        chain_id: 1,
                        block_ranges: vec![proto_rpc::BlockRange { from: 0, to: 0 }],
                        has_upgrade_heights: false,
                        has_corrections: false,
                    }],
                }))
            });

        let server = TestServer::new(mock_server).await;
        let db = MockBlockDb::new();
        let mut buf = Vec::new();
        let result = list_internal(cfg, &db, None, Some(server.address.clone()), &mut buf).await;
        assert!(result.is_ok(), "List should succeed");
    }
}
