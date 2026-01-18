use std::path::PathBuf;

use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::{Client, Request, Response};

#[derive(Debug, Clone)]
pub struct DownloadTarget {
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct DownloadResult {
    pub response: Response,
    pub bytes_written: usize,
}

impl Client {
    pub async fn download(&self, request: Request, target: DownloadTarget) -> Result<DownloadResult, String> {
        let response = self.request(request).await?;
        let bytes_written = response.body.len();
        let mut file = File::create(&target.path).await.map_err(|err| err.to_string())?;
        file.write_all(&response.body).await.map_err(|err| err.to_string())?;
        Ok(DownloadResult {
            response,
            bytes_written,
        })
    }
}
