//! What the shared raw-TCP mock (`test_mock_http_conn`) puts on the wire.
//!
//! The header it adds is invisible to a caller reading its own byte string, and
//! the failure it prevents (a client reusing a socket the mock has closed) shows
//! up as an `IncompleteMessage` in some other test, about once in a thousand
//! runs. So the contract is pinned here, at the socket, where it is not timing.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Serve `resp` through the shared helper to a bare TCP client that sends one
/// request, and return everything the client read until the server closed.
async fn wire(resp: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        crate::test_mock_http_conn(stream, resp).await;
    });
    let mut client = TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut out = String::new();
    client.read_to_string(&mut out).await.unwrap();
    out
}

#[tokio::test]
async fn a_response_silent_about_the_connection_goes_out_saying_close() {
    let got = wire(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi").await;
    assert_eq!(
        got,
        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 2\r\n\r\nhi",
        "the mock closes after one response, so it must say so, or a pooled client reuses a dead socket"
    );
}

#[tokio::test]
async fn a_response_that_already_says_close_goes_out_unchanged() {
    let sent: &'static [u8] = b"HTTP/1.1 200 OK\r\nconnection: close\r\nContent-Length: 2\r\n\r\nhi";
    let got = wire(sent).await;
    assert_eq!(got.as_bytes(), sent, "no second Connection header");
}
