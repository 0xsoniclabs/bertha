use std::path::Path;

use crate::{grpc::RpcServer, workspace::open_workspace};

pub async fn start(listener: tokio::net::TcpListener) -> Result<(), Box<dyn std::error::Error>> {
    let workspace_path = Path::new("./").canonicalize()?;
    let (_cfg, db) = open_workspace(workspace_path, true)?;

    let server = RpcServer::new(db);
    server.serve(listener).await
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{
        cmd::{ChangeWorkingDir, init, start},
        db::{BlockDb, RocksBlockDb},
        grpc::RpcClient,
        workspace::BLOCK_DB_NAME,
    };

    #[tokio::test]
    async fn starts_server_successfully() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        {
            let db_path = Path::new("./").join(BLOCK_DB_NAME).canonicalize().unwrap();
            let mut db = RocksBlockDb::open(db_path).unwrap();
            db.put_raw(1, 1, vec![1, 2, 3].as_slice()).unwrap();
        }

        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let job = tokio::spawn(async move {
            start(listener).await.unwrap();
        });

        let client = RpcClient::try_new(format!("http://{addr}").parse().unwrap()).await;
        assert!(client.is_ok());
        let mut client = client.unwrap();
        let res = client.get_block(1, 1).await.expect("Block should be found");
        assert_eq!(res.data, vec![1, 2, 3]);
        job.abort(); // Stop the server
    }

    #[tokio::test]
    async fn fails_if_workspace_does_not_exist() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());

        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let result = start(listener).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }
}
