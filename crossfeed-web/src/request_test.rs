use http::Uri;

use crate::Request;

#[test]
fn builds_request() {
    let uri: Uri = "http://example.com/".parse().unwrap();
    let request = Request::builder(uri.clone())
        .method(http::Method::POST)
        .body(b"hello".to_vec())
        .build();

    assert_eq!(request.uri, uri);
    assert_eq!(request.method, http::Method::POST);
    assert_eq!(request.body, b"hello".to_vec());
}
