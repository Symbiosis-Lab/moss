//! Local redirect fixtures: consume the complete upload before redirecting it.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread::JoinHandle;

pub(crate) struct RedirectServer {
    pub url: String,
    stop: Arc<AtomicBool>,
    first: JoinHandle<String>,
    second: JoinHandle<usize>,
}

impl RedirectServer {
    pub fn new(status: u16) -> Self {
        let first = TcpListener::bind("127.0.0.1:0").unwrap();
        let second = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/upload", first.local_addr().unwrap());
        let target = format!("http://{}/target", second.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        second.set_nonblocking(true).unwrap();
        let second = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let mut count = 0;
            loop {
                match second.accept() {
                    Ok((mut s, _)) => {
                        s.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
                        read_request(&s);
                        count += 1;
                        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").unwrap();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if stopped.load(Ordering::SeqCst) || std::time::Instant::now() >= deadline { break; }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(e) => panic!("second listener: {e}"),
                }
            }
            count
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
        self.stop.store(true, Ordering::SeqCst);
        (self.first.join().unwrap(), self.second.join().unwrap())
    }
}

fn read_request(stream: &std::net::TcpStream) -> String {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let mut len = 0;
    loop {
        line.clear();
        reader.read_line(&mut line).unwrap();
        if line.trim().is_empty() { break; }
        if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") { len = n.trim().parse().unwrap(); }
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body).unwrap();
    String::from_utf8(body).unwrap()
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
