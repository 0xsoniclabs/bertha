use std::path::Path;

use crate::{app_dir::open_app_dir, grpc::RpcServer};

pub async fn start(listener: tokio::net::TcpListener) -> Result<(), Box<dyn std::error::Error>> {
    let app_dir = Path::new("./").canonicalize()?;
    let db = open_app_dir(app_dir, true)?;

    let server = RpcServer::new(db);
    server.serve(listener).await
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tokio_stream::StreamExt;

    use crate::{
        app_dir::BLOCK_DB_NAME,
        cmd::{ChangeWorkingDir, init, start},
        db::{BlockDb, RocksBlockDb},
        grpc::RpcClient,
    };

    #[tokio::test]
    async fn starts_server_successfully() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        {
            let db_path = Path::new("./").join(BLOCK_DB_NAME).canonicalize().unwrap();
            let db = RocksBlockDb::open(db_path).unwrap();
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
        let mut res = client.get_block_range(1, 1, 1).await.unwrap();
        assert_eq!(
            res.next()
                .await
                .expect("stream should not be empty")
                .expect("not an error")
                .data,
            vec![1, 2, 3]
        );
        job.abort(); // Stop the server
    }

    #[tokio::test]
    async fn fails_if_app_dir_is_not_initialized() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());

        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let result = start(listener).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no database found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }
}
