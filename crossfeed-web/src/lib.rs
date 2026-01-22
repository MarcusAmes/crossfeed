mod batch;
mod client;
#[cfg(test)]
mod client_test;
mod download;
#[cfg(test)]
mod download_test;
mod rate_limit;
#[cfg(test)]
mod rate_limit_test;
mod request;
#[cfg(test)]
mod request_test;
mod response;
mod retry;
#[cfg(test)]
mod retry_test;

pub use batch::{BatchItem, BatchRequest, BatchResponse, BatchResultStream};
pub use client::{CancelToken, Client, ClientConfig, ProxyConfig, ProxyKind, RequestError};
pub use download::{DownloadResult, DownloadTarget};
pub use rate_limit::RateLimiter;
pub use request::{Request, RequestBuilder, RequestMethod};
pub use response::Response;
pub use retry::{RetryPolicy, RetryableError};
