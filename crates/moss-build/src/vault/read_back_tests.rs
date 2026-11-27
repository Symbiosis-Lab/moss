use super::*;
use mockito::{Matcher, Server, ServerGuard};
use serde_json::json;
use tempfile::TempDir;

/// A fake seta answering `path` for `since`, with the page's records.
async fn page(server: &mut ServerGuard, path: &str, since: u64, body: Value) -> mockito::Mock {
    server
        .mock("GET", path)
        .match_query(Matcher::UrlEncoded("since".into(), since.to_string()))
        .with_header("content-type", "application/json")
        .with_body(body.to_string())
        .create_async()
        .await
}

/// A seta whose site has no request form entries.
async fn no_requests(server: &mut ServerGuard) {
    server.mock("GET", Matcher::Regex("^/api/sites/s1/form-entries".into())).with_header("content-type", "application/json")
        .with_body(r#"{"records":[],"cursor":0,"more":false}"#).create_async().await;
}

fn client(server: &ServerGuard) -> MossSetaClient {
    let identity = crate::identity::Identity::generate().unwrap();
    MossSetaClient::with_identity_and_url(&identity, &server.url())
}

async fn pull_parts(seta: &MossSetaClient, root: &Path, parts: &[Part]) -> Vec<usize> {
    let commerce = Some(seta.clone());
    let sources = Sources { seta, commerce, site_id: "s1" };
    pull(&sources, root, parts).await.into_iter().map(|(p, r)| r.unwrap_or_else(|e| panic!("{}: {e}", p.name()))).collect()
}

fn lines(root: &Path, rel: &str) -> Vec<Value> {
    read_lines(&root.join(rel)).unwrap()
}

/// The server's second answer re-sends what the folder already holds, as a
/// server with a stale cursor would: the folder still gains nothing.
async fn twice(server: &mut ServerGuard, path: &str, key: &str, records: Value) {
    let last = records.as_array().unwrap().last().unwrap()[key].as_u64().unwrap();
    page(server, path, 0, json!({"records": records, "cursor": last, "more": false})).await;
    page(server, path, last, json!({"records": records, "cursor": last, "more": false})).await;
}

#[tokio::test]
async fn a_subscriber_event_reaches_the_folder_once() {
    let mut server = Server::new_async().await;
    let consent = json!({"seq": 1, "eventId": "e1", "kind": "consent", "email": "a@example.org", "fact": {"type": "subscribed"}});
    twice(&mut server, "/api/sites/s1/subscriber-events", "seq", json!([consent])).await;
    let (dir, seta) = (TempDir::new().unwrap(), client(&server));

    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Newsletter]).await, [1]);
    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Newsletter]).await, [0]);
    assert_eq!(lines(dir.path(), SUBSCRIBER.file), [consent]);
}

#[tokio::test]
async fn a_form_entry_reaches_its_scope_view_once() {
    let mut server = Server::new_async().await;
    no_requests(&mut server).await;
    let entry = json!({"seq": 4, "recordedAt": 1700000000, "email": "b@example.org",
        "fact": {"type": "response_submitted", "scope": "rsvp", "answers": {"guests": 2, "note": "a, b", "topics": ["Print", "Web"]}}});
    twice(&mut server, "/api/sites/s1/subscriber-events", "seq", json!([entry])).await;
    let (dir, seta) = (TempDir::new().unwrap(), client(&server));

    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Forms]).await, [1]);
    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Forms]).await, [0]);
    let csv = fs::read_to_string(dir.path().join(".moss/data/forms/rsvp.csv")).unwrap();
    assert_eq!(csv, "recordedAt,email,guests,note,topics\n1700000000,b@example.org,2,\"a, b\",Print; Web\n");
}

#[tokio::test]
async fn a_comment_and_its_later_approval_reach_the_folder_once() {
    let mut server = Server::new_async().await;
    let path = "/api/sites/s1/comment-records";
    let held = json!({"id": 7, "email": "c@example.org", "content": "hi", "pending": true});
    page(&mut server, path, 0, json!({"records": [held], "cursor": 7, "more": false, "answers": []})).await;
    let asked = server
        .mock("GET", path)
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("since".into(), "7".into()),
            Matcher::UrlEncoded("pending".into(), "7".into()),
        ]))
        .with_body(json!({"records": [], "cursor": 7, "more": false, "answers": [{"id": 7, "state": "approved"}]}).to_string())
        .expect(1)
        .create_async()
        .await;
    let quiet = page(&mut server, path, 7, json!({"records": [], "cursor": 7, "more": false, "answers": []})).await;
    let (dir, seta) = (TempDir::new().unwrap(), client(&server));

    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Comments]).await, [1]);
    pull_parts(&seta, dir.path(), &[Part::Comments]).await;
    pull_parts(&seta, dir.path(), &[Part::Comments]).await;
    assert_eq!(lines(dir.path(), COMMENTS.file), [held]);
    assert_eq!(lines(dir.path(), COMMENT_ANSWERS), [json!({"id": 7, "state": "approved"})]);
    asked.assert_async().await;
    drop(quiet);
}

#[tokio::test]
async fn a_booking_change_reaches_the_folder_once() {
    let mut server = Server::new_async().await;
    let booking = json!({"changeSeq": 3, "id": "b1", "status": "confirmed", "stripePaymentIntentId": "pi_1"});
    twice(&mut server, "/sites/s1/bookings", "changeSeq", json!([booking])).await;
    let (dir, seta) = (TempDir::new().unwrap(), client(&server));

    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Bookings]).await, [1]);
    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Bookings]).await, [0]);
    assert_eq!(lines(dir.path(), BOOKINGS.file), [booking]);
}

#[tokio::test]
async fn a_site_commerce_does_not_know_has_no_bookings() {
    let mut server = Server::new_async().await;
    server.mock("GET", "/sites/s1/bookings").match_query(Matcher::Any).with_status(404).with_body(r#"{"error":"unknown_site"}"#).create_async().await;
    let (dir, seta) = (TempDir::new().unwrap(), client(&server));
    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Bookings]).await, [0]);
}

#[tokio::test]
async fn an_analytics_event_reaches_the_folder_once() {
    let mut server = Server::new_async().await;
    let view = json!({"id": 9, "ts": 1, "path": "/", "visitor": "v"});
    let path = "/api/sites/s1/events";
    page(&mut server, path, 0, json!({"events": [view], "cursor": 9, "more": false})).await;
    page(&mut server, path, 9, json!({"events": [view], "cursor": 9, "more": false})).await;
    let (dir, seta) = (TempDir::new().unwrap(), client(&server));
    let old_cursor = dir.path().join(OLD_ANALYTICS_CURSOR);
    fs::create_dir_all(old_cursor.parent().unwrap()).unwrap();
    fs::write(&old_cursor, r#"{"cursor":3}"#).unwrap();

    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Analytics]).await, [1]);
    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Analytics]).await, [0]);
    assert_eq!(lines(dir.path(), ANALYTICS.file), [view]);
    assert!(!old_cursor.exists());
}

/// A crash cut the last line short: the pull asks again from the last whole
/// line, so nothing is lost and nothing is held twice.
#[tokio::test]
async fn a_line_cut_short_is_fetched_again_and_never_doubled() {
    let mut server = Server::new_async().await;
    let (one, two) = (json!({"seq": 1, "eventId": "e1"}), json!({"seq": 2, "eventId": "e2"}));
    page(&mut server, "/api/sites/s1/subscriber-events", 1, json!({"records": [two], "cursor": 2, "more": false})).await;
    let dir = TempDir::new().unwrap();
    let file = dir.path().join(SUBSCRIBER.file);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, format!("{one}\n{{\"seq\":2,\"eve")).unwrap();

    assert_eq!(pull_parts(&client(&server), dir.path(), &[Part::Newsletter]).await, [1]);
    assert_eq!(lines(dir.path(), SUBSCRIBER.file), [one, two]);
}

#[tokio::test]
async fn a_404_that_is_not_an_unknown_site_is_an_error() {
    let mut server = Server::new_async().await;
    server.mock("GET", "/sites/s1/bookings").match_query(Matcher::Any).with_status(404).with_body("Not Found").create_async().await;
    let (dir, seta) = (TempDir::new().unwrap(), client(&server));
    let sources = Sources { seta: &seta, commerce: Some(seta.clone()), site_id: "s1" };
    assert!(pull(&sources, dir.path(), &[Part::Bookings]).await[0].1.is_err());
}

/// The cursor comes from the end of the stream alone: a head that is not even
/// text, longer than the first window, is never read, and a last line longer
/// than the window is still found whole.
#[test]
fn the_cursor_is_read_from_the_tail_alone() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("s.jsonl");
    let mut bytes = vec![0xFF; 10_000];
    bytes.push(b'\n');
    bytes.extend(format!("{}\n", json!({"seq": 5, "pad": "x".repeat(9_000)})).as_bytes());
    bytes.extend(b"{\"seq\":6,\"cut");
    fs::write(&file, &bytes).unwrap();
    assert_eq!(tail_key(&file, "seq").unwrap(), 5);
    assert_eq!(tail_key(&dir.path().join("absent.jsonl"), "seq").unwrap(), 0);
}

async fn ask(server: &mut ServerGuard, since: &str, pending: String, body: Value) -> mockito::Mock {
    server
        .mock("GET", "/api/sites/s1/comment-records")
        .match_query(Matcher::AllOf(vec![Matcher::UrlEncoded("since".into(), since.into()), Matcher::UrlEncoded("pending".into(), pending)]))
        .with_body(body.to_string())
        .expect(1)
        .create_async()
        .await
}

/// seta reads every comment for each request, so a batch of pending ids rides
/// on a page the stream needs anyway, and a batch left over gets a request of
/// its own: two pages and three batches are three requests.
#[tokio::test]
async fn pending_batches_ride_on_the_stream_pages() {
    let mut server = Server::new_async().await;
    let dir = TempDir::new().unwrap();
    let held: Vec<Value> = (1..=401).map(|id| json!({"id": id, "pending": true})).collect();
    append_lines(&dir.path().join(COMMENTS.file), &held).unwrap();
    let batch = |ids: std::ops::RangeInclusive<u64>| ids.map(|id| id.to_string()).collect::<Vec<_>>().join(",");
    let quiet = |answer: u64| json!({"records": [], "cursor": 402, "more": false, "answers": [{"id": answer, "state": "deleted"}]});
    let asks = [
        ask(&mut server, "401", batch(1..=200), json!({"records": [{"id": 402}], "cursor": 402, "more": true, "answers": [{"id": 1, "state": "approved"}]})).await,
        ask(&mut server, "402", batch(201..=400), quiet(201)).await,
        ask(&mut server, "402", batch(401..=401), quiet(401)).await,
    ];

    assert_eq!(pull_parts(&client(&server), dir.path(), &[Part::Comments]).await, [1]);
    for asked in asks {
        asked.assert_async().await;
    }
    assert_eq!(lines(dir.path(), COMMENT_ANSWERS).len(), 3);
}

#[tokio::test]
async fn a_request_form_entry_reaches_its_form_view_once_in_the_form_order() {
    let mut server = Server::new_async().await;
    page(&mut server, "/api/sites/s1/subscriber-events", 0, json!({"records": [], "cursor": 0, "more": false})).await;
    let entry = json!({"seq": 3, "form": "report", "page": "/add/", "receivedAt": 1700000000,
        "fields": ["program", "outcome", "topics"], "answers": {"program": "=HYPERLINK(\"x\")", "outcome": "\t=1+1", "topics": ["Culture", "Youth"]}});
    twice(&mut server, "/api/sites/s1/form-entries", "seq", json!([entry])).await;
    let (dir, seta) = (TempDir::new().unwrap(), client(&server));

    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Forms]).await, [1]);
    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Forms]).await, [0]);
    assert_eq!(lines(dir.path(), FORM_ENTRIES.file), [entry]);
    let csv = fs::read_to_string(dir.path().join(".moss/data/forms/report.csv")).unwrap();
    assert_eq!(csv, "receivedAt,page,program,outcome,topics\n1700000000,/add/,\"'=HYPERLINK(\"\"x\"\")\",'\t=1+1,Culture; Youth\n");
}

#[tokio::test]
async fn a_seta_without_request_forms_has_no_entries() {
    let mut server = Server::new_async().await;
    page(&mut server, "/api/sites/s1/subscriber-events", 0, json!({"records": [], "cursor": 0, "more": false})).await;
    server.mock("GET", Matcher::Regex("^/api/sites/s1/form-entries".into())).with_status(404).with_body("404 Not Found").create_async().await;
    let (dir, seta) = (TempDir::new().unwrap(), client(&server));
    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Forms]).await, [0]);
}

/// A pull that brings no form entry leaves the view as the owner left it, and
/// one that brings an entry rewrites only its own scope's file: each write is a
/// change the folder syncs.
#[tokio::test]
async fn the_forms_view_is_written_only_when_it_changes() {
    let mut server = Server::new_async().await;
    no_requests(&mut server).await;
    let path = "/api/sites/s1/subscriber-events";
    let entry = |seq: u64, scope: &str| json!({"seq": seq, "email": "e@example.org", "fact": {"type": "response_submitted", "scope": scope, "answers": {"q": seq}}});
    let consent = json!({"seq": 4, "fact": {"type": "subscribed"}});
    page(&mut server, path, 0, json!({"records": [entry(1, "a"), entry(2, "b")], "cursor": 2, "more": false})).await;
    page(&mut server, path, 2, json!({"records": [consent], "cursor": 4, "more": false})).await;
    page(&mut server, path, 4, json!({"records": [entry(5, "a")], "cursor": 5, "more": false})).await;
    let (dir, seta) = (TempDir::new().unwrap(), client(&server));
    let view = |scope: &str| dir.path().join(FORMS_DIR).join(format!("{scope}.csv"));
    let long_ago = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1);
    let age = |scope: &str| fs::metadata(view(scope)).unwrap().modified().unwrap();
    let make_old = |scope: &str| File::options().write(true).open(view(scope)).unwrap().set_modified(long_ago).unwrap();

    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Forms]).await, [2]);
    fs::write(view("a"), "sorted by hand\n").unwrap();
    make_old("b");
    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Newsletter, Part::Forms]).await, [1, 0]);
    assert_eq!(fs::read_to_string(view("a")).unwrap(), "sorted by hand\n");
    assert_eq!(age("b"), long_ago);
    assert_eq!(pull_parts(&seta, dir.path(), &[Part::Forms]).await, [1]);
    assert_eq!(fs::read_to_string(view("a")).unwrap(), "recordedAt,email,q\n,e@example.org,1\n,e@example.org,5\n");
    assert_eq!(age("b"), long_ago);
}

/// The comment streams hold commenters' emails: they stay out of the watched
/// folder that comment plugins read.
#[test]
fn the_comment_streams_are_not_watched() {
    assert!(!crate::moss_paths::is_watchable_rel(COMMENTS.file));
    assert!(!crate::moss_paths::is_watchable_rel(COMMENT_ANSWERS));
}

#[tokio::test]
async fn pages_are_read_to_the_end() {
    let mut server = Server::new_async().await;
    let path = "/api/sites/s1/subscriber-events";
    page(&mut server, path, 0, json!({"records": [{"seq": 1}], "cursor": 1, "more": true})).await;
    page(&mut server, path, 1, json!({"records": [{"seq": 2}], "cursor": 2, "more": false})).await;
    let dir = TempDir::new().unwrap();
    assert_eq!(pull_parts(&client(&server), dir.path(), &[Part::Newsletter]).await, [2]);
}

#[test]
fn every_part_has_its_name() {
    for part in Part::ALL {
        assert_eq!(Part::parse(part.name()), Some(part));
    }
    assert_eq!(Part::parse("subscribers"), None);
}
