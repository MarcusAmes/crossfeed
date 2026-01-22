use http::{HeaderMap, Method, Uri};

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub uri: Uri,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub http_version: String,
}

impl Request {
    pub fn builder(uri: Uri) -> RequestBuilder {
        RequestBuilder::new(uri)
    }
}

#[derive(Debug, Clone)]
pub struct RequestBuilder {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Vec<u8>,
    http_version: String,
}

impl RequestBuilder {
    pub fn new(uri: Uri) -> Self {
        Self {
            method: Method::GET,
            uri,
            headers: HeaderMap::new(),
            body: Vec::new(),
            http_version: "HTTP/1.1".to_string(),
        }
    }

    pub fn method(mut self, method: Method) -> Self {
        self.method = method;
        self
    }

    pub fn header(mut self, name: http::header::HeaderName, value: http::HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    pub fn http_version(mut self, http_version: impl Into<String>) -> Self {
        self.http_version = http_version.into();
        self
    }

    pub fn build(self) -> Request {
        Request {
            method: self.method,
            uri: self.uri,
            headers: self.headers,
            body: self.body,
            http_version: self.http_version,
        }
    }
}

pub type RequestMethod = Method;
