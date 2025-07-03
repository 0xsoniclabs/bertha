use std::path::Path;

use crate::{
    blockdb::{self, BLOCK_DB_NAME},
    rpc_server::RpcServer,
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

    use crate::cmd::{ChangeWorkingDir, init, start};

    #[tokio::test]
    async fn start_fails_if_db_does_not_exist() {
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
    async fn start_fails_with_invalid_port() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        // Reserved port
        let res = start(80).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("transport error"));
    }
}
