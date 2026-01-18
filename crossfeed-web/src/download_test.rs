use std::net::SocketAddr;
use std::path::PathBuf;

use http::Uri;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::{Client, ClientConfig, DownloadTarget, Request};

async fn start_test_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
            let _ = stream.write_all(response).await;
        }
    });

    addr
}

#[tokio::test]
async fn download_writes_file() {
    let addr = start_test_server().await;
    let client = Client::new(ClientConfig::default());
    let uri: Uri = format!("http://{}/", addr).parse().unwrap();
    let request = Request::builder(uri).build();
    let target = DownloadTarget {
        path: PathBuf::from("/tmp/crossfeed-download-test"),
    };

    let result = client.download(request, target.clone()).await;
    assert!(result.is_ok());

    let data = tokio::fs::read(target.path).await.unwrap();
    assert_eq!(data, b"hello".to_vec());
}
