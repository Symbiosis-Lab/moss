use super::*;

/// The gap this closes: a host's own `Retry-After` was previously
/// ignored outright — every retry used the same blind exponential
/// schedule (500ms, 1s, 2s, …) regardless of what the 429 response
/// asked for. `Retry-After: 1` asks for a full second, longer than the
/// 500ms the old schedule's first retry would have waited, so a crawl
/// that still only waited ~500ms here would prove the header was never
/// read. Timing is the only way to observe that distinction — total_pages
/// alone can't, since both the old and new code eventually retry and
/// succeed either way.
#[tokio::test]
async fn a_429_with_retry_after_is_honored_before_the_retry_that_imports_the_page() {
    let mut server = mockito::Server::new_async().await;
    let rate_limited = server
        .mock("GET", "/page")
        .with_status(429)
        .with_header("Retry-After", "1")
        .with_body("rate limited")
        .create_async()
        .await;
    let ok = server
        .mock("GET", "/page")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>Real page body, long enough to be extracted \
             as content once the rate limit clears.</p></article></body></html>",
        )
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let config = ScrapeConfig::new(format!("{}/page", server.url()), tmp.path());
    let started = std::time::Instant::now();
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    let elapsed = started.elapsed();
    rate_limited.assert_async().await;
    ok.assert_async().await;

    assert_eq!(res.total_pages, 1, "the page imports once the rate limit clears");
    assert_eq!(res.failed_pages, 0);
    assert!(
        elapsed >= std::time::Duration::from_millis(900),
        "Retry-After: 1 must be honored as roughly a 1s wait, not the shorter \
         500ms default backoff the old code would have used instead: {elapsed:?}"
    );
}

/// A 429 that never clears must not be retried forever — bounded at the
/// pipeline's standard attempt count, after which the URL is reported
/// failed like any other permanently-unreachable page. `.expect(3)` below
/// is the proof: the mock itself fails the test if it is hit any other
/// number of times.
#[tokio::test]
async fn a_429_that_never_clears_fails_after_a_bounded_number_of_attempts() {
    let mut server = mockito::Server::new_async().await;
    let rate_limited = server
        .mock("GET", "/stuck")
        .with_status(429)
        .with_body("rate limited")
        .expect(3)
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let config = ScrapeConfig::new(format!("{}/stuck", server.url()), tmp.path());
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    rate_limited.assert_async().await;

    assert_eq!(res.total_pages, 0);
    assert_eq!(res.failed_pages, 1, "a 429 that never clears is reported as a failed page");
}

/// The fix this file exists for: once a fetch to a host has exhausted
/// every retry on 429 (the test above), the crawl must not go straight
/// back to hammering that SAME host at full speed — the next request to
/// it is paced out. Sitemap-seeded (not link-discovered) so `/a`, `/b`,
/// `/c` are queued in a fixed, known order — `extract_links` returns a
/// `HashSet`, so sibling links from one page have no guaranteed order
/// (see the comment on the duplicate-variant test below), which this
/// test cannot tolerate: `/b` must be fetched right after `/a` fails,
/// and `/c` right after `/b` succeeds, or the timing math doesn't hold.
/// `/a`'s own three failed attempts already space its retries out by
/// ~1.5s (500ms + 1s backoff) — more than the pacer's first 500ms
/// growth step — so `/b` sees no EXTRA wait (it doesn't need to, to
/// prove the fix); `/c` is where growth becomes observable, since `/b`
/// itself answers almost instantly and leaves little of that 1.5s
/// credit behind.
#[tokio::test]
async fn a_terminal_rate_limit_paces_the_next_request_to_the_same_host() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let robots = server
        .mock("GET", "/robots.txt")
        .with_status(200)
        .with_body(format!("Sitemap: {base}/sitemap.xml\n"))
        .create_async()
        .await;
    let sitemap = server
        .mock("GET", "/sitemap.xml")
        .with_status(200)
        .with_header("content-type", "application/xml")
        .with_body(format!(
            "<?xml version=\"1.0\"?>\
             <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\
             <url><loc>{base}/a</loc></url>\
             <url><loc>{base}/b</loc></url>\
             <url><loc>{base}/c</loc></url>\
             </urlset>"
        ))
        .create_async()
        .await;
    let root = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>Root page, long enough to be extracted \
             as content by the generic scorer.</p></article></body></html>",
        )
        .create_async()
        .await;
    let stuck = server
        .mock("GET", "/a")
        .with_status(429)
        .with_body("rate limited")
        .expect(3)
        .create_async()
        .await;
    let b = server
        .mock("GET", "/b")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>Page b, long enough to be extracted \
             as content by the generic scorer.</p></article></body></html>",
        )
        .create_async()
        .await;
    let c = server
        .mock("GET", "/c")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>Page c, long enough to be extracted \
             as content by the generic scorer.</p></article></body></html>",
        )
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    let started = std::time::Instant::now();
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    let elapsed = started.elapsed();

    robots.assert_async().await;
    sitemap.assert_async().await;
    root.assert_async().await;
    stuck.assert_async().await;
    b.assert_async().await;
    c.assert_async().await;

    assert_eq!(res.total_pages, 3, "root, b, and c all import");
    assert_eq!(res.failed_pages, 1, "a's 429 never clears");
    assert_eq!(
        res.rate_limited_hosts.len(),
        1,
        "exactly one host was ever rate-limited this crawl"
    );
    assert_eq!(
        res.rate_limited_hosts[0].interval_ms, 500,
        "one 429 event grows the interval by exactly one doubling step from zero"
    );
    assert!(
        elapsed >= std::time::Duration::from_millis(1700),
        "`/a`'s own ~1.5s of retry backoff plus the ~250ms pacing wait before `/c` \
         (half the grown interval, decayed once by `/b`'s own success) must both show \
         up in the total crawl time, proving the wait is actually awaited rather than \
         merely recorded: {elapsed:?}"
    );
}

/// Gap 2: a failed fetch of a query-string variant of a path this crawl
/// has already CONFIRMED a duplicate on must not become a `scrape_error`
/// stub — the content is already on disk under that path, so the stub
/// would be a confusing duplicate of a real page. Counted as
/// `unreachable_variants` instead. `?itemId=1` confirms the duplicate
/// (rule 2: same path as root, byte-identical body, no canonical of its
/// own) before `?itemId=2` ever fails — guaranteed by linking `?itemId=2`
/// only from `?itemId=1`'s own page rather than from root alongside it:
/// `extract_links` returns a `HashSet`, so two sibling links discovered
/// off the SAME page have no guaranteed processing order, and an earlier
/// version of this test asserting that order flaked under
/// `--test-threads=1` alone (ablated: reverted to sibling links under
/// root, reproduced the flake in a handful of runs). Chaining the link
/// through `?itemId=1`'s own body instead means `?itemId=2` is not even
/// IN the queue until the loop iteration that confirms `?itemId=1` a
/// duplicate has already finished. See
/// `an_old_cms_style_path_with_no_confirmed_duplicate_still_stubs_a_failed_sibling`
/// for the case where that precondition is absent and the stub must
/// survive instead. A genuinely new URL that fails the same way
/// (`/new-page`, no relation to anything already written) must still
/// get its stub exactly as before this check existed — both are
/// exercised together so an over-broad fix (treating every failure on
/// a written path as a variant) would be caught by the same test that
/// proves the real gap is closed.
#[tokio::test]
async fn a_failed_variant_of_a_confirmed_duplicate_path_is_not_stubbed_but_a_failed_new_page_still_is()
{
    let mut server = mockito::Server::new_async().await;
    let base = server.url();
    let shared_body = "Shared root content for the confirmed-duplicate-path test, long \
         enough to pass the content extraction scorer reliably.";

    let root = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            // The links sit in a <nav> OUTSIDE <article> — link discovery
            // reads the whole document, but content extraction strips nav
            // chrome, so root's extracted body still comes out
            // byte-identical to `?itemId=1`'s. Only `new-page` and
            // `?itemId=1` are linked here; `?itemId=2` is reachable only
            // through `?itemId=1`'s own page below, so it cannot be
            // queued, let alone processed, before `?itemId=1` is.
            "<html><body><nav><a href=\"{base}/new-page\">New</a>\
             <a href=\"{base}/?itemId=1\">Confirmed duplicate</a></nav>\
             <article><p>{shared_body}</p></article></body></html>"
        ))
        .create_async()
        .await;
    let new_page = server
        .mock("GET", "/new-page")
        .with_status(404)
        .with_body("not found")
        .create_async()
        .await;
    // Same path as root, same extracted body, no canonical of its own —
    // rule 2 confirms this a duplicate before `?itemId=2` is ever fetched.
    // Its own `?itemId=2` link is the only way that URL is ever
    // discovered, which is what pins the processing order.
    let confirmed_duplicate = server
        .mock("GET", "/?itemId=1")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><body><nav><a href=\"{base}/?itemId=2\">Unreachable after that</a></nav>\
             <article><p>{shared_body}</p></article></body></html>"
        ))
        .create_async()
        .await;
    let unreachable_variant = server
        .mock("GET", "/?itemId=2")
        .with_status(429)
        .with_body("rate limited")
        .expect(3)
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    root.assert_async().await;
    new_page.assert_async().await;
    confirmed_duplicate.assert_async().await;
    unreachable_variant.assert_async().await;

    assert_eq!(res.total_pages, 1, "only the root page is real content");
    assert_eq!(res.failed_pages, 1, "the genuinely new failing URL still counts as failed");
    assert_eq!(res.duplicate_pages, 1, "?itemId=1 confirms the duplicate");
    assert_eq!(
        res.unreachable_variants, 1,
        "?itemId=2 fails on a path already confirmed to produce duplicates, so it is \
         counted on its own, not as failed_pages"
    );

    let written: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    assert_eq!(
        written.len(),
        2,
        "the root page and the new-page stub, and nothing for either itemId variant: {written:?}"
    );
    let bodies: Vec<String> =
        written.iter().map(|p| std::fs::read_to_string(p).unwrap()).collect();
    assert!(
        bodies.iter().any(|b| b.contains("new-page")),
        "the genuinely new failing URL must still get a scrape_error stub: {bodies:?}"
    );
    assert!(
        !bodies.iter().any(|b| b.contains("itemId=2")),
        "the failed variant of a confirmed-duplicate path must never produce a stub: {bodies:?}"
    );
}

/// Gap 2's narrowing, proven directly: an old-CMS site that routes
/// distinct posts through the SAME path (`?p=1`, `?p=2`, …, each with
/// its own self-canonical, exactly the shape
/// `distinct_query_string_pages_with_self_canonicals_both_import` above
/// proves must both import when both fetches succeed) must not lose a
/// later sibling's stub just because an EARLIER sibling at the same
/// path happened to import first. `?p=1` writes successfully and is
/// never a duplicate of anything; `?p=2` then fails outright. Before
/// this narrowing, `VariantFetchFailed` matched on "any page already
/// written at this path" and would have silently dropped `?p=2` with no
/// stub — the same loss the real variant case (the test above) must
/// avoid, just triggered by two real pages instead of one real page
/// and its lightbox/filter echo.
#[tokio::test]
async fn an_old_cms_style_path_with_no_confirmed_duplicate_still_stubs_a_failed_sibling() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let page1 = server
        .mock("GET", "/blog/?p=1")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><head><link rel=\"canonical\" href=\"{base}/blog/?p=1\"></head>\
             <body><article><p>First real post, unique content A, long enough to be \
             extracted as the article body for page one.</p>\
             <a href=\"{base}/blog/?p=2\">Next</a></article></body></html>"
        ))
        .create_async()
        .await;
    let page2 = server
        .mock("GET", "/blog/?p=2")
        .with_status(429)
        .with_body("rate limited")
        .expect(3)
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/blog/?p=1"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    page1.assert_async().await;
    page2.assert_async().await;

    assert_eq!(res.total_pages, 1, "only ?p=1 ever fetched successfully");
    assert_eq!(
        res.failed_pages, 1,
        "?p=2 is a genuinely distinct post that failed to fetch, not a confirmed variant \
         of ?p=1 — no Duplicate verdict was ever reached on this path, so it must still \
         get the ordinary failure stub"
    );
    assert_eq!(
        res.unreachable_variants, 0,
        "nothing on this path has ever been confirmed a duplicate"
    );

    let written: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    assert_eq!(
        written.len(),
        2,
        "?p=1's real content and ?p=2's failure stub, both on disk: {written:?}"
    );
}

/// A page that extracts to nothing is a page failure, not a crawl failure:
/// the note is replaced by the error placeholder, `failed_pages` counts it,
/// and the run still succeeds. Before `compose_note` had one owner, only
/// the local-file arm refused an empty body — this arm wrote an empty note
/// and called it a success.
#[tokio::test]
async fn an_unextractable_page_is_counted_as_failed_not_written_empty() {
    let mut server = mockito::Server::new_async().await;
    let empty = server
        .mock("GET", "/nothing/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><head><title>t</title></head><body></body></html>")
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let config = ScrapeConfig::new(format!("{}/nothing/", server.url()), tmp.path());
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    empty.assert_async().await;

    assert_eq!(res.total_pages, 0, "nothing was imported");
    assert_eq!(res.failed_pages, 1, "the page is counted as failed");

    let written: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    assert_eq!(written.len(), 1, "one placeholder, not an empty note");
    let body = std::fs::read_to_string(&written[0]).unwrap();
    assert!(
        body.contains("no article content found"),
        "the placeholder must say why: {body}"
    );
}

/// Bug fix: a recursive crawl must never write a non-HTML response as a
/// `.md` page. A real site's own links routinely point at a PDF, an
/// image, a calendar file, and the site's own RSS feed — all four are
/// fetched alongside one real HTML page here, and only the HTML page may
/// become a note. The rest are counted as skipped, not failed, and
/// produce no file on disk at all.
#[tokio::test]
async fn non_html_responses_are_skipped_not_written_as_pages() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let index_body = format!(
        "<html><body><article><p>Real page body, long enough to be extracted as content.</p>\
         <a href=\"{base}/doc.pdf\">PDF</a>\
         <a href=\"{base}/photo.tiff\">TIFF</a>\
         <a href=\"{base}/event.ics\">ICS</a>\
         <a href=\"{base}/feed.xml\">Feed</a>\
         </article></body></html>"
    );
    let index = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(&index_body)
        .create_async()
        .await;
    let pdf = server
        .mock("GET", "/doc.pdf")
        .with_status(200)
        .with_header("content-type", "application/pdf")
        .with_body("%PDF-1.4 binary bytes here")
        .create_async()
        .await;
    let tiff = server
        .mock("GET", "/photo.tiff")
        .with_status(200)
        .with_header("content-type", "image/tiff")
        .with_body("II*\u{0} binary tiff bytes")
        .create_async()
        .await;
    let ics = server
        .mock("GET", "/event.ics")
        .with_status(200)
        .with_header("content-type", "text/calendar")
        .with_body("BEGIN:VCALENDAR\nEND:VCALENDAR")
        .create_async()
        .await;
    let feed = server
        .mock("GET", "/feed.xml")
        .with_status(200)
        .with_header("content-type", "application/rss+xml")
        .with_body("<?xml version=\"1.0\"?><rss><channel></channel></rss>")
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    index.assert_async().await;
    pdf.assert_async().await;
    tiff.assert_async().await;
    ics.assert_async().await;
    feed.assert_async().await;

    assert_eq!(res.total_pages, 1, "only the real HTML page is imported");
    assert_eq!(res.failed_pages, 0);
    assert_eq!(res.skipped_pages, 4, "the PDF, TIFF, .ics and feed are all skipped");

    let written: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    assert_eq!(
        written.len(),
        1,
        "no .md should be written for any of the skipped responses"
    );
}

/// Gap: a failed fetch used to write a `scrape_error` stub for ANY URL,
/// including one whose own extension already names a file a successful
/// fetch would have skipped as non-HTML (see
/// `non_html_responses_are_skipped_not_written_as_pages`) — on at least
/// one real corpus site, a flaky run produced six stub pages for a PDF
/// alone. A file this crawl would never have written as a page must not
/// become a stub page just because its fetch failed. `/s/doc.pdf`
/// proves that; `/about` and `/page.html` (no extension, and an
/// HTML-like one) prove the fix is scoped to non-page extensions only —
/// an ordinary failing link still gets its stub exactly as before.
#[tokio::test]
async fn a_failed_fetch_of_a_non_page_extension_is_not_stubbed_but_an_ordinary_failure_still_is()
{
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let index = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><body><article><p>Real page body, long enough to be extracted \
             as content.</p>\
             <a href=\"{base}/s/doc.pdf\">PDF</a>\
             <a href=\"{base}/about\">About</a>\
             <a href=\"{base}/page.html\">Page</a>\
             </article></body></html>"
        ))
        .create_async()
        .await;
    // 404s (permanent, never retried) — a transient 429/503 would have
    // proven the same thing after outlasting its retries, but 404 keeps
    // the test to one request per URL.
    let pdf = server.mock("GET", "/s/doc.pdf").with_status(404).create_async().await;
    let about = server.mock("GET", "/about").with_status(404).create_async().await;
    let page_html = server.mock("GET", "/page.html").with_status(404).create_async().await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    index.assert_async().await;
    pdf.assert_async().await;
    about.assert_async().await;
    page_html.assert_async().await;

    assert_eq!(res.total_pages, 1, "only the real HTML page is imported");
    assert_eq!(
        res.failed_pages, 2,
        "/about and /page.html are ordinary failed fetches and still count as failed"
    );
    assert_eq!(
        res.unreachable_files, 1,
        "/s/doc.pdf's own extension already answers the non-HTML question the fetch \
         never got to ask"
    );

    let written: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    assert_eq!(
        written.len(),
        3,
        "the real page plus two failure stubs (/about, /page.html) — nothing for the PDF: \
         {written:?}"
    );
    // Relative PATHS, not content — the real page's own body legitimately
    // mentions `doc.pdf` (its link to it got rewritten the same as any
    // other in-scope URL), so a content grep for "doc.pdf" would catch
    // that innocent reference instead of proving anything about a stub.
    let relative_paths: Vec<String> = written
        .iter()
        .map(|p| p.strip_prefix(tmp.path()).unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(
        relative_paths.iter().any(|p| p.contains("about")),
        "an ordinary failing link with no extension must still get its stub: \
         {relative_paths:?}"
    );
    assert!(
        relative_paths.iter().any(|p| p.contains("page")),
        "an ordinary failing link with an HTML-like extension must still get its stub: \
         {relative_paths:?}"
    );
    assert!(
        !relative_paths.iter().any(|p| p.contains("doc")),
        "a failed fetch of a non-page extension must never produce a stub file: \
         {relative_paths:?}"
    );
}

/// Bug fix: a capped recursive crawl must say so in its own result, not
/// only via a process exit code, and must say how many discovered
/// in-scope URLs it left behind — before this fix `ScrapeResult` carried
/// no such signal at all.
#[tokio::test]
async fn a_capped_crawl_reports_the_cap_and_the_leftover_count() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let index_body = format!(
        "<html><body><article><p>Root page with enough words to be extracted.</p>\
         <a href=\"{base}/a\">A</a><a href=\"{base}/b\">B</a><a href=\"{base}/c\">C</a>\
         </article></body></html>"
    );
    let index = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(&index_body)
        .create_async()
        .await;
    let leaf = |name: &str| {
        format!(
            "<html><body><article><p>Leaf page {name}, long enough to be extracted as content.</p></article></body></html>"
        )
    };
    let _a = server
        .mock("GET", "/a")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(leaf("a"))
        .create_async()
        .await;
    let _b = server
        .mock("GET", "/b")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(leaf("b"))
        .create_async()
        .await;
    let _c = server
        .mock("GET", "/c")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(leaf("c"))
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    config.max_pages = Some(2);
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    index.assert_async().await;

    assert_eq!(res.total_pages, 2, "the cap stopped the crawl after exactly 2 pages");
    assert!(res.capped, "the result must say the cap is what stopped the crawl");
    assert_eq!(
        res.remaining_urls, 2,
        "two of the three discovered children were never visited"
    );

    let written: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    assert_eq!(written.len(), 2, "the index plus exactly one child were written");
}

// ── canonical/body-hash duplicate detection (a recursive crawl's queue
// routinely fills with query-string variants of one page — a lightbox
// `?itemId=…`, a listing filter, a calendar export) ───────────────────

/// Rule 1: a chain of lightbox-style variants, each whose own `<link
/// rel="canonical">` names the one real gallery page, sits BETWEEN that
/// page and a second real page discovered only at the end of the chain.
/// A strict chain (each variant links only to the next) keeps the fetch
/// order deterministic, unlike sibling links pulled from one page's
/// `HashSet`. `max_pages` is set to 2 — the count of REAL pages: if a
/// duplicate consumed a cap slot, the cap would be reached mid-chain and
/// `/gallery/finale` would never be discovered at all. Since it does not, both
/// real pages import and the crawl finishes clean.
#[tokio::test]
async fn canonical_duplicates_are_not_written_and_do_not_consume_the_cap() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let gallery = server
        .mock("GET", "/gallery")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><body><article><p>Real gallery page body, long enough to be \
             extracted as content.</p><a href=\"{base}/gallery?itemId=1\">Item 1</a>\
             </article></body></html>"
        ))
        .create_async()
        .await;
    let variant = |n: u32, next_href: &str| {
        format!(
            "<html><head><link rel=\"canonical\" href=\"{base}/gallery\"></head>\
             <body><article><p>Lightbox chrome around item {n}.</p>\
             <a href=\"{next_href}\">Next</a></article></body></html>"
        )
    };
    let item1 = server
        .mock("GET", "/gallery?itemId=1")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(variant(1, &format!("{base}/gallery?itemId=2")))
        .create_async()
        .await;
    let item2 = server
        .mock("GET", "/gallery?itemId=2")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(variant(2, &format!("{base}/gallery?itemId=3")))
        .create_async()
        .await;
    let item3 = server
        .mock("GET", "/gallery?itemId=3")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(variant(3, &format!("{base}/gallery/finale")))
        .create_async()
        .await;
    let finale = server
        .mock("GET", "/gallery/finale")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><body><article><p>A second real page, discovered only past the \
             duplicate chain, long enough to be extracted as content.</p>\
             </article></body></html>"
        ))
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/gallery"), tmp.path());
    config.recursive = true;
    config.max_pages = Some(2);
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    gallery.assert_async().await;
    item1.assert_async().await;
    item2.assert_async().await;
    item3.assert_async().await;
    finale.assert_async().await;

    assert_eq!(res.total_pages, 2, "the gallery page and /gallery/finale are both written");
    assert_eq!(res.duplicate_pages, 3, "all three itemId variants are duplicates");
    assert_eq!(res.failed_pages, 0);
    assert_eq!(res.skipped_pages, 0);
    assert!(
        !res.capped,
        "three duplicates must not have consumed cap slots meant for /gallery/finale: {res:?}"
    );
    assert_eq!(res.remaining_urls, 0);

    let written: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    assert_eq!(written.len(), 2, "no duplicate must ever reach disk");
}

/// Rule 2 (fallback for a page with no canonical of its own): a second
/// fetch of the SAME URL PATH — a query-string variant, `?ref=share` —
/// whose extracted BODY is byte-identical to an already-written page's
/// is a duplicate too. A chain (root → note-a → the query variant)
/// keeps every URL inside the crawl's scope and its fetch order
/// deterministic.
#[tokio::test]
async fn no_canonical_but_identical_body_at_the_same_path_is_a_duplicate() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();
    let shared_body = "Shared identical content for the duplicate-body dedup test, \
         long enough to pass the content extraction scorer reliably.";

    let root = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><body><article><p>Root page body, distinct from the shared \
             note content and long enough to be extracted.</p>\
             <a href=\"{base}/note-a\">A</a></article></body></html>"
        ))
        .create_async()
        .await;
    let note_a = server
        .mock("GET", "/note-a")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            // The "Next" link sits in a <nav> OUTSIDE <article> — link
            // discovery reads the whole document, but content extraction
            // strips nav chrome, so note-a's extracted body still comes
            // out byte-identical to its own query-variant's.
            "<html><body><nav><a href=\"{base}/note-a?ref=share\">Next</a></nav>\
             <article><p>{shared_body}</p></article></body></html>"
        ))
        .create_async()
        .await;
    let note_a_variant = server
        .mock("GET", "/note-a?ref=share")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><body><article><p>{shared_body}</p></article></body></html>"
        ))
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    root.assert_async().await;
    note_a.assert_async().await;
    note_a_variant.assert_async().await;

    assert_eq!(res.total_pages, 2, "the root and the first-seen note are written");
    assert_eq!(
        res.duplicate_pages, 1,
        "the byte-identical same-path query variant is a duplicate"
    );
    assert_eq!(res.failed_pages, 0);

    let written: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    assert_eq!(written.len(), 2, "the duplicate body must never reach disk");
}

/// Rule 2 must never fire across two DIFFERENT paths, even when their
/// bodies are byte-identical: two real, distinct pages (different
/// addresses) that happen to share the same short templated body — the
/// shape a "coming soon" location stub takes — must both import.
#[tokio::test]
async fn identical_body_at_different_paths_both_import() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();
    let stub_body = "Coming soon — check back later.";

    let root = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><body><article><p>Root page, distinct from the stub pages \
             below and long enough to be extracted on its own.</p>\
             <a href=\"{base}/boston\">Boston</a>\
             <a href=\"{base}/chicago\">Chicago</a></article></body></html>"
        ))
        .create_async()
        .await;
    let boston = server
        .mock("GET", "/boston")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!("<html><body><article><p>{stub_body}</p></article></body></html>"))
        .create_async()
        .await;
    let chicago = server
        .mock("GET", "/chicago")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!("<html><body><article><p>{stub_body}</p></article></body></html>"))
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    root.assert_async().await;
    boston.assert_async().await;
    chicago.assert_async().await;

    assert_eq!(res.total_pages, 3, "the root and both distinct-path stubs all import");
    assert_eq!(
        res.duplicate_pages, 0,
        "different paths never collide, regardless of a shared body"
    );
    assert_eq!(res.failed_pages, 0);

    let written: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    assert_eq!(written.len(), 3, "both stub pages must survive: {written:?}");
}

/// Negative case: two real, DISTINCT pages behind an old-CMS-style query
/// string (`?p=1` / `?p=2`), each self-canonical, must both import — the
/// design's whole point is that a blanket query-string strip would
/// wrongly collapse exactly this shape into one page. Both pages map to
/// the same on-disk filename (query strings never enter `url_to_file_path`),
/// so this also proves `rename_for_collision` is still load-bearing after
/// this fix, not a deletion candidate.
#[tokio::test]
async fn distinct_query_string_pages_with_self_canonicals_both_import() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let page1 = server
        .mock("GET", "/blog/?p=1")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><head><link rel=\"canonical\" href=\"{base}/blog/?p=1\"></head>\
             <body><article><p>First real post, unique content A, long enough to be \
             extracted as the article body for page one.</p>\
             <a href=\"{base}/blog/?p=2\">Next</a></article></body></html>"
        ))
        .create_async()
        .await;
    let page2 = server
        .mock("GET", "/blog/?p=2")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><head><link rel=\"canonical\" href=\"{base}/blog/?p=2\"></head>\
             <body><article><p>Second real post, unique content B, long enough to be \
             extracted as the article body for page two.</p></article></body></html>"
        ))
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/blog/?p=1"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    page1.assert_async().await;
    page2.assert_async().await;

    assert_eq!(res.total_pages, 2, "both distinct pages must import");
    assert_eq!(res.duplicate_pages, 0, "distinct self-canonicals are never duplicates");
    assert_eq!(res.failed_pages, 0);

    let written: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    assert_eq!(
        written.len(),
        2,
        "both pages collide on filename (query strings aren't part of it) and must \
         survive via rename_for_collision: {written:?}"
    );
}

/// Rule 1 must compare only against identities already WRITTEN, never
/// against a target merely sitting in the queue: this page's own
/// canonical names a second page that this crawl goes on to discover and
/// queue, but that second page's own fetch then fails outright. If rule 1
/// had pre-emptively dropped this page on the bet that the queued target
/// would supply the content instead, that bet would have cost the only
/// real copy this crawl ever had of it — nothing would be written under
/// either URL. This page's own content must survive regardless.
#[tokio::test]
async fn a_page_is_written_when_its_own_canonical_target_later_fails_to_fetch() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    // The start URL's own path is the crawl root ("/"), which is the
    // one case `is_within_scope` treats specially and admits every
    // same-host URL regardless of path — needed here so the linked
    // `/canonical-target` is actually queued rather than filtered out
    // as a sibling path outside the starting URL's own prefix.
    let variant = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><head><link rel=\"canonical\" href=\"{base}/canonical-target\"></head>\
             <body><article><p>Real content that must survive even though its own \
             canonical tag names a page that will fail to load.</p>\
             <a href=\"{base}/canonical-target\">Canonical</a></article></body></html>"
        ))
        .create_async()
        .await;
    let canonical_target = server
        .mock("GET", "/canonical-target")
        .with_status(404)
        .with_body("not found")
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    variant.assert_async().await;
    canonical_target.assert_async().await;

    assert_eq!(res.total_pages, 1, "the variant's own real content is written");
    assert_eq!(res.failed_pages, 1, "the canonical target's own fetch failure is recorded");
    assert_eq!(res.duplicate_pages, 0, "nothing was dropped on a bet that never paid off");

    let written: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    assert_eq!(written.len(), 2, "the real page and the failure placeholder both land on disk");
    let bodies: Vec<String> =
        written.iter().map(|p| std::fs::read_to_string(p).unwrap()).collect();
    assert!(
        bodies.iter().any(|b| b.contains("Real content that must survive")),
        "the variant's real content must not have been discarded: {bodies:?}"
    );
}

// ── sitemap discovery (a crawl that only follows links never reaches a
// page nothing links to, and stops discovering new pages at all once
// the page cap is hit) ──────────────────────────────────────────────

#[tokio::test]
async fn a_sitemap_index_followed_by_a_sitemap_seeds_unlinked_pages() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let robots = server
        .mock("GET", "/robots.txt")
        .with_status(200)
        .with_body(format!("User-agent: *\nSitemap: {base}/sitemap-index.xml\n"))
        .create_async()
        .await;
    let sitemap_index = server
        .mock("GET", "/sitemap-index.xml")
        .with_status(200)
        .with_header("content-type", "application/xml")
        .with_body(format!(
            "<?xml version=\"1.0\"?>\
             <sitemapindex xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\
             <sitemap><loc>{base}/sitemap-1.xml</loc></sitemap>\
             </sitemapindex>"
        ))
        .create_async()
        .await;
    let sitemap_leaf = server
        .mock("GET", "/sitemap-1.xml")
        .with_status(200)
        .with_header("content-type", "application/xml")
        .with_body(format!(
            "<?xml version=\"1.0\"?>\
             <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\
             <url><loc>{base}/unlinked</loc></url>\
             </urlset>"
        ))
        .create_async()
        .await;
    let index = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>Root page with no link at all to the \
             sitemap-only page, long enough to be extracted as content.</p>\
             </article></body></html>",
        )
        .create_async()
        .await;
    let unlinked = server
        .mock("GET", "/unlinked")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>A page no crawlable link reaches, only the \
             sitemap declares it, long enough to be extracted as content.</p>\
             </article></body></html>",
        )
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    robots.assert_async().await;
    sitemap_index.assert_async().await;
    sitemap_leaf.assert_async().await;
    index.assert_async().await;
    unlinked.assert_async().await;

    assert_eq!(res.total_pages, 2, "the root and the unlinked sitemap page both import");
    assert_eq!(res.sitemap_urls, 1, "exactly one URL came from the sitemap");
    assert!(!res.sitemap_truncated);

    let unlinked_md = tmp.path().join("unlinked.md");
    assert!(unlinked_md.exists(), "the sitemap-only page must be written to disk");
}

#[tokio::test]
async fn sitemap_urls_beyond_the_link_cap_are_still_imported() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let robots = server
        .mock("GET", "/robots.txt")
        .with_status(200)
        .with_body(format!("Sitemap: {base}/sitemap.xml\n"))
        .create_async()
        .await;
    let sitemap = server
        .mock("GET", "/sitemap.xml")
        .with_status(200)
        .with_header("content-type", "application/xml")
        .with_body(format!(
            "<?xml version=\"1.0\"?>\
             <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\
             <url><loc>{base}/s1</loc></url><url><loc>{base}/s2</loc></url>\
             </urlset>"
        ))
        .create_async()
        .await;
    let index = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><body><article><p>Root page with three discovered-only \
             links and no sitemap coverage of its own.</p>\
             <a href=\"{base}/d1\">D1</a><a href=\"{base}/d2\">D2</a>\
             <a href=\"{base}/d3\">D3</a></article></body></html>"
        ))
        .create_async()
        .await;
    let s1 = server
        .mock("GET", "/s1")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>Sitemap-only page one, long enough to be \
             extracted as real content for this test.</p></article></body></html>",
        )
        .create_async()
        .await;
    let s2 = server
        .mock("GET", "/s2")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>Sitemap-only page two, long enough to be \
             extracted as real content for this test.</p></article></body></html>",
        )
        .create_async()
        .await;
    // None of the three link-discovered-only children may ever be
    // fetched: the cap of 1 is used up by the root page alone, and a
    // sitemap-declared URL is never capped, so no cap "room" is ever
    // freed up for them.
    let d1 = server.mock("GET", "/d1").expect(0).create_async().await;
    let d2 = server.mock("GET", "/d2").expect(0).create_async().await;
    let d3 = server.mock("GET", "/d3").expect(0).create_async().await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    config.max_pages = Some(1);
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    robots.assert_async().await;
    sitemap.assert_async().await;
    index.assert_async().await;
    s1.assert_async().await;
    s2.assert_async().await;
    d1.assert_async().await;
    d2.assert_async().await;
    d3.assert_async().await;

    assert_eq!(
        res.total_pages, 3,
        "root plus both sitemap-declared pages import despite a page cap of 1"
    );
    assert!(res.capped, "the link-discovered children are still capped");
    assert_eq!(res.remaining_urls, 3, "all three discovered-only children are left behind");
    assert_eq!(res.sitemap_urls, 2);
}

#[tokio::test]
async fn a_sitemap_url_wins_survivor_choice_over_a_link_discovered_duplicate() {
    // The gap this fixes: `/aboutus`, reached only by a link, and
    // `/about`, reached only via the sitemap, are the SAME page —
    // `/aboutus` declares `/about` as its own canonical. Before this
    // fix `/about` was never even queued (nothing links to it), so
    // `/aboutus` was the only copy ever written. Seeding sitemap URLs
    // ahead of link-discovered ones means `/about` is written FIRST,
    // so rule 1 (compares only against identities already WRITTEN)
    // now correctly makes `/aboutus` the duplicate instead.
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let robots = server
        .mock("GET", "/robots.txt")
        .with_status(200)
        .with_body(format!("Sitemap: {base}/sitemap.xml\n"))
        .create_async()
        .await;
    let sitemap = server
        .mock("GET", "/sitemap.xml")
        .with_status(200)
        .with_header("content-type", "application/xml")
        .with_body(format!(
            "<?xml version=\"1.0\"?>\
             <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\
             <url><loc>{base}/about</loc></url>\
             </urlset>"
        ))
        .create_async()
        .await;
    let index = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><body><article><p>Root page linking only to /aboutus, long \
             enough to be extracted as content on its own.</p>\
             <a href=\"{base}/aboutus\">About us</a></article></body></html>"
        ))
        .create_async()
        .await;
    let about = server
        .mock("GET", "/about")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>The real about page, reachable only \
             through the sitemap, long enough to be extracted.</p>\
             </article></body></html>",
        )
        .create_async()
        .await;
    let aboutus = server
        .mock("GET", "/aboutus")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><head><link rel=\"canonical\" href=\"{base}/about\"></head>\
             <body><article><p>A link-discovered variant of the about page, \
             declaring the sitemap's URL as its own canonical.</p>\
             </article></body></html>"
        ))
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    robots.assert_async().await;
    sitemap.assert_async().await;
    index.assert_async().await;
    about.assert_async().await;
    aboutus.assert_async().await;

    assert_eq!(res.total_pages, 2, "the root and the sitemap's /about both import");
    assert_eq!(res.duplicate_pages, 1, "the link-discovered /aboutus is the duplicate");

    assert!(tmp.path().join("about.md").exists(), "the sitemap's own URL must survive");
    assert!(
        !tmp.path().join("aboutus.md").exists(),
        "the link-discovered duplicate must never reach disk"
    );
}

#[tokio::test]
async fn no_sitemap_gives_unchanged_behavior() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let robots = server.mock("GET", "/robots.txt").with_status(404).create_async().await;
    let sitemap = server.mock("GET", "/sitemap.xml").with_status(404).create_async().await;
    let index = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><body><article><p>An ordinary root page with one ordinary \
             link, long enough to be extracted as content.</p>\
             <a href=\"{base}/a\">A</a></article></body></html>"
        ))
        .create_async()
        .await;
    let a = server
        .mock("GET", "/a")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>The one linked child page, long enough \
             to be extracted as content.</p></article></body></html>",
        )
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    robots.assert_async().await;
    sitemap.assert_async().await;
    index.assert_async().await;
    a.assert_async().await;

    assert_eq!(res.total_pages, 2, "the root and its one linked child import");
    assert_eq!(res.sitemap_urls, 0, "no sitemap was found");
    assert!(!res.sitemap_truncated);
    assert!(!res.capped);
    assert_eq!(res.duplicate_pages, 0);
    assert_eq!(res.failed_pages, 0);
}

/// The sitemap protocol lets a site serve its sitemap gzip-compressed
/// (`sitemap.xml.gz`) with no `Content-Encoding` header — ureq itself is
/// never built with gzip transfer-encoding support here, so this is the
/// only path that ever needs to inflate one. Detected by the gzip magic
/// bytes, not the `.gz` suffix, so this also covers a server that names
/// the compressed file plain `sitemap.xml`.
#[tokio::test]
async fn a_gzip_compressed_sitemap_is_still_discovered() {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let sitemap_xml = format!(
        "<?xml version=\"1.0\"?>\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\
         <url><loc>{base}/unlinked</loc></url>\
         </urlset>"
    );
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(sitemap_xml.as_bytes()).unwrap();
    let gz_body = encoder.finish().unwrap();

    let robots = server
        .mock("GET", "/robots.txt")
        .with_status(200)
        .with_body(format!("Sitemap: {base}/sitemap.xml.gz\n"))
        .create_async()
        .await;
    let sitemap = server
        .mock("GET", "/sitemap.xml.gz")
        .with_status(200)
        .with_header("content-type", "application/gzip")
        .with_body(gz_body)
        .create_async()
        .await;
    let index = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>Root page with no link at all to the \
             gzip-sitemap-only page, long enough to be extracted as content.</p>\
             </article></body></html>",
        )
        .create_async()
        .await;
    let unlinked = server
        .mock("GET", "/unlinked")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>A page declared only by the gzip-compressed \
             sitemap, long enough to be extracted as content.</p>\
             </article></body></html>",
        )
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    robots.assert_async().await;
    sitemap.assert_async().await;
    index.assert_async().await;
    unlinked.assert_async().await;

    assert_eq!(res.total_pages, 2, "the root and the gzip sitemap's page both import");
    assert_eq!(res.sitemap_urls, 1);
    assert!(tmp.path().join("unlinked.md").exists());
}

/// Regression guard: one corpus site's sitemap index lists a per-locale
/// sitemap for each UI language, and each `<url>` entry names its
/// sibling-locale URL via the sitemap protocol's own `hreflang`
/// extension — the exact shape found on a real site while building this
/// fix, where `/en/post` is a UI-language toggle over the SAME article
/// body as `/post`, each self-canonical (so canonical-identity dedup,
/// rule 1, does not collapse them on its own). Seeding both sitemaps'
/// URLs unfiltered would double-import the whole site. `/en/post` must
/// never even be fetched.
#[tokio::test]
async fn sitemap_locale_alternates_collapse_to_one_representative() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let robots = server
        .mock("GET", "/robots.txt")
        .with_status(200)
        .with_body(format!("Sitemap: {base}/sitemap-index.xml\n"))
        .create_async()
        .await;
    let sitemap_index = server
        .mock("GET", "/sitemap-index.xml")
        .with_status(200)
        .with_header("content-type", "application/xml")
        .with_body(format!(
            "<?xml version=\"1.0\"?>\
             <sitemapindex xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\
             <sitemap><loc>{base}/zh-sitemap.xml</loc></sitemap>\
             <sitemap><loc>{base}/en-sitemap.xml</loc></sitemap>\
             </sitemapindex>"
        ))
        .create_async()
        .await;
    let zh_sitemap = server
        .mock("GET", "/zh-sitemap.xml")
        .with_status(200)
        .with_header("content-type", "application/xml")
        .with_body(format!(
            "<?xml version=\"1.0\"?>\
             <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" \
             xmlns:xhtml=\"http://www.w3.org/1999/xhtml\">\
             <url><loc>{base}/post</loc>\
             <xhtml:link rel=\"alternate\" hreflang=\"en\" href=\"{base}/en/post\"/>\
             </url></urlset>"
        ))
        .create_async()
        .await;
    let en_sitemap = server
        .mock("GET", "/en-sitemap.xml")
        .with_status(200)
        .with_header("content-type", "application/xml")
        .with_body(format!(
            "<?xml version=\"1.0\"?>\
             <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" \
             xmlns:xhtml=\"http://www.w3.org/1999/xhtml\">\
             <url><loc>{base}/en/post</loc>\
             <xhtml:link rel=\"alternate\" hreflang=\"zh\" href=\"{base}/post\"/>\
             </url></urlset>"
        ))
        .create_async()
        .await;
    let index = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>Home page, unrelated to the locale-mirrored \
             article, long enough to be extracted as content.</p>\
             </article></body></html>",
        )
        .create_async()
        .await;
    let post = server
        .mock("GET", "/post")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>The article body, in its original \
             language, long enough to be extracted as content.</p>\
             </article></body></html>",
        )
        .create_async()
        .await;
    // The UI-language mirror must never be fetched at all — collapsed
    // away before the crawl's own queue is ever seeded.
    let en_post = server.mock("GET", "/en/post").expect(0).create_async().await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    robots.assert_async().await;
    sitemap_index.assert_async().await;
    zh_sitemap.assert_async().await;
    en_sitemap.assert_async().await;
    index.assert_async().await;
    post.assert_async().await;
    en_post.assert_async().await;

    assert_eq!(res.total_pages, 2, "the home page and the one representative article");
    assert_eq!(res.sitemap_urls, 1, "the locale mirror collapses into its sibling");
    assert!(
        !tmp.path().join("en/post.md").exists(),
        "the UI-language mirror must never be written"
    );
}

/// The regression the collapse alone didn't fix: a UI-language toggle
/// link, present on every page of the ORIGINAL locale, discovers the
/// same mirror the sitemap already named as an alternate — regardless
/// of collapsing the SEED list, a plain link-following crawl reached it
/// anyway. It must never be fetched via this path either.
#[tokio::test]
async fn a_link_discovered_locale_alternate_is_never_fetched() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let robots = server
        .mock("GET", "/robots.txt")
        .with_status(200)
        .with_body(format!("Sitemap: {base}/sitemap.xml\n"))
        .create_async()
        .await;
    let sitemap = server
        .mock("GET", "/sitemap.xml")
        .with_status(200)
        .with_header("content-type", "application/xml")
        .with_body(format!(
            "<?xml version=\"1.0\"?>\
             <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" \
             xmlns:xhtml=\"http://www.w3.org/1999/xhtml\">\
             <url><loc>{base}/post</loc>\
             <xhtml:link rel=\"alternate\" hreflang=\"en\" href=\"{base}/en/post\"/>\
             </url></urlset>"
        ))
        .create_async()
        .await;
    // The home page carries the same per-page language-toggle link a
    // real bilingual site puts on every page — an ordinary, in-scope,
    // crawlable link straight to the mirror the sitemap already named.
    let index = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><body><article><p>Home page linking to the language \
             toggle, long enough to be extracted as content.</p>\
             <a href=\"{base}/en/post\">EN</a></article></body></html>"
        ))
        .create_async()
        .await;
    let post = server
        .mock("GET", "/post")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>The article body, in its original \
             language, long enough to be extracted as content.</p>\
             </article></body></html>",
        )
        .create_async()
        .await;
    // Must never be fetched via the link either, not just unseeded.
    let en_post = server.mock("GET", "/en/post").expect(0).create_async().await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    robots.assert_async().await;
    sitemap.assert_async().await;
    index.assert_async().await;
    post.assert_async().await;
    en_post.assert_async().await;

    assert_eq!(res.total_pages, 2, "the home page and the one representative article");
    assert_eq!(
        res.duplicate_pages, 1,
        "the link-discovered mirror counts as a duplicate, once"
    );
    assert!(
        !tmp.path().join("en/post.md").exists(),
        "the link-discovered mirror must never be written"
    );
}

/// Negative case: the block list is exactly what the sitemap declares as
/// an alternate, never a `/en/`-prefix heuristic. A second English page
/// with no declared `zh` counterpart — linked from the same home page
/// that links to the real alternate — must still be crawled normally.
#[tokio::test]
async fn a_link_to_a_non_alternate_page_under_the_same_locale_prefix_is_still_followed() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    let robots = server
        .mock("GET", "/robots.txt")
        .with_status(200)
        .with_body(format!("Sitemap: {base}/sitemap.xml\n"))
        .create_async()
        .await;
    let sitemap = server
        .mock("GET", "/sitemap.xml")
        .with_status(200)
        .with_header("content-type", "application/xml")
        .with_body(format!(
            "<?xml version=\"1.0\"?>\
             <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" \
             xmlns:xhtml=\"http://www.w3.org/1999/xhtml\">\
             <url><loc>{base}/post</loc>\
             <xhtml:link rel=\"alternate\" hreflang=\"en\" href=\"{base}/en/post\"/>\
             </url></urlset>"
        ))
        .create_async()
        .await;
    let index = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(format!(
            "<html><body><article><p>Home page linking to an English-only \
             page the sitemap never names as anyone's alternate, long \
             enough to be extracted as content.</p>\
             <a href=\"{base}/en/other\">English-only page</a></article></body></html>"
        ))
        .create_async()
        .await;
    let post = server
        .mock("GET", "/post")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>The article body, in its original \
             language, long enough to be extracted as content.</p>\
             </article></body></html>",
        )
        .create_async()
        .await;
    // Same `/en/` prefix as the real alternate, but never declared as
    // one — must be fetched normally, proving the rule isn't a prefix
    // heuristic.
    let en_other = server
        .mock("GET", "/en/other")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>An English-only page with no zh \
             counterpart in the sitemap, long enough to be extracted.</p>\
             </article></body></html>",
        )
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{base}/"), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    robots.assert_async().await;
    sitemap.assert_async().await;
    index.assert_async().await;
    post.assert_async().await;
    en_other.assert_async().await;

    assert_eq!(res.total_pages, 3, "the home page, the article, and the non-alternate page");
    assert_eq!(res.duplicate_pages, 0, "nothing here is a declared duplicate");
    assert!(
        tmp.path().join("en/other.md").exists(),
        "a page under the same prefix but not a declared alternate must still import"
    );
}

/// Widgets found while composing a page reach the run summary: a hosted
/// iframe is carried as a link, a contact form with no address anywhere is
/// dropped and counted — and neither leaves a trace in the written page.
#[tokio::test]
async fn widget_counts_reach_the_scrape_result() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();
    let index = server
        .mock("GET", "/")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(
            "<html><body><article><p>Real page body, long enough to be extracted as content \
             by the scorer without any trouble at all.</p>\
             <iframe src=\"https://pay.example/donate/123\" title=\"Give\"></iframe>\
             <form action=\"/send\"><input name=\"name\"><textarea name=\"m\"></textarea></form>\
             </article></body></html>",
        )
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let res = scrape_to_folder(ScrapeConfig::new(format!("{base}/"), tmp.path()), |_| {})
        .await
        .expect("the crawl itself succeeds");
    index.assert_async().await;

    assert_eq!((res.widgets_carried, res.widgets_dropped), (1, 1));
    let note = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .expect("the page is written");
    let body = std::fs::read_to_string(note).unwrap();
    assert!(body.contains("[Give](https://pay.example/donate/123)"), "{body}");
    assert!(!body.contains("<form"), "{body}");
}

/// A recursive crawl ends with `children: false` on the pages that are folder
/// indexes — the root, and `events` because `events/` exists — and nowhere
/// else. The folder is written after its page here (`events.md` lands before
/// `events/2026/one.md`), which is why the decision is made after the crawl.
#[tokio::test]
async fn a_recursive_crawl_marks_folder_indexes_children_false_and_leaves_leaves_alone() {
    let mut server = mockito::Server::new_async().await;
    let page = |links: &str| {
        format!(
            "<html><body><article><p>Page body, long enough to be extracted as content \
             by the generic scorer.</p>{links}</article></body></html>"
        )
    };
    let mut mocks = Vec::new();
    for (path, body) in [
        ("/", page("<a href=\"/events\">Events</a> <a href=\"/about\">About</a>")),
        ("/events", page("<a href=\"/events/2026/one\">One</a>")),
        ("/events/2026/one", page("")),
        ("/about", page("")),
    ] {
        mocks.push(
            server
                .mock("GET", path)
                .with_status(200)
                .with_header("content-type", "text/html")
                .with_body(body)
                .create_async()
                .await,
        );
    }

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{}/", server.url()), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    assert_eq!(res.total_pages, 4);

    let read = |rel: &str| std::fs::read_to_string(tmp.path().join(rel)).unwrap();
    for rel in ["index.md", "events.md"] {
        assert!(read(rel).contains("\nchildren: false\n---\n"), "{rel}:\n{}", read(rel));
    }
    for rel in ["about.md", "events/2026/one.md"] {
        assert!(!read(rel).contains("children"), "{rel}:\n{}", read(rel));
    }
}

/// No page declares its site name (no `og:site_name`, no JSON-LD), but the home
/// page's title is the name and every other page's `<title>` ends in it — the
/// crawl strips it from those once it has seen them all.
#[tokio::test]
async fn a_title_segment_shared_by_every_crawled_page_is_stripped() {
    let mut server = mockito::Server::new_async().await;
    let page = |title: &str, links: &str| {
        format!(
            "<html><head><title>{title}</title></head><body><article><p>Page body, long \
             enough to be extracted as content by the generic scorer.</p>{links}</article>\
             </body></html>"
        )
    };
    let mut mocks = Vec::new();
    for (path, body) in [
        ("/", page("Studio Name", "<a href=\"/about\">A</a> <a href=\"/press\">P</a>")),
        ("/about", page("About | Studio Name", "")),
        ("/press", page("Press | Studio Name", "")),
    ] {
        mocks.push(
            server
                .mock("GET", path)
                .with_status(200)
                .with_header("content-type", "text/html")
                .with_body(body)
                .create_async()
                .await,
        );
    }

    let tmp = tempfile::tempdir().unwrap();
    let mut config = ScrapeConfig::new(format!("{}/", server.url()), tmp.path());
    config.recursive = true;
    let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
    assert_eq!(res.total_pages, 3);

    for (rel, want) in [("index.md", "Studio Name"), ("about.md", "About"), ("press.md", "Press")] {
        let md = std::fs::read_to_string(tmp.path().join(rel)).unwrap();
        assert!(md.contains(&format!("title: \"{want}\"\n")), "{rel}:\n{md}");
    }
}
