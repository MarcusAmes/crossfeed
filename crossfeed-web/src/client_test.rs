use std::net::SocketAddr;

use http::Uri;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_stream::StreamExt;

use crate::{BatchRequest, Client, ClientConfig, Request};

async fn start_test_server(expected: usize) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        for _ in 0..expected {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK";
                let _ = stream.write_all(response).await;
            }
        }
    });

    addr
}

#[tokio::test]
async fn request_returns_response() {
    let addr = start_test_server(1).await;
    let client = Client::new(ClientConfig::default());
    let uri: Uri = format!("http://{}/", addr).parse().unwrap();
    let request = Request::builder(uri).build();

    let response = client.request(request).await.unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"OK".to_vec());
}

#[tokio::test]
async fn batch_returns_out_of_order() {
    let addr = start_test_server(2).await;
    let client = Client::new(ClientConfig::default());
    let uri: Uri = format!("http://{}/", addr).parse().unwrap();
    let requests = vec![
        BatchRequest {
            request: Request::builder(uri.clone()).build(),
        },
        BatchRequest {
            request: Request::builder(uri).build(),
        },
    ];

    let mut stream = client.request_batch(requests).await;
    let mut count = 0;
    while let Some(result) = stream.next().await {
        assert!(result.is_ok());
        count += 1;
    }
    assert_eq!(count, 2);
}
