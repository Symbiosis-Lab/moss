use super::*;
use crate::build::types::{SourceRevision, SourceSpan};

fn picture(page: &str, url: &str) -> ReferenceOccurrence {
    ReferenceOccurrence {
        source_path: page.to_string(),
        source_revision: SourceRevision::from_source(page),
        reference: url.to_string(),
        source_span: SourceSpan { start_byte: 0, end_byte: 1, line: 3 },
    }
}

const QUICK: Budget = Budget { at_once: 8, per_url: Duration::from_millis(800), whole_run: Duration::from_secs(5) };

fn page_address(source: &str) -> Option<String> {
    Some(format!("https://site.example/{}", source.trim_end_matches(".md")))
}

#[tokio::test(flavor = "multi_thread")]
async fn only_a_picture_its_host_calls_gone_is_marked() {
    let mut server = mockito::Server::new_async().await;
    let _ok = server.mock("HEAD", "/ok.jpg").with_status(200).with_header("content-type", "image/jpeg").create_async().await;
    let _gone = server.mock("HEAD", "/gone.jpg").with_status(404).create_async().await;
    let _gone_get = server.mock("GET", "/gone.jpg").with_status(404).create_async().await;
    let _removed = server.mock("HEAD", "/removed.jpg").with_status(410).create_async().await;
    let _removed_get = server.mock("GET", "/removed.jpg").with_status(410).create_async().await;
    // A signed-URL CDN: HEAD says 404, the GET a browser sends is served.
    let _signed = server.mock("HEAD", "/signed.jpg").with_status(404).create_async().await;
    let _signed_get = server.mock("GET", "/signed.jpg").match_header("range", "bytes=0-0").with_status(206).create_async().await;
    let _no_head = server.mock("HEAD", "/no-head.jpg").with_status(405).create_async().await;
    let _ranged = server.mock("GET", "/no-head.jpg").match_header("range", "bytes=0-0").with_status(206).create_async().await;
    let _busy = server.mock("HEAD", "/busy.jpg").with_status(503).create_async().await;
    // A hotlink-protected host: the picture is there for a visitor's browser.
    let _guarded_ok = server.mock("HEAD", "/guarded.jpg").match_header("referer", "https://site.example/posts/cat").with_status(200).create_async().await;
    let _guarded = server.mock("HEAD", "/guarded.jpg").match_header("referer", mockito::Matcher::Missing).with_status(404).create_async().await;
    let _guarded_get = server.mock("GET", "/guarded.jpg").match_header("referer", mockito::Matcher::Missing).with_status(404).create_async().await;
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let hang = format!("http://{}/hang.jpg", silent.local_addr().unwrap());
    let url = |p: &str| format!("{}{p}", server.url());

    let report = probe_all(
        vec![
            picture("posts/cat.md", &url("/ok.jpg")),
            picture("posts/cat.md", &url("/gone.jpg")),
            picture("posts/dog.md", &url("/gone.jpg")),
            picture("posts/cat.md", &url("/removed.jpg")),
            picture("posts/cat.md", &url("/no-head.jpg")),
            picture("posts/cat.md", &url("/busy.jpg")),
            picture("posts/cat.md", &url("/signed.jpg")),
            picture("posts/cat.md", &url("/guarded.jpg")),
            picture("posts/cat.md", &hang),
        ],
        &page_address,
        QUICK,
    )
    .await;

    let marked: Vec<_> = report.gone.iter().map(|g| (g.occurrence.source_path.as_str(), g.occurrence.reference.trim_start_matches(&server.url()), g.status)).collect();
    assert_eq!(marked, vec![("posts/cat.md", "/gone.jpg", 404), ("posts/dog.md", "/gone.jpg", 404), ("posts/cat.md", "/removed.jpg", 410)]);
    assert!(!report.offline);
    drop(silent);
}

#[tokio::test(flavor = "multi_thread")]
async fn no_host_answering_marks_nothing() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = closed.local_addr().unwrap();
    drop(closed);

    let report = probe_all(vec![picture("a.md", &format!("http://{address}/a.jpg"))], &|_| None, QUICK).await;

    assert_eq!(report, RemotePictureReport { offline: true, ..Default::default() });
    assert_eq!(report.lines(), vec!["Pictures from other sites were not checked: no host answered.".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_past_its_limit_counts_what_it_left() {
    let mut server = mockito::Server::new_async().await;
    let _ok = server.mock("HEAD", "/ok.jpg").with_status(200).create_async().await;
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let hang = |n: u8| format!("http://{}/{n}.jpg", silent.local_addr().unwrap());
    let budget = Budget { at_once: 8, per_url: Duration::from_secs(30), whole_run: Duration::from_millis(600) };

    let report = probe_all(
        vec![picture("a.md", &format!("{}/ok.jpg", server.url())), picture("a.md", &hang(1)), picture("a.md", &hang(2))],
        &|_| None,
        budget,
    )
    .await;

    assert_eq!((report.gone.len(), report.unchecked, report.offline), (0, 2, false));
    assert_eq!(report.lines(), vec!["2 pictures from other sites were not checked in time.".to_string()]);
}
