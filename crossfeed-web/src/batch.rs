use std::pin::Pin;

use tokio::sync::mpsc;
use tokio_stream::{Stream, wrappers::ReceiverStream};

use crate::{Client, Request, Response};

#[derive(Debug, Clone)]
pub struct BatchRequest {
    pub request: Request,
}

#[derive(Debug, Clone)]
pub struct BatchItem {
    pub id: usize,
    pub request: Request,
}

#[derive(Debug, Clone)]
pub struct BatchResponse {
    pub id: usize,
    pub response: Response,
}

pub type BatchResultStream = Pin<Box<dyn Stream<Item = Result<BatchResponse, String>> + Send>>;

impl Client {
    pub async fn request_batch(&self, requests: Vec<BatchRequest>) -> BatchResultStream {
        let (sender, receiver) = mpsc::channel(requests.len());
        for (index, item) in requests.into_iter().enumerate() {
            let client = self.clone();
            let sender = sender.clone();
            tokio::spawn(async move {
                let result = client
                    .request(item.request)
                    .await
                    .map(|response| BatchResponse {
                        id: index,
                        response,
                    });
                let _ = sender.send(result).await;
            });
        }
        drop(sender);
        Box::pin(ReceiverStream::new(receiver))
    }
}
