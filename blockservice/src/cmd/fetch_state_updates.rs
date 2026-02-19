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

use std::{io::Write, path::Path};

use crate::{app_dir::open_app_dir, grpc::RpcClient};

pub async fn fetch_state_updates(
    app_dir: impl AsRef<Path>,
    url: String,
    chain_id: u64,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // To not write files into arbitrary directories, we first check that we actually
    // are in a valid application directory.
    let (cfg, _) = open_app_dir(&app_dir, true)?;

    let auth_token = cfg.get_auth_token().cloned();

    let mut client = RpcClient::try_new(url, auth_token).await?;
    let updates = client.get_state_updates(chain_id).await?;

    writeln!(
        writer,
        "Received {} state update files for chain ID {}",
        updates.updates.len(),
        chain_id
    )?;

    for update in updates.updates {
        match std::fs::File::create_new(app_dir.as_ref().join(&update.filename)) {
            Ok(mut file) => {
                file.write_all(update.data.as_bytes())?;
                writeln!(writer, "{}", update.filename)?;
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    writeln!(writer, "{} already exists - skipping", update.filename)?;
                } else {
                    return Err(e.into());
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::{
        app_dir::init_app_dir,
        grpc::{
            auth::{self, AUTHORIZATION_HEADER_NAME},
            proto_rpc,
            test_utils::{MockRpcServer, TestServer},
        },
        utils::test_dir::{Permissions, TestDir},
    };

    fn build_mock_server() -> MockRpcServer {
        let mut mock_server = MockRpcServer::new();
        mock_server.expect_get_state_updates().returning({
            move |_| {
                Ok(tonic::Response::new(proto_rpc::StateUpdates {
                    updates: vec![
                        proto_rpc::StateUpdate {
                            filename: "update1.json".to_string(),
                            data: "foo".to_string(),
                        },
                        proto_rpc::StateUpdate {
                            filename: "update2.json".to_string(),
                            data: "bar".to_string(),
                        },
                    ],
                }))
            }
        });
        mock_server
    }

    #[tokio::test]
    async fn fetches_files_and_stores_them_in_application_directory() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let server = TestServer::new(build_mock_server()).await;

        let mut log = Vec::new();
        let result = fetch_state_updates(tmpdir.path(), server.address.clone(), 7, &mut log).await;

        assert!(result.is_ok());
        let log_str = String::from_utf8(log).unwrap();
        assert!(log_str.contains("Received 2 state update files for chain ID 7"));
        assert!(log_str.contains("update1.json"));
        assert!(log_str.contains("update2.json"));

        assert!(tmpdir.path().join("update1.json").exists());
        let update1 = std::fs::read_to_string(tmpdir.path().join("update1.json")).unwrap();
        assert_eq!(update1, "foo");
        assert!(tmpdir.path().join("update2.json").exists());
        let update2 = std::fs::read_to_string(tmpdir.path().join("update2.json")).unwrap();
        assert_eq!(update2, "bar");
    }

    #[tokio::test]
    async fn skips_existing_files() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let server = TestServer::new(build_mock_server()).await;

        // Create existing file
        let existing_file = tmpdir.path().join("update1.json");
        std::fs::write(&existing_file, "existing data").unwrap();

        let mut log = Vec::new();
        let result = fetch_state_updates(tmpdir.path(), server.address.clone(), 7, &mut log).await;

        assert!(result.is_ok());
        let log_str = String::from_utf8(log).unwrap();
        assert!(log_str.contains("Received 2 state update files for chain ID 7"));
        assert!(log_str.contains("update1.json already exists - skipping"));
        assert!(log_str.contains("update2.json"));

        assert!(tmpdir.path().join("update1.json").exists());
        let update1 = std::fs::read_to_string(tmpdir.path().join("update1.json")).unwrap();
        assert_eq!(update1, "existing data"); // not overwritten
    }

    #[tokio::test]
    async fn fails_if_app_dir_is_not_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        let mut log = Vec::new();
        let result =
            fetch_state_updates(tmpdir.path(), "http://foo.bar".to_owned(), 7, &mut log).await;

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
        let result = fetch_state_updates(tmpdir.path(), url, 7, std::io::sink()).await;
        assert_eq!(result.unwrap_err().to_string(), "transport error");
    }

    #[tokio::test]
    async fn forwards_server_errors() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let mut mock_server = MockRpcServer::new();
        mock_server
            .expect_get_state_updates()
            .returning(move |_| Err(tonic::Status::internal("server error")));

        let server = TestServer::new(mock_server).await;

        let mut log = Vec::new();
        let result = fetch_state_updates(tmpdir.path(), server.address.clone(), 7, &mut log).await;

        assert!(result.unwrap_err().to_string().contains("server error"));
    }

    #[tokio::test]
    async fn forwards_io_errors() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let server = TestServer::new(build_mock_server()).await;

        // Make directory read-only
        tmpdir.set_permissions(Permissions::ReadOnly).unwrap();

        let mut log = Vec::new();
        let result = fetch_state_updates(tmpdir.path(), server.address.clone(), 7, &mut log).await;

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
    }

    #[tokio::test]
    async fn provides_auth_token_when_supplied() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (mut cfg, _db) = open_app_dir(tmpdir.path(), true).unwrap();

        let cases = vec![
            Some(auth::token_to_metadata_value("my-token").unwrap()),
            None,
        ];
        for auth_token in cases {
            cfg.set_auth_token(auth_token.clone()).unwrap();

            let mut mock_server = MockRpcServer::new();
            mock_server
                .expect_get_state_updates()
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
                        Ok(tonic::Response::new(proto_rpc::StateUpdates {
                            updates: vec![],
                        }))
                    }
                });

            let server = TestServer::new(mock_server).await;
            let mut buf = Vec::new();
            let result =
                fetch_state_updates(tmpdir.path(), server.address.clone(), 1, &mut buf).await;
            assert!(result.is_ok(), "fetch update state should succeed");
        }
    }
}
