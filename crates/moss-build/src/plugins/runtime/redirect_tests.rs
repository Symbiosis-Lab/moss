//! Bounded loopback HTTP fixtures shared with the fetch shim tests.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub(crate) struct CapturedRequest {
    pub line: String,
    pub headers: std::collections::HashMap<String, String>,
    pub body: String,
}

pub(crate) struct Server {
    pub url: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
}

impl Server {
    pub fn new(reply: impl Fn(usize, &CapturedRequest) -> String + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !stopped.load(Ordering::SeqCst) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // Some platforms hand the listener's non-blocking mode to the accepted socket.
                        stream.set_nonblocking(false).unwrap();
                        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                        stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
                        let request = read_request(&stream);
                        let index = { let mut all = captured.lock().unwrap(); all.push(request.clone()); all.len() - 1 };
                        let _ = stream.write_all(reply(index, &request).as_bytes());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(2)),
                    Err(e) => panic!("listener: {e}"),
                }
            }
        });
        Self { url, stop, requests, worker: Some(worker) }
    }

    pub fn finish(mut self) -> Vec<CapturedRequest> {
        self.stop.store(true, Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap();
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

pub(crate) fn reply(status: u16, location: Option<&str>, body: &str) -> String {
    let location = location.map(|l| format!("Location: {l}\r\n")).unwrap_or_default();
    format!("HTTP/1.1 {status} X\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

pub(crate) struct RedirectServer {
    pub url: String,
    first: Server,
    second: Server,
}

impl RedirectServer {
    pub fn new(status: u16) -> Self {
        let second = Server::new(|_, _| reply(200, None, "ok"));
        let target = format!("{}/target", second.url);
        let first = Server::new(move |_, _| reply(status, Some(&target), ""));
        Self { url: format!("{}/upload", first.url), first, second }
    }

    pub fn finish(self) -> (String, usize) {
        let first = self.first.finish();
        let second = self.second.finish();
        (first[0].body.clone(), second.len())
    }
}

fn read_request(stream: &std::net::TcpStream) -> CapturedRequest {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let request_line = line.trim().to_string();
    let mut headers = std::collections::HashMap::new();
    loop {
        line.clear();
        reader.read_line(&mut line).unwrap();
        if line.trim().is_empty() { break; }
        let (name, value) = line.split_once(':').unwrap();
        headers.insert(name.to_ascii_lowercase(), value.trim().to_string());
    }
    let mut body = Vec::new();
    if headers.get("transfer-encoding").is_some_and(|v| v == "chunked") {
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            let len = usize::from_str_radix(line.trim(), 16).unwrap();
            if len == 0 { break; }
            let mut chunk = vec![0; len + 2];
            reader.read_exact(&mut chunk).unwrap();
            body.extend_from_slice(&chunk[..len]);
        }
    } else {
        let len = headers.get("content-length").map(|v| v.parse().unwrap()).unwrap_or(0);
        body.resize(len, 0);
        reader.read_exact(&mut body).unwrap();
    }
    CapturedRequest { line: request_line, headers, body: String::from_utf8_lossy(&body).into_owned() }
}

#[test]
fn a_request_that_arrives_late_is_still_read_whole() {
    let server = Server::new(|_, _| reply(200, None, "ok"));
    let mut client = std::net::TcpStream::connect(server.url.trim_start_matches("http://")).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    client.write_all(b"GET /late HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    assert_eq!(server.finish()[0].line, "GET /late HTTP/1.1");
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
