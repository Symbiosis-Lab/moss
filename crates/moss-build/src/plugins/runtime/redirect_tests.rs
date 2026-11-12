//! Local redirect fixtures: consume the complete upload before redirecting it.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread::JoinHandle;

pub(crate) struct RedirectServer {
    pub url: String,
    stop: Arc<AtomicBool>,
    first: JoinHandle<CapturedRequest>,
    second: JoinHandle<Vec<CapturedRequest>>,
}

impl RedirectServer {
    pub fn new(status: u16) -> Self {
        Self::with_target_host(status, false)
    }

    fn with_target_host(status: u16, different_spelling: bool) -> Self {
        let first = TcpListener::bind("127.0.0.1:0").unwrap();
        let second = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/upload", first.local_addr().unwrap());
        let address = second.local_addr().unwrap();
        let target = if different_spelling { format!("http://localhost:{}/target", address.port()) } else { format!("http://{address}/target") };
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        second.set_nonblocking(true).unwrap();
        let second = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let mut requests = Vec::new();
            loop {
                match second.accept() {
                    Ok((mut s, _)) => {
                        s.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
                        requests.push(read_request(&s));
                        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").unwrap();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if stopped.load(Ordering::SeqCst) || std::time::Instant::now() >= deadline { break; }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(e) => panic!("second listener: {e}"),
                }
            }
            requests
        });
        let first = std::thread::spawn(move || {
            let (mut s, _) = first.accept().unwrap();
            s.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
            let body = read_request(&s);
            write!(s, "HTTP/1.1 {status} Redirect\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            body
        });
        Self { url, stop, first, second }
    }

    pub fn finish(self) -> (String, usize) {
        let (first, second) = self.finish_requests();
        (first.body, second.len())
    }

    fn finish_requests(self) -> (CapturedRequest, Vec<CapturedRequest>) {
        self.stop.store(true, Ordering::SeqCst);
        (self.first.join().unwrap(), self.second.join().unwrap())
    }
}

#[derive(Debug)]
struct CapturedRequest {
    line: String,
    headers: std::collections::HashMap<String, String>,
    body: String,
}

fn read_request(stream: &std::net::TcpStream) -> CapturedRequest {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let request_line = line.trim().to_string();
    let mut headers = std::collections::HashMap::new();
    let mut len = 0;
    loop {
        line.clear();
        reader.read_line(&mut line).unwrap();
        if line.trim().is_empty() { break; }
        let (name, value) = line.split_once(':').unwrap();
        headers.insert(name.to_ascii_lowercase(), value.trim().to_string());
        if name.eq_ignore_ascii_case("content-length") { len = value.trim().parse().unwrap(); }
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body).unwrap();
    CapturedRequest { line: request_line, headers, body: String::from_utf8(body).unwrap() }
}

#[tokio::test]
async fn redirect_policy_stops_uploads_before_the_second_port() {
    use super::download::{do_http_request_full, FetchMethod, Redirect};
    for (policy, expected) in [(Redirect::Error, None), (Redirect::Manual, Some(302)), (Redirect::Follow, Some(200))] {
        let server = RedirectServer::new(302);
        let result = do_http_request_full(server.url.clone(), FetchMethod::Post { body: "payload".into() }, None, Some(2000), policy).await;
        let (body, requests) = server.finish();
        assert_eq!(body, "payload");
        assert_eq!(requests, usize::from(policy == Redirect::Follow));
        match expected {
            Some(status) => assert_eq!(result.unwrap().status, status),
            None => assert!(result.err().unwrap().contains("redirect refused")),
        }
    }
    for status in [300, 301, 303, 304, 307, 308, 399] {
        let server = RedirectServer::new(status);
        let result = do_http_request_full(server.url.clone(), FetchMethod::Post { body: "payload".into() }, None, Some(2000), Redirect::Error).await;
        let (_, requests) = server.finish();
        assert_eq!(requests, 0);
        assert!(result.err().unwrap().contains("redirect refused"), "{status}");
    }
}


#[tokio::test]
async fn followed_redirects_strip_standard_credentials_but_forward_custom_headers() {
    use super::download::{do_http_request_full, FetchMethod, Redirect};
    for different_spelling in [false, true] {
        for policy in [Redirect::Follow, Redirect::Error] {
            let server = RedirectServer::with_target_host(302, different_spelling);
            let headers = std::collections::HashMap::from([
                ("Authorization".into(), "Bearer fixture".into()),
                ("Cookie".into(), "session=fixture".into()),
                ("X-Api-Key".into(), "fixture-key".into()),
            ]);
            let result = do_http_request_full(server.url.clone(), FetchMethod::Post { body: "payload".into() }, Some(headers), Some(2000), policy).await;
            let (first, next) = server.finish_requests();
            assert_eq!(first.headers["authorization"], "Bearer fixture");
            assert_eq!(first.headers["cookie"], "session=fixture");
            assert_eq!(first.headers["content-length"], "7");
            assert_eq!(first.body, "payload");
            if policy == Redirect::Error {
                assert!(result.err().unwrap().contains("redirect refused"));
                assert!(next.is_empty());
            } else {
                assert_eq!(result.unwrap().status, 200);
                assert_eq!(next.len(), 1);
                assert_eq!(next[0].line, "GET /target HTTP/1.1");
                assert!(!next[0].headers.contains_key("authorization"));
                assert!(!next[0].headers.contains_key("cookie"));
                assert!(!next[0].headers.contains_key("content-length"));
                assert_eq!(next[0].headers["x-api-key"], "fixture-key");
                assert!(next[0].body.is_empty());
            }
        }
    }
}
