//! `moss domain link` / `status` against a local mock of the hosting service.
//! DNS never leaves the process: lookups come from a table.

use super::*;
use crate::deploy::MOSSPUB_VPS_IP as IP;
use crate::identity::Identity;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type Seen = Arc<Mutex<Vec<String>>>;

/// Answers each `"METHOD /path"` with its canned `(status, body)` and records
/// the request lines in arrival order. A key listed twice answers its requests
/// in order, then repeats the last. Unlisted routes answer 404.
async fn mock(routes: Vec<(&'static str, u16, &'static str)>) -> (MossSetaClient, Seen) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Seen = Arc::default();
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let routes = routes.clone();
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = stream.read(&mut chunk).await.unwrap_or(0);
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    let Some(head_end) = text.find("\r\n\r\n") else {
                        if n == 0 { return; }
                        continue;
                    };
                    let want = text[..head_end]
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                        .unwrap_or(0);
                    if buf.len() >= head_end + 4 + want || n == 0 {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&buf).to_string();
                let mut parts = text.lines().next().unwrap_or_default().split(' ');
                let key = format!("{} {}", parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
                let nth = {
                    let mut l = log.lock().unwrap();
                    let nth = l.iter().filter(|k| **k == key).count();
                    l.push(key.clone());
                    nth
                };
                let matching: Vec<_> = routes.iter().filter(|(k, _, _)| *k == key).collect();
                let (code, body) = matching
                    .get(nth)
                    .or(matching.last())
                    .map(|(_, c, b)| (*c, *b))
                    .unwrap_or((404, r#"{"error":"no route"}"#));
                let resp = format!(
                    "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                    code, body.len(), body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    let id = Identity::generate().unwrap();
    (MossSetaClient::with_identity_and_url(&id, &format!("http://{}", addr)), seen)
}

fn count(seen: &Seen, key: &str) -> usize {
    seen.lock().unwrap().iter().filter(|k| *k == key).count()
}

fn folder() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join(".moss")).unwrap();
    d
}

type Routes = Vec<(&'static str, u16, &'static str)>;

const LINK: &str = "POST /api/sites/s1/domains";
const OWNED: &str = "GET /api/domains/owned";
const CDN_GET: &str = "GET /api/domains/example.com/cdn";
const CDN_POST: &str = "POST /api/domains/example.com/cdn";
const LINKED: &str = r#"{"domain":"example.com","siteId":"s1"}"#;
const OWNED_EXTERNAL: &str = r#"{"domains":[{"host":"example.com","source":"external","assigned_to":{"id":"s1"}}]}"#;
const OWNED_MOSS: &str = r#"{"domains":[{"host":"example.com","source":"moss","assigned_to":{"id":"s1"}}]}"#;
const NO_CDN: &str = r#"{"cdn_status":"disabled","records":[]}"#;
const PENDING: &str = r#"{"cdn_status":"pending_dcv","records":[
 {"type":"CNAME","subdomain":"www","value":"sites.mosspub.com"},
 {"type":"CNAME","subdomain":"_acme-challenge.www","value":"www.example.com.dcv.example.net"},
 {"type":"TXT","subdomain":"_cf-custom-hostname.www","value":"owner-token-123"}]}"#;
const VALIDATING: &str = r#"{"cdn_status":"validating","records":[
 {"type":"CNAME","subdomain":"www","value":"sites.mosspub.com"},
 {"type":"CNAME","subdomain":"_acme-challenge.www","value":"www.example.com.dcv.example.net"},
 {"type":"TXT","subdomain":"_cf-custom-hostname.www","value":"owner-token-123"}]}"#;
const ACTIVE: &str = r#"{"cdn_status":"active","records":[
 {"type":"CNAME","subdomain":"www","value":"sites.mosspub.com"},
 {"type":"CNAME","subdomain":"_acme-challenge.www","value":"www.example.com.dcv.example.net"},
 {"type":"TXT","subdomain":"_cf-custom-hostname.www","value":"owner-token-123"}]}"#;
const DISABLED_WITH_RECORDS: &str = r#"{"cdn_status":"disabled","records":[
 {"type":"CNAME","subdomain":"www","value":"sites.mosspub.com"}]}"#;
const FAILED: &str = r#"{"cdn_status":"failed","records":[
 {"type":"CNAME","subdomain":"www","value":"sites.mosspub.com"}]}"#;
const DNS_OK: &str = r#"{"dns_ok":true,"cert_ok":true,"last_checked":1.0,"first_configured_at":null}"#;
const DNS_PENDING: &str = r#"{"dns_ok":false,"cert_ok":false,"last_checked":1.0,"first_configured_at":null}"#;

struct Table(HashMap<&'static str, Vec<&'static str>>);
impl records::Lookup for Table {
    fn addrs(&self, host: &str) -> Vec<String> {
        self.0.get(host).map(|v| v.iter().map(|s| s.to_string()).collect()).unwrap_or_default()
    }
}

fn table(rows: &[(&'static str, &[&'static str])]) -> Table {
    Table(rows.iter().map(|(k, v)| (*k, v.to_vec())).collect())
}

/// Every record in place: the apex at the server address, www at the edge.
fn all_in_place() -> Table {
    table(&[("example.com", &[IP]), ("sites.mosspub.com", &["104.0.0.1"]), ("www.example.com", &["104.0.0.1"])])
}

async fn link_with(client: &MossSetaClient, input: &str, json: bool) -> Outcome {
    let dir = folder();
    let target = LinkTarget { client, config_folder: &dir.path().to_string_lossy(), shown_folder: "~/Sites/blog", site_id: "s1" };
    link_flow(&target, input, json).await
}

async fn link(routes: Routes, input: &str, json: bool) -> (Outcome, Seen) {
    let (client, seen) = mock(routes).await;
    (link_with(&client, input, json).await, seen)
}

fn external_routes() -> Routes {
    vec![(LINK, 201, LINKED), (OWNED, 200, OWNED_EXTERNAL), (CDN_GET, 200, NO_CDN), (CDN_POST, 200, PENDING)]
}

#[tokio::test]
async fn external_link_prints_the_records_and_links_both_hosts() {
    let (o, seen) = link(external_routes(), "www.Example.com", false).await;
    assert_eq!(o.code, 0, "{}", o.err);
    for row in [
        "A      @".to_string(),
        IP.to_string(),
        "CNAME  www".to_string(),
        "sites.mosspub.com".to_string(),
        "_acme-challenge.www".to_string(),
        "TXT".to_string(),
        "owner-token-123".to_string(),
        "MX".to_string(),
        "up to an hour".to_string(),
        "moss domain status ~/Sites/blog example.com".to_string(),
    ] {
        assert!(o.out.contains(&row), "missing {:?} in:\n{}", row, o.out);
    }
    assert_eq!(count(&seen, LINK), 2);
    assert_eq!(count(&seen, CDN_POST), 1);
}

#[tokio::test]
async fn moss_bought_domain_prints_no_records() {
    let routes = vec![(LINK, 201, LINKED), (OWNED, 200, OWNED_MOSS), (CDN_GET, 200, ACTIVE)];
    let (o, seen) = link(routes, "example.com", false).await;
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(o.out.contains("moss manages its DNS"), "{}", o.out);
    assert!(!o.out.contains("CNAME"), "{}", o.out);
    assert_eq!(count(&seen, LINK), 1);
    assert_eq!(count(&seen, CDN_POST), 0);
}

#[tokio::test]
async fn relinking_reuses_the_records_instead_of_spending_quota() {
    let routes = vec![(LINK, 201, LINKED), (OWNED, 200, OWNED_EXTERNAL), (CDN_GET, 200, PENDING)];
    let (o, seen) = link(routes, "example.com", false).await;
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(o.out.contains("owner-token-123"));
    assert_eq!(count(&seen, CDN_POST), 0);
}

#[tokio::test]
async fn a_disabled_or_failed_hostname_is_started_again() {
    for current in [NO_CDN, DISABLED_WITH_RECORDS, FAILED] {
        let routes = vec![(LINK, 201, LINKED), (OWNED, 200, OWNED_EXTERNAL), (CDN_GET, 200, current), (CDN_POST, 200, PENDING)];
        let (o, seen) = link(routes, "example.com", false).await;
        assert_eq!(o.code, 0, "{}", o.err);
        assert_eq!(count(&seen, CDN_POST), 1, "{}", current);
    }
}

#[tokio::test]
async fn link_json_is_one_stable_object() {
    let (o, _) = link(external_routes(), "example.com", true).await;
    let v: serde_json::Value = serde_json::from_str(&o.out).unwrap();
    assert_eq!(v["domain"], "example.com");
    assert_eq!(v["source"], "external");
    assert_eq!(v["canonical_host"], "www.example.com");
    assert_eq!(v["linked"], serde_json::json!(["example.com", "www.example.com"]));
    assert_eq!(v["records"][0], serde_json::json!({"type":"A","name":"@","value":IP}));
    assert_eq!(v["records"].as_array().unwrap().len(), 4);
    assert_eq!(v["next"], "moss domain status ~/Sites/blog example.com");
}

#[tokio::test]
async fn only_a_registered_domain_is_accepted() {
    let (o, seen) = link(external_routes(), "blog.example.com", false).await;
    assert_eq!(o.code, 1);
    assert!(o.err.contains("only a registered domain"), "{}", o.err);
    assert!(seen.lock().unwrap().is_empty(), "nothing may be sent for a refused name");
}

#[tokio::test]
async fn a_service_error_on_link_exits_2_and_says_rerunning_is_safe() {
    let (o, seen) = link(vec![(LINK, 500, r#"{"error":"boom"}"#)], "example.com", false).await;
    assert_eq!(o.code, 2);
    assert!(o.err.contains("error:") && o.err.contains("Re-running the same command is safe"), "{}", o.err);
    assert_eq!(seen.lock().unwrap().len(), 1, "nothing may follow a failed link");
}

#[tokio::test]
async fn with_json_an_error_is_an_object_on_stdout() {
    let (o, _) = link(vec![(LINK, 500, r#"{"error":"boom"}"#)], "example.com", true).await;
    assert_eq!(o.code, 2);
    assert!(o.err.is_empty());
    let v: serde_json::Value = serde_json::from_str(&o.out).unwrap();
    assert_eq!(v["exit"], 2);
    assert!(v["error"].as_str().unwrap().contains("boom"));
}

#[tokio::test]
async fn rerunning_after_a_failed_www_link_finishes_the_job_without_a_second_start() {
    let routes = vec![
        (LINK, 201, LINKED),
        (LINK, 500, r#"{"error":"boom"}"#),
        (LINK, 200, LINKED),
        (LINK, 200, LINKED),
        (OWNED, 200, OWNED_EXTERNAL),
        (CDN_GET, 200, NO_CDN),
        (CDN_GET, 200, PENDING),
        (CDN_POST, 200, PENDING),
    ];
    let (client, seen) = mock(routes).await;
    let first = link_with(&client, "example.com", false).await;
    assert_eq!(first.code, 2);
    assert!(first.err.contains("Re-running the same command is safe"), "{}", first.err);
    let second = link_with(&client, "example.com", false).await;
    assert_eq!(second.code, 0, "{}", second.err);
    assert!(second.out.contains("owner-token-123"), "{}", second.out);
    assert_eq!(count(&seen, CDN_POST), 1, "the second run reuses the first run's records");
}

async fn status_with(routes: Routes, t: Table, json: bool) -> (Outcome, Seen) {
    let (client, seen) = mock(routes).await;
    let dir = folder();
    (status_flow(&client, dir.path().to_str().unwrap(), "example.com", json, &t).await, seen)
}

async fn status(cdn: &'static str, t: Table, json: bool) -> (Outcome, Seen) {
    status_with(vec![(OWNED, 200, OWNED_EXTERNAL), (CDN_GET, 200, cdn)], t, json).await
}

#[tokio::test]
async fn status_with_no_records_yet_is_waiting_for_dns() {
    let (o, seen) = status(PENDING, table(&[("sites.mosspub.com", &["104.0.0.1"])]), false).await;
    assert_eq!(o.code, 3);
    assert_eq!(o.out.matches("[not yet]").count(), 2, "{}", o.out);
    assert_eq!(o.out.matches("[waiting]").count(), 2, "{}", o.out);
    assert!(o.out.contains("now: could not look up"));
    assert!(o.out.contains("waiting for DNS. 2 of 2 address records"));
    assert_eq!(count(&seen, "GET /api/domains/example.com/status"), 0, "an owner-run domain needs no probe");
}

#[tokio::test]
async fn status_with_some_records_names_what_is_left() {
    let t = table(&[("example.com", &[IP]), ("sites.mosspub.com", &["104.0.0.1"]), ("www.example.com", &["203.0.113.9"])]);
    let (o, _) = status(PENDING, t, false).await;
    assert_eq!(o.code, 3);
    assert!(o.out.contains(&format!("[done   ] A @ {}", IP)), "{}", o.out);
    assert!(o.out.contains("[not yet] CNAME www"), "{}", o.out);
    assert!(o.out.contains("now: 203.0.113.9"), "{}", o.out);
    assert!(o.out.contains("waiting for DNS. 1 of 2"));
}

#[tokio::test]
async fn status_with_every_record_and_an_active_certificate_is_live() {
    let (o, _) = status(ACTIVE, all_in_place(), false).await;
    assert_eq!(o.code, 0);
    assert!(o.out.contains("Verdict: live."), "{}", o.out);
    assert!(!o.out.contains("[not yet]") && !o.out.contains("[waiting]"));
}

#[tokio::test]
async fn a_live_status_makes_the_next_build_use_www() {
    let dir = folder();
    let path = dir.path().to_str().unwrap();
    crate::vault::config::set_domain_in_config(path, "example.com").unwrap();
    let (client, _) = mock(vec![(OWNED, 200, OWNED_EXTERNAL), (CDN_GET, 200, ACTIVE)]).await;
    let o = status_flow(&client, path, "example.com", false, &all_in_place()).await;
    assert_eq!(o.code, 0, "{}", o.out);
    let url = crate::build::site_url::resolve_for_project(path, None).unwrap();
    assert_eq!(url.as_str(), "https://www.example.com");
}

#[tokio::test]
async fn records_added_but_not_yet_confirmed_is_not_reported_as_missing() {
    let (o, _) = status(PENDING, all_in_place(), false).await;
    assert_eq!(o.code, 3);
    assert!(o.out.contains("waiting for the hosting service"), "{}", o.out);
    assert!(!o.out.contains("not in place"), "{}", o.out);
    assert!(!o.out.contains("[not yet]"), "{}", o.out);
}

#[tokio::test]
async fn status_with_records_in_place_and_a_certificate_being_issued_waits_for_it() {
    let (o, _) = status(VALIDATING, all_in_place(), true).await;
    assert_eq!(o.code, 3);
    let v: serde_json::Value = serde_json::from_str(&o.out).unwrap();
    assert_eq!(v["verdict"], "waiting_for_certificate");
    assert_eq!(v["certificate"], "being issued");
    assert_eq!(v["records"][1]["done"], true);
}

#[tokio::test]
async fn a_failed_hostname_or_an_empty_record_list_exits_4() {
    for cdn in [FAILED, NO_CDN] {
        let (o, _) = status(cdn, all_in_place(), true).await;
        assert_eq!(o.code, 4, "{}", cdn);
        let v: serde_json::Value = serde_json::from_str(&o.out).unwrap();
        assert_eq!(v["verdict"], "failed");
        assert!(v["reason"].as_str().unwrap().contains("moss domain link ") && v["reason"].as_str().unwrap().contains(" example.com"));
    }
}

#[tokio::test]
async fn status_of_a_moss_bought_domain_follows_the_services_probe() {
    let route = "GET /api/domains/example.com/status";
    let (o, seen) = status_with(vec![(OWNED, 200, OWNED_MOSS), (CDN_GET, 200, ACTIVE), (route, 200, DNS_OK)], table(&[]), false).await;
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(o.out.contains("moss manages this domain's DNS"), "{}", o.out);
    assert_eq!(count(&seen, route), 1);
    let (o, _) = status_with(vec![(OWNED, 200, OWNED_MOSS), (CDN_GET, 200, NO_CDN), (route, 200, DNS_PENDING)], table(&[]), true).await;
    assert_eq!(o.code, 3);
    assert!(o.out.contains("waiting_for_dns"), "{}", o.out);
}

#[tokio::test]
async fn status_service_error_exits_2() {
    let (o, _) = status_with(vec![(OWNED, 500, r#"{"error":"x"}"#)], table(&[]), false).await;
    assert_eq!(o.code, 2);
}

#[tokio::test]
async fn list_json_marks_this_site() {
    let (client, _) = mock(vec![(OWNED, 200, OWNED_EXTERNAL)]).await;
    let o = list_flow(&client, Some("s1"), true).await;
    let v: serde_json::Value = serde_json::from_str(&o.out).unwrap();
    assert_eq!(v["domains"][0], serde_json::json!({"host":"example.com","source":"external","assigned_to":"s1","this_site":true}));
}
