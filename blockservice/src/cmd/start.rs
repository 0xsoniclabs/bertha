use std::path::Path;

use crate::{
    blockdb::{self, BLOCK_DB_NAME},
    grpc::RpcServer,
};

pub async fn start(listening_port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = Path::new("./").join(BLOCK_DB_NAME).canonicalize()?;
    let db = blockdb::RocksBlockDb::open_for_reading(db_path)?;
    let server = RpcServer::new(db);
    server.serve(listening_port).await
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{
        blockdb::{BLOCK_DB_NAME, BlockDb, RocksBlockDb},
        cmd::{ChangeWorkingDir, init, start},
        grpc::{RpcClient, test_utils::SERVER_STARTUP_TIMER},
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

        let job = tokio::spawn(async {
            let _ = start(8080).await.unwrap();
        });

        // Wait for the server to start
        tokio::time::sleep(tokio::time::Duration::from_millis(SERVER_STARTUP_TIMER)).await;

        let client = RpcClient::try_new("http://[::1]:8080".parse().unwrap()).await;
        assert!(client.is_ok());
        let mut client = client.unwrap();
        let res = client.get_block(1, 1).await.expect("Block should be found");
        assert_eq!(res.data, vec![1, 2, 3]);
        job.abort(); // Stop the server
    }

    #[tokio::test]
    async fn fails_if_db_does_not_exist() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        let res = start(1).await;
        assert!(res.is_err());
        assert!(
            res.unwrap_err()
                .to_string()
                .contains("No such file or directory")
        );
    }

    #[tokio::test]
    async fn fails_with_invalid_port() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        // Reserved port
        let res = start(80).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("transport error"));
    }
}
