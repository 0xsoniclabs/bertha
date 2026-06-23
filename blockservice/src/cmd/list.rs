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

use std::path::Path;

use crate::{BlockRange, app_dir::open_app_dir, config::ChainConfig, db::BlockDb, grpc::RpcClient};

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
                    chain_range.has_rules_update_heights,
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
                let has_rules_update_heights = db.get_rules_update_heights(chain_id)?.is_some();
                let has_corrections = db.get_corrections(chain_id)?.is_some();
                Ok((chain_id, ranges, has_rules_update_heights, has_corrections))
            })
            .collect::<Result<_, crate::error::Error>>()?
    };

    for (chain_id, ranges, has_rules_update_heights, has_corrections) in chain_listings {
        let chain_cfg = cfg
            .get_chain_config(chain_id)
            .unwrap_or(ChainConfig::new(chain_id));

        writeln!(writer, "{}", chain_cfg.pretty_name())?;

        let has_rules_update_heights_str = if has_rules_update_heights {
            "yes"
        } else {
            "no"
        };
        let has_corrections_str = if has_corrections { "yes" } else { "no" };
        writeln!(
            writer,
            "├── rules update heights: {has_rules_update_heights_str}"
        )?;
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
    use tonic::metadata::{Ascii, MetadataValue};

    use super::*;
    use crate::{
        app_dir::init_app_dir,
        config::ChainConfig,
        db::{
            BlockDb, CHAIN_IDS_KEY, KvDb, make_block_ranges_key, serialize_block_ranges,
            serialize_chain_ids,
        },
        grpc::{
            auth::{self, AUTHORIZATION_HEADER_NAME},
            proto_rpc::{self, BlockRange, ChainListing, ChainListings},
            test_utils::{MockRpcServer, TestServer},
        },
        test_templates::auth_token,
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
    async fn print_availability_of_rules_update_heights_and_corrections() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let mut buf = Vec::new();
        let result = list(tmpdir.path(), Some(1), None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] (no name): (no description)
                ├── rules update heights: no
                ├── corrections: no
                └── no blocks
                ",
            }
        );

        let (_, mut db) = open_app_dir(tmpdir.path(), false).unwrap();
        db.put_rules_update_heights(1, b"rules-update-heights")
            .unwrap();
        db.put_corrections(1, b"corrections").unwrap();
        drop(db);

        let mut buf = Vec::new();
        let result = list(tmpdir.path(), Some(1), None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] (no name): (no description)
                ├── rules update heights: yes
                ├── corrections: yes
                └── no blocks
                ",
            }
        );
    }

    #[tokio::test]
    async fn prints_all_stored_ranges() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        // no blocks for chain id
        let mut buf = Vec::new();
        let result = list(tmpdir.path(), Some(1), None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] (no name): (no description)
                ├── rules update heights: no
                ├── corrections: no
                └── no blocks
               ",
            }
        );

        // block ranges for chain id
        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
        db.kv_db()
            .put_raw(
                &make_block_ranges_key(1),
                &serialize_block_ranges([2..=4, 6..=8]),
            )
            .unwrap();
        db.kv_db()
            .put_raw(&CHAIN_IDS_KEY, &serialize_chain_ids([1]))
            .unwrap();
        drop(db);

        let mut buf = Vec::new();
        let result = list(tmpdir.path(), Some(1), None, &mut buf).await;
        assert!(result.is_ok());

        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] (no name): (no description)
                ├── rules update heights: no
                ├── corrections: no
                ├── 2 - 4
                └── 6 - 8
                "
            }
        );

        // block ranges for multiple chain ids
        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
        db.kv_db()
            .put_raw(&make_block_ranges_key(3), &serialize_block_ranges([3..=5]))
            .unwrap();
        db.kv_db()
            .put_raw(&CHAIN_IDS_KEY, &serialize_chain_ids([1, 3]))
            .unwrap();
        drop(db);

        let mut buf = Vec::new();
        let result = list(tmpdir.path(), None, None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] (no name): (no description)
                ├── rules update heights: no
                ├── corrections: no
                ├── 2 - 4
                └── 6 - 8
                [3] (no name): (no description)
                ├── rules update heights: no
                ├── corrections: no
                └── 3 - 5
                ",
            }
        );
    }

    #[tokio::test]
    async fn uses_config_file_name_and_description_if_available() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let mut buf = Vec::new();
        let result = list(tmpdir.path(), Some(1), None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] (no name): (no description)
                ├── rules update heights: no
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
        let (mut cfg, _db) = open_app_dir(tmpdir.path(), false).unwrap();
        cfg.add_chain(chain_cfg.clone()).unwrap();

        let mut buf = Vec::new();
        let result = list(tmpdir.path(), Some(1), None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [1] Test Chain: A test chain
                ├── rules update heights: no
                ├── corrections: no
                └── no blocks
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
        let (mut cfg, mut db) = open_app_dir(tmpdir.path(), false).unwrap();
        cfg.add_chain(chain_cfg.clone()).unwrap();

        // Add ranges for chain 7 w/o adding to config file
        for n in [2, 3, 4, 6, 7, 8] {
            db.put_bytes(7, n, b"block").unwrap();
        }
        drop(db);

        let mut buf = Vec::new();
        let result = list(tmpdir.path(), None, None, &mut buf).await;
        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            indoc::indoc! {"
                [7] (no name): (no description)
                ├── rules update heights: no
                ├── corrections: no
                ├── 2 - 4
                └── 6 - 8
                [32] Test Chain: A test chain
                ├── rules update heights: no
                ├── corrections: no
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
            let list_response = ChainListings {
                chain_listings: vec![ChainListing {
                    chain_id: 1,
                    block_ranges: vec![],
                    has_rules_update_heights: false,
                    has_corrections: false,
                }],
            };
            let mut mock_server = MockRpcServer::new();
            mock_server
                .expect_list()
                .returning(move |_| Ok(tonic::Response::new(list_response.clone())));
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
                    ├── rules update heights: no
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
                    has_rules_update_heights: true,
                    has_corrections: true,
                }],
            };
            let mut mock_server = MockRpcServer::new();
            mock_server
                .expect_list()
                .returning(move |_| Ok(tonic::Response::new(list_response.clone())));
            let server = TestServer::new(mock_server).await;

            let mut buf = Vec::new();
            let result = list(tmpdir.path(), None, Some(server.address.clone()), &mut buf).await;
            assert!(result.is_ok());
            assert_eq!(
                String::from_utf8(buf).unwrap(),
                indoc::indoc! {"
                    [1] (no name): (no description)
                    ├── rules update heights: yes
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
                        has_rules_update_heights: true,
                        has_corrections: false,
                    },
                    ChainListing {
                        chain_id: 3,
                        block_ranges: vec![BlockRange { from: 3, to: 5 }],
                        has_rules_update_heights: false,
                        has_corrections: true,
                    },
                ],
            };
            let mut mock_server = MockRpcServer::new();
            mock_server
                .expect_list()
                .returning(move |_| Ok(tonic::Response::new(list_response.clone())));
            let server = TestServer::new(mock_server).await;

            let mut buf = Vec::new();
            let result = list(tmpdir.path(), None, Some(server.address.clone()), &mut buf).await;
            assert!(result.is_ok());
            assert_eq!(
                String::from_utf8(buf).unwrap(),
                indoc::indoc! {"
                    [1] (no name): (no description)
                    ├── rules update heights: yes
                    ├── corrections: no
                    ├── 2 - 4
                    └── 6 - 8
                    [3] (no name): (no description)
                    ├── rules update heights: no
                    ├── corrections: yes
                    └── 3 - 5
                    "
                }
            );
        }
    }

    #[rstest_reuse::apply(auth_token)]
    #[tokio::test]
    async fn provides_auth_token_when_supplied(auth_token: Option<MetadataValue<Ascii>>) {
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
                        has_rules_update_heights: false,
                        has_corrections: false,
                    }],
                }))
            });

        let server = TestServer::new(mock_server).await;
        let mut buf = Vec::new();
        let result = list(tmpdir.path(), None, Some(server.address.clone()), &mut buf).await;
        assert!(result.is_ok(), "List should succeed");
    }
}
