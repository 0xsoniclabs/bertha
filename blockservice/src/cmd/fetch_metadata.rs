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

use crate::{app_dir::open_app_dir, db::BlockDb, grpc::RpcClient};

/// Fetches metadata from a remote blockservice and stores it in the local database.
pub async fn fetch_metadata(
    app_dir: impl AsRef<Path>,
    url: String,
    chain_id: u64,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (cfg, mut db) = open_app_dir(&app_dir, false)?;

    let auth_token = cfg.get_auth_token().cloned();

    let mut client = RpcClient::try_new(url, auth_token).await?;
    let metadata = client.get_metadata(chain_id).await?;

    if let Some(rules_update_heights) = metadata.rules_update_heights {
        db.put_rules_update_heights(chain_id, &rules_update_heights)?;
        writeln!(
            writer,
            "Stored rules update heights for chain ID {chain_id}"
        )?;
    } else {
        writeln!(
            writer,
            "No rules update heights available for chain ID {chain_id}"
        )?;
    }

    if let Some(corrections) = metadata.corrections {
        db.put_corrections(chain_id, &corrections)?;
        writeln!(writer, "Stored corrections for chain ID {chain_id}")?;
    } else {
        writeln!(writer, "No corrections available for chain ID {chain_id}")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use tonic::metadata::{Ascii, MetadataValue};

    use super::*;
    use crate::{
        app_dir::init_app_dir,
        grpc::{
            auth::{self, AUTHORIZATION_HEADER_NAME},
            proto_rpc,
            test_utils::{MockRpcServer, TestServer},
        },
        test_templates::auth_token,
        utils::test_dir::{Permissions, TestDir},
    };

    #[rstest::rstest]
    #[case::no_metadata(None, None)]
    #[case::only_rules_update_heights(Some(b"rules-update-heights".to_vec()), None)]
    #[case::only_corrections(None, Some(b"corrections".to_vec()))]
    #[case::both_metadata(Some(b"rules-update-heights".to_vec()), Some(b"corrections".to_vec()))]
    #[tokio::test]
    async fn fetches_metadata_and_stores_in_database(
        #[case] rules_update_heights: Option<Vec<u8>>,
        #[case] corrections: Option<Vec<u8>>,
    ) {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let mut server = MockRpcServer::new();
        server.expect_get_metadata().returning({
            let rules_update_heights = rules_update_heights.clone();
            let corrections = corrections.clone();
            move |_| {
                Ok(tonic::Response::new(proto_rpc::Metadata {
                    rules_update_heights: rules_update_heights.clone(),
                    corrections: corrections.clone(),
                }))
            }
        });
        let server = TestServer::new(server).await;

        let chain_id = 1;

        let mut log = Vec::new();
        let result =
            fetch_metadata(tmpdir.path(), server.address.clone(), chain_id, &mut log).await;

        assert!(result.is_ok());
        let log_str = String::from_utf8(log).unwrap();

        let (_cfg, db) = open_app_dir(tmpdir.path(), true).unwrap();

        if rules_update_heights.is_some() {
            assert!(log_str.contains(&format!(
                "Stored rules update heights for chain ID {chain_id}"
            )));
            let stored = db.get_rules_update_heights(chain_id).unwrap().unwrap();
            assert_eq!(stored, rules_update_heights.unwrap());
        } else {
            assert!(log_str.contains(&format!(
                "No rules update heights available for chain ID {chain_id}"
            )));
            assert!(db.get_rules_update_heights(chain_id).unwrap().is_none());
        }

        if corrections.is_some() {
            assert!(log_str.contains(&format!("Stored corrections for chain ID {chain_id}")));
            let stored = db.get_corrections(chain_id).unwrap().unwrap();
            assert_eq!(stored, corrections.unwrap());
        } else {
            assert!(log_str.contains(&format!("No corrections available for chain ID {chain_id}")));
            assert!(db.get_corrections(chain_id).unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn fails_if_app_dir_is_not_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        let mut log = Vec::new();
        let result = fetch_metadata(tmpdir.path(), "http://foo.bar".to_owned(), 1, &mut log).await;

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no blockservice.toml found")
        );
    }

    #[tokio::test]
    async fn fails_for_invalid_server_url() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let url = "invalid-url".to_string();
        let result = fetch_metadata(tmpdir.path(), url, 1, std::io::sink()).await;
        assert_eq!(result.unwrap_err().to_string(), "transport error");
    }

    #[tokio::test]
    async fn forwards_server_errors() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let mut mock_server = MockRpcServer::new();
        mock_server
            .expect_get_metadata()
            .returning(move |_| Err(tonic::Status::internal("server error")));

        let server = TestServer::new(mock_server).await;

        let mut log = Vec::new();
        let result = fetch_metadata(tmpdir.path(), server.address.clone(), 1, &mut log).await;

        assert!(result.unwrap_err().to_string().contains("server error"));
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
            .expect_get_metadata()
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
            .returning({
                move |_| {
                    Ok(tonic::Response::new(proto_rpc::Metadata {
                        rules_update_heights: None,
                        corrections: None,
                    }))
                }
            });

        let server = TestServer::new(mock_server).await;
        let mut buf = Vec::new();
        let result = fetch_metadata(tmpdir.path(), server.address.clone(), 1, &mut buf).await;
        assert!(result.is_ok(), "fetch metadata should succeed");
    }
}
