use super::download::{do_http_request, do_http_request_full, FetchMethod, HttpMethod, Redirect};
use super::redirect_tests::{reply, Server};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn headers() -> HashMap<String, String> {
    HashMap::from([
        ("aCcEpT".into(), "application/json".into()),
        ("ACCEPT-LANGUAGE".into(), "en".into()),
        ("Content-TYPE".into(), "text/plain".into()),
        ("User-AGENT".into(), "fixture".into()),
        ("Authorization".into(), "Bearer fixture".into()),
        ("Cookie".into(), "session=fixture".into()),
        ("X-Access-Token".into(), "secret".into()),
        ("X-Other".into(), "private".into()),
    ])
}

#[tokio::test]
async fn both_adapters_strip_credentials_on_cross_origin_hops() {
    for adapter in ["get", "json", "bytes", "fetch"] {
        let second = Server::new(|_, _| reply(200, None, "ok"));
        let target = format!("{}/cdn", second.url.replace("127.0.0.1", "localhost"));
        let first = Server::new(move |_, _| reply(302, Some(&target), ""));
        let url = first.url.clone();
        let expected_type = match adapter {
            "fetch" => {
                let result = do_http_request_full(url, FetchMethod::Get, Some(headers()), Some(2000), Redirect::Follow).await.unwrap();
                assert_eq!(result.status, 200);
                "text/plain"
            }
            _ => {
                let (method, content_type) = match adapter {
                    "json" => (HttpMethod::Post { body: "payload".into() }, "application/json"),
                    "bytes" => (HttpMethod::PostBytes { body: b"payload".to_vec(), content_type: "multipart/form-data; boundary=fixture".into() }, "multipart/form-data; boundary=fixture"),
                    _ => (HttpMethod::Get, "text/plain"),
                };
                assert_eq!(do_http_request(url, method, Some(headers()), Some(2000), "fixture").await.unwrap().status, 200);
                content_type
            }
        };
        let initial = first.finish();
        let next = second.finish();
        assert_eq!(initial[0].headers["authorization"], "Bearer fixture");
        assert_eq!(initial[0].headers["content-type"], expected_type);
        assert_eq!(next.len(), 1);
        for name in ["authorization", "cookie", "x-access-token", "x-other"] {
            assert!(!next[0].headers.contains_key(name), "{adapter} leaked {name}: {:?}", next[0]);
        }
        assert_eq!(next[0].headers["accept"], "application/json");
        assert_eq!(next[0].headers["accept-language"], "en");
        assert_eq!(next[0].headers["user-agent"], "fixture");
        let dropped_body = matches!(adapter, "json" | "bytes");
        assert_eq!(next[0].headers.get("content-type").map(String::as_str), (!dropped_body).then_some(expected_type), "{adapter}");
        assert!(next[0].body.is_empty());
    }
}

#[tokio::test]
async fn same_origin_keeps_headers_and_resolves_relative_locations() {
    for helper in [false, true] {
        let server = Server::new(|i, _| if i == 0 { reply(302, Some("../done"), "") } else { reply(200, None, "ok") });
        let url = format!("{}/nested/start", server.url);
        if helper { do_http_request(url, HttpMethod::Get, Some(headers()), Some(2000), "fixture").await.unwrap(); }
        else { do_http_request_full(url, FetchMethod::Get, Some(headers()), Some(2000), Redirect::Follow).await.unwrap(); }
        let requests = server.finish();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].line, "GET /done HTTP/1.1");
        assert_eq!(requests[1].headers["authorization"], "Bearer fixture");
        assert_eq!(requests[1].headers["cookie"], "session=fixture");
        assert_eq!(requests[1].headers["x-access-token"], "secret");
    }
}

#[tokio::test]
async fn redirect_methods_do_not_start_forwarding_upload_bodies() {
    for status in [301, 302, 303, 307, 308] {
        for verb in ["GET", "HEAD", "POST", "PUT", "DELETE"] {
            let second = Server::new(|_, _| reply(200, None, "ok"));
            let target = second.url.clone();
            let first = Server::new(move |_, _| reply(status, Some(&target), ""));
            let method = match verb {
                "GET" => FetchMethod::Get, "HEAD" => FetchMethod::Head, "DELETE" => FetchMethod::Delete,
                "POST" => FetchMethod::Post { body: "payload".into() }, _ => FetchMethod::Put { body: "payload".into() },
            };
            let mut h = headers();
            if matches!(verb, "POST" | "PUT") { h.insert("Transfer-Encoding".into(), "chunked".into()); }
            let result = do_http_request_full(first.url.clone(), method, Some(h), Some(2000), Redirect::Follow).await.unwrap();
            let initial = first.finish();
            let next = second.finish();
            assert_eq!(initial[0].headers["content-type"], "text/plain", "fetch permits overrides");
            assert_eq!(initial[0].body, if matches!(verb, "POST" | "PUT") { "payload" } else { "" });
            let follows = status < 307 || matches!(verb, "GET" | "HEAD");
            assert_eq!(result.status, if follows { 200 } else { status });
            assert_eq!(next.len(), usize::from(follows), "{verb} {status}");
            if follows {
                assert_eq!(next[0].line, format!("{} / HTTP/1.1", if verb == "HEAD" { "HEAD" } else { "GET" }));
                assert!(next[0].body.is_empty());
                for name in ["content-length", "transfer-encoding", "authorization", "x-access-token"] { assert!(!next[0].headers.contains_key(name)); }
                assert_eq!(next[0].headers.contains_key("content-type"), !matches!(verb, "POST" | "PUT"), "{verb} {status}");
            }
        }
        for bytes in [false, true] {
            let second = Server::new(|_, _| reply(200, None, "ok"));
            let target = second.url.clone();
            let first = Server::new(move |_, _| reply(status, Some(&target), ""));
            let method = if bytes { HttpMethod::PostBytes { body: b"payload".to_vec(), content_type: "multipart/form-data; boundary=fixture".into() } } else { HttpMethod::Post { body: "payload".into() } };
            let result = do_http_request(first.url.clone(), method, Some(headers()), Some(2000), "fixture").await.unwrap();
            assert_eq!(result.status, if status < 307 { 200 } else { status });
            assert_eq!(first.finish()[0].body, "payload");
            assert_eq!(second.finish().len(), usize::from(status < 307));
        }
    }
}

#[tokio::test]
async fn returning_to_an_origin_or_url_userinfo_cannot_restore_credentials() {
    for helper in [false, true] {
        let back = Arc::new(Mutex::new(String::new()));
        let redirect_back = back.clone();
        let second = Server::new(move |_, _| reply(302, Some(&redirect_back.lock().unwrap()), ""));
        let target = second.url.replace("http://", "http://injected:secret@");
        let first = Server::new(move |i, _| if i == 0 { reply(302, Some(&target), "") } else { reply(200, None, "ok") });
        let url = first.url.replace("http://", "http://original:password@");
        *back.lock().unwrap() = format!("{url}/back");
        let mut h = headers();
        h.remove("Authorization");
        if helper { do_http_request(url, HttpMethod::Get, Some(h), Some(2000), "fixture").await.unwrap(); }
        else { do_http_request_full(url, FetchMethod::Get, Some(h), Some(2000), Redirect::Follow).await.unwrap(); }
        let first = first.finish();
        let second = second.finish();
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].headers["authorization"], "Basic b3JpZ2luYWw6cGFzc3dvcmQ=");
        for request in [&second[0], &first[1]] {
            assert!(!request.headers.contains_key("authorization"));
            assert!(!request.headers.contains_key("x-access-token"));
            assert!(!request.headers.contains_key("cookie"));
        }
    }
}

#[tokio::test]
async fn redirect_limits_locations_and_opt_outs_match_the_contract() {
    for status in [300, 301, 304, 307, 398, 399] {
        for policy in [Redirect::Error, Redirect::Manual, Redirect::Follow] {
            let server = Server::new(move |_, _| reply(status, None, "redirect body"));
            let result = do_http_request_full(server.url.clone(), FetchMethod::Get, None, Some(2000), policy).await;
            assert_eq!(server.finish().len(), 1);
            if policy == Redirect::Error { assert!(result.err().unwrap().contains("redirect refused")); }
            else { let response = result.unwrap(); assert_eq!(response.status, status); assert_eq!(response.body, if status == 304 { b"".as_slice() } else { b"redirect body".as_slice() }); }
        }
    }
    for helper in [false, true] {
        for final_status in [200, 300, 302, 304, 307, 398, 399] {
            let server = Server::new(move |i, _| if i < 4 { reply(302, Some("/next"), "") } else { reply(final_status, None, "done") });
            let result = if helper {
                do_http_request(server.url.clone(), HttpMethod::Get, None, Some(2000), "fixture").await.map(|r| r.status)
            } else { do_http_request_full(server.url.clone(), FetchMethod::Get, None, Some(2000), Redirect::Follow).await.map(|r| r.status) };
            assert_eq!(server.finish().len(), 5);
            if (300..399).contains(&final_status) { assert!(result.unwrap_err().contains("max redirects (5)")); }
            else { assert_eq!(result.unwrap(), final_status); }
        }
        let server = Server::new(|_, _| reply(302, Some("ftp://example.org/"), ""));
        let result = if helper {
            do_http_request(server.url.clone(), HttpMethod::Get, None, Some(2000), "fixture").await.map(|r| r.status)
        } else { do_http_request_full(server.url.clone(), FetchMethod::Get, None, Some(2000), Redirect::Follow).await.map(|r| r.status) };
        assert!(result.unwrap_err().contains("scheme"));
        assert_eq!(server.finish().len(), 1);
    }
}

#[tokio::test]
async fn one_timeout_covers_the_entire_redirect_chain() {
    for helper in [false, true] {
        let server = Server::new(|i, _| {
            std::thread::sleep(Duration::from_millis(100));
            if i == 0 { reply(302, Some("/next"), "") } else { reply(200, None, "ok") }
        });
        let result = if helper {
            do_http_request(server.url.clone(), HttpMethod::Get, None, Some(170), "fixture").await.map(|r| r.status)
        } else { do_http_request_full(server.url.clone(), FetchMethod::Get, None, Some(170), Redirect::Follow).await.map(|r| r.status) };
        assert!(result.is_err(), "each hop must not get another 170ms");
        assert_eq!(server.finish().len(), 2);
    }
}

#[tokio::test]
async fn final_status_head_and_body_errors_keep_their_response_contract() {
    for helper in [false, true] {
        for status in [200, 500] {
            for truncated in [false, true] {
                let server = Server::new(move |_, _| format!("HTTP/1.1 {status} X\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\nok", if truncated { 10 } else { 2 }));
                let result = if helper {
                    do_http_request(server.url.clone(), HttpMethod::Get, None, Some(2000), "fixture").await.map(|r| (r.status, r.ok))
                } else { do_http_request_full(server.url.clone(), FetchMethod::Get, None, Some(2000), Redirect::Follow).await.map(|r| (r.status, r.ok)) };
                server.finish();
                if status == 200 && truncated { assert!(result.unwrap_err().contains("body")); }
                else { assert_eq!(result.unwrap(), (status, status == 200)); }
            }
            let server = Server::new(move |_, _| format!("HTTP/1.1 {status} X\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n"));
            let response = do_http_request_full(server.url.clone(), FetchMethod::Head, None, Some(2000), Redirect::Follow).await.unwrap();
            server.finish();
            assert_eq!(response.status, status);
            assert!(response.body.is_empty());
        }
    }
}
