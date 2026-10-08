use super::*;

#[test]
fn transient_error_classification() {
    assert!(should_retry_status(None)); // transport error / timeout
    assert!(should_retry_status(Some(429)));
    assert!(should_retry_status(Some(503)));
    assert!(!should_retry_status(Some(404)));
    assert!(!should_retry_status(Some(200)));
}

#[test]
fn parse_retry_after_accepts_integer_delta_seconds() {
    assert_eq!(
        parse_retry_after("5"),
        Some(std::time::Duration::from_secs(5))
    );
}

#[test]
fn parse_retry_after_caps_at_the_sane_maximum() {
    assert_eq!(parse_retry_after("999999"), Some(MAX_RETRY_AFTER));
}

#[test]
fn parse_retry_after_rejects_a_negative_delta() {
    assert_eq!(parse_retry_after("-5"), None, "falls back to exponential backoff");
}

/// `Retry-After: 0` paired with a 429/503 is a contradiction (the host
/// just refused the request for being too frequent, then asked for no
/// wait at all) — treated as no useful signal, the same as a negative
/// delta or a past date, so the retry still gets the exponential
/// backoff's real delay instead of hammering the host again instantly.
#[test]
fn parse_retry_after_rejects_a_zero_delta() {
    assert_eq!(parse_retry_after("0"), None, "falls back to exponential backoff, not an instant retry");
}

#[test]
fn parse_retry_after_rejects_unparsable_text() {
    assert_eq!(parse_retry_after("not a retry hint"), None);
}

#[test]
fn parse_retry_after_accepts_a_future_http_date_capped_at_the_maximum() {
    // Well past MAX_RETRY_AFTER, so the only way this comes back as
    // exactly the cap is if the HTTP-date branch (not the integer
    // branch, which `"Tue, ..."` can't parse as a number) ran and then
    // was capped — proving both halves of the function at once.
    let far_future = chrono::Utc::now() + chrono::Duration::seconds(3600);
    let header = far_future.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
    assert_eq!(parse_retry_after(&header), Some(MAX_RETRY_AFTER));
}

#[test]
fn parse_retry_after_rejects_a_past_http_date() {
    let past = chrono::Utc::now() - chrono::Duration::seconds(30);
    let header = past.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
    assert_eq!(parse_retry_after(&header), None, "a date already past means retry now, via backoff, not a negative sleep");
}

// ── observe_pace (pure — the wiring between a fetch's own Result and
// the host pacer, independent of any real network or sleep) ──────────

/// `observe_pace` is only the glue that pulls `status` out of
/// `FetchError` and hands it to `HostPacer::observe` — the grow/decay/
/// ignore decision itself is `crawl_state.rs`'s to test. What this file
/// owns is that the status survives the trip intact.
#[test]
fn observe_pace_passes_the_typed_status_through_to_the_pacer() {
    let mut pacer = HostPacer::default();
    let err: Result<(), FetchError> =
        Err(FetchError { status: Some(429), message: "429 Too Many Requests".to_string() });
    observe_pace(&mut pacer, "example.test", &err);
    assert!(pacer.current_interval("example.test") > std::time::Duration::ZERO);
}

/// The gap a string-prefix match on the message would have left: the
/// status travels as DATA `call_with_retry_n` captured straight from
/// `ureq::Error::Status`, so pacing keeps working even when the message
/// text doesn't start with the code at all — a wording change (a
/// translated status line, a reordered message) can't silently disable
/// it the way matching on `message.starts_with("429 ")` could have.
#[test]
fn observe_pace_matches_by_typed_status_not_message_wording() {
    let mut pacer = HostPacer::default();
    let err: Result<(), FetchError> = Err(FetchError {
        status: Some(429),
        message: "fetch failed, server said: too many requests".to_string(),
    });
    observe_pace(&mut pacer, "example.test", &err);
    assert!(
        pacer.current_interval("example.test") > std::time::Duration::ZERO,
        "status is read as typed data, not re-derived from the message text"
    );
}

#[test]
fn test_content_type_to_extension() {
    assert_eq!(content_type_to_extension("image/png"), "png");
    assert_eq!(content_type_to_extension("image/jpeg"), "jpg");
    assert_eq!(content_type_to_extension("video/mp4"), "mp4");
    assert_eq!(content_type_to_extension("unknown/type"), "bin");
}

#[test]
fn test_hash_url_produces_consistent_hash() {
    let url1 = "https://example.com/image.png";
    let url2 = "https://example.com/image.png";
    let url3 = "https://example.com/other.png";

    assert_eq!(hash_url(url1), hash_url(url2));
    assert_ne!(hash_url(url1), hash_url(url3));
}

#[test]
fn test_hash_url_produces_valid_filename() {
    let url = "https://example.com/path/to/image.png?query=value";
    let hash = hash_url(url);

    // Hash should be a valid filename (alphanumeric)
    assert!(hash.chars().all(|c| c.is_alphanumeric()));
    // Should be reasonably short
    assert!(hash.len() <= 16);
}

// ── refuse_unsafe_scrape_url ─────────────────────────────────────────

#[test]
fn refuses_file_scheme() {
    let err = refuse_unsafe_scrape_url("file:///etc/passwd")
        .expect_err("file:// must never be accepted");
    assert!(err.contains("scheme"), "{err}");
}

#[test]
fn refuses_loopback_ipv4() {
    let err = refuse_unsafe_scrape_url("http://127.0.0.1:8080/admin")
        .expect_err("a loopback URL must be refused");
    assert!(err.contains("loopback"), "{err}");
}

#[test]
fn refuses_loopback_ipv6() {
    refuse_unsafe_scrape_url("http://[::1]/").expect_err("::1 must be refused");
}

#[test]
fn refuses_ipv4_mapped_loopback() {
    // ::ffff:127.0.0.1 — a common bypass for a check that only inspects
    // the plain IPv6 loopback form.
    refuse_unsafe_scrape_url("http://[::ffff:127.0.0.1]/")
        .expect_err("an IPv4-mapped loopback literal must be refused");
}

#[test]
fn refuses_link_local() {
    // 169.254.169.254 — the AWS/GCP/Azure instance-metadata address, the
    // canonical SSRF target.
    let err = refuse_unsafe_scrape_url("http://169.254.169.254/latest/meta-data/")
        .expect_err("a link-local URL must be refused");
    assert!(err.contains("link-local"), "{err}");
}

#[test]
fn refuses_localhost_hostname() {
    refuse_unsafe_scrape_url("http://localhost/").expect_err("localhost must be refused");
}

#[test]
fn allows_a_plain_https_url() {
    assert!(refuse_unsafe_scrape_url("https://example.com/blog").is_ok());
}

#[test]
fn allows_a_plain_http_url() {
    assert!(refuse_unsafe_scrape_url("http://example.com/blog").is_ok());
}

// ── fetch_with_validated_redirects (SSRF via redirect) ──────────────
//
// `refuse_unsafe_scrape_url` alone only checks the URL the caller typed
// — a public host that PASSES that check can still 302/303/307/308 to a
// loopback/link-local target, and ureq's own redirect-follower (what
// `proxied_ureq_agent`'s 5-redirect default drives) would walk straight
// into it with no re-check at all. These tests exercise the manual
// follower instead, using a mockito server as the "allowed host" that
// issues the redirect — mockito binds to 127.0.0.1, but that is the
// server ISSUING the 3xx, never a target these tests ask the follower to
// fetch, so it never trips `refuse_unsafe_scrape_url` itself.

fn no_redirect_test_agent(url: &str) -> ureq::Agent {
    crate::system::proxy::proxied_ureq_agent_no_redirects(
        url,
        std::time::Duration::from_secs(5),
    )
}

/// The detail this module's own refusals carry — `ureq::Error::Status`'s
/// `Display` only ever prints `"{url}: status code {code}"` (see
/// `ureq::error::Error`'s impl), so a test asserting on the human-
/// readable reason must read it off the wrapped `Response` directly.
fn refusal_detail(err: &ureq::Error) -> String {
    match err {
        ureq::Error::Status(_, resp) => resp.status_text().to_string(),
        other => other.to_string(),
    }
}

#[tokio::test]
async fn a_redirect_to_loopback_is_refused() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/start")
        .with_status(302)
        .with_header("Location", "http://127.0.0.1:1/internal")
        .create_async()
        .await;

    let start = format!("{}/start", server.url());
    let agent = no_redirect_test_agent(&start);
    let err = fetch_with_validated_redirects(&agent, &start, "test-agent", None)
        .expect_err("a redirect to a loopback address must be refused");
    // Not just `.expect_err(...)`: an ablated per-hop check would still
    // ATTEMPT the connection to 127.0.0.1:1 and get a real connection
    // error back (nothing listens there in the sandbox) — an `Err` for
    // the wrong reason. Asserting the detail is what actually
    // distinguishes "the check refused this" from "the network did".
    assert!(refusal_detail(&err).contains("disallowed"), "{}", refusal_detail(&err));
}

#[tokio::test]
async fn a_redirect_to_link_local_metadata_is_refused() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/start")
        .with_status(302)
        .with_header("Location", "http://169.254.169.254/latest/meta-data/")
        .create_async()
        .await;

    let start = format!("{}/start", server.url());
    let agent = no_redirect_test_agent(&start);
    let err = fetch_with_validated_redirects(&agent, &start, "test-agent", None)
        .expect_err("a redirect to the cloud-metadata address must be refused");
    // Asserting the detail (not just "some Err") matters here: an
    // ablated check would still connect-fail refusing this unroutable
    // address in most sandboxes, which is an Err for the wrong reason —
    // see `a_redirect_to_loopback_is_refused`'s comment.
    assert!(refusal_detail(&err).contains("disallowed"), "{}", refusal_detail(&err));
}

#[tokio::test]
async fn a_redirect_to_ipv6_loopback_is_refused() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/start")
        .with_status(302)
        .with_header("Location", "http://[::1]/internal")
        .create_async()
        .await;

    let start = format!("{}/start", server.url());
    let agent = no_redirect_test_agent(&start);
    let err = fetch_with_validated_redirects(&agent, &start, "test-agent", None)
        .expect_err("a redirect to ::1 must be refused");
    assert!(refusal_detail(&err).contains("disallowed"), "{}", refusal_detail(&err));
}

#[tokio::test]
async fn a_legitimate_redirect_is_still_followed() {
    // Canonicalization (trailing slash, path rewrite — the same shape as
    // an http→https upgrade) must keep working: only a redirect to a
    // DISALLOWED target is refused, not every redirect. Exercises the
    // `fetch_following_redirects` MECHANISM (hop-following, resolving a
    // relative `Location`) with an always-allow validator — every test
    // server available here is itself a loopback address, so the REAL
    // `refuse_unsafe_scrape_url` would refuse this hop regardless of how
    // legitimate it is (that policy is proven separately, on real
    // public URLs, by `allows_a_plain_http(s)_url` above).
    let mut server = mockito::Server::new_async().await;
    let _redirect = server
        .mock("GET", "/old-path")
        .with_status(301)
        .with_header("Location", "/new-path")
        .create_async()
        .await;
    let _target = server
        .mock("GET", "/new-path")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>ok</body></html>")
        .create_async()
        .await;

    let start = format!("{}/old-path", server.url());
    let agent = no_redirect_test_agent(&start);
    let response = fetch_following_redirects(&agent, &start, "test-agent", None, |_| Ok(()))
        .expect("a legitimate redirect must still be followed");
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn a_redirect_chain_longer_than_the_hop_cap_is_refused() {
    let mut server = mockito::Server::new_async().await;
    let mut mocks = Vec::new();
    for i in 0..=MAX_REDIRECT_HOPS {
        mocks.push(
            server
                .mock("GET", format!("/hop{i}").as_str())
                .with_status(302)
                .with_header("Location", &format!("/hop{}", i + 1))
                .create_async()
                .await,
        );
    }

    let start = format!("{}/hop0", server.url());
    let agent = no_redirect_test_agent(&start);
    fetch_with_validated_redirects(&agent, &start, "test-agent", None)
        .expect_err("a chain past MAX_REDIRECT_HOPS must be refused, not followed forever");
}

// ── media Accept header (CDN content negotiation, not SSRF) ─────────
//
// Reproduced against the real `images.squarespace-cdn.com` /
// `static1.squarespace.com` pattern: a request with ureq's own default
// `Accept: */*` comes back `image/webp` even with `?format=original` on
// the URL; the same request with no `Accept` header, or one that never
// names `image/webp`/`image/avif`, gets the real uploaded file back.
// These paths are invented reductions of that host pattern, not a real
// site's URLs.

#[tokio::test]
async fn a_media_download_sends_the_media_accept_header() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/content/v1/abc/def/photo.jpg")
        .match_header("accept", crate::vault::import::media::MEDIA_ACCEPT)
        .with_status(200)
        .with_header("content-type", "image/jpeg")
        .with_body(b"\xff\xd8\xff\xe0fake-jpeg-bytes".as_slice())
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let url = format!("{}/content/v1/abc/def/photo.jpg", server.url());
    download_asset(&url, tmp.path(), "test-agent")
        .await
        .expect("mock only matches the request carrying MEDIA_ACCEPT; an \
                 unmatched mockito request errors instead of 200-ing");
}

#[tokio::test]
async fn a_linked_file_over_the_cap_is_not_written_and_one_under_it_keeps_its_name() {
    let mut server = mockito::Server::new_async().await;
    let _big = server
        .mock("GET", "/files/big.pdf")
        .with_status(200)
        .with_header("content-type", "application/pdf")
        .with_body("0123456789")
        .create_async()
        .await;
    let _small = server
        .mock("GET", "/files/small.pdf")
        .with_status(200)
        .with_header("content-type", "application/pdf")
        .with_body("012345")
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let big = download_linked_file(&format!("{}/files/big.pdf", server.url()), tmp.path(), "t", 8, "big.pdf".into())
        .await
        .unwrap();
    assert!(matches!(big, Downloaded::TooLarge));
    assert!(!tmp.path().join("big.pdf").exists(), "an oversized file is never written");

    let small =
        download_linked_file(&format!("{}/files/small.pdf", server.url()), tmp.path(), "t", 8, "small.pdf".into())
            .await
            .unwrap();
    assert!(matches!(small, Downloaded::Saved(name) if name == "small.pdf"));
}

#[test]
fn a_linked_file_keeps_the_name_its_url_gave_it() {
    assert_eq!(linked_filename("https://example.test/s/Violin%20Part.PDF"), "Violin Part.pdf");
    assert_eq!(linked_filename("https://example.test/f/score.pdf"), "score.pdf");
}

#[tokio::test]
async fn a_page_fetch_keeps_the_default_accept_header() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/article")
        // ureq's own default, unchanged by this fix — a page fetch must
        // never carry MEDIA_ACCEPT.
        .match_header("accept", "*/*")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body("<html><body>ok</body></html>")
        .create_async()
        .await;

    let url = format!("{}/article", server.url());
    fetch_page(&url, "test-agent")
        .await
        .expect("mock only matches Accept: */*; a page fetch must still send it");
}

#[tokio::test]
async fn a_redirect_target_on_a_ruled_host_is_rewritten_before_validation() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/start")
        .with_status(302)
        .with_header(
            "Location",
            "https://images.squarespace-cdn.com/content/v1/abc/def/photo.jpg?format=750w",
        )
        .create_async()
        .await;

    let start = format!("{}/start", server.url());
    let agent = no_redirect_test_agent(&start);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
    let seen_clone = seen.clone();
    // A validator that always refuses: this test is about what URL the
    // rewrite hook hands the validator, not the SSRF policy (proven
    // separately) or an actual connection to a live CDN. `accept:
    // Some(..)` is what an asset download actually passes — this is the
    // only caller the rewrite applies to.
    let err = fetch_following_redirects(
        &agent,
        &start,
        "test-agent",
        Some(crate::vault::import::media::MEDIA_ACCEPT),
        move |u: &str| {
            *seen_clone.lock().unwrap() = Some(u.to_string());
            Err("refused for test".to_string())
        },
    )
    .expect_err("the validator always refuses");
    assert!(refusal_detail(&err).contains("refused for test"));
    assert_eq!(
        seen.lock().unwrap().as_deref(),
        Some("https://images.squarespace-cdn.com/content/v1/abc/def/photo.jpg?format=original"),
        "the redirect target must be canonicalized by original_media_url \
         before it reaches the validator, same as the pre-download rewrite"
    );
}

#[tokio::test]
async fn a_page_fetch_redirect_is_never_rewritten_even_on_a_ruled_host() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/start")
        .with_status(302)
        .with_header(
            "Location",
            "https://images.squarespace-cdn.com/content/v1/abc/def/photo.jpg?format=750w",
        )
        .create_async()
        .await;

    let start = format!("{}/start", server.url());
    let agent = no_redirect_test_agent(&start);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
    let seen_clone = seen.clone();
    // `accept: None` — a page fetch, same as `fetch_page` passes. The
    // rewrite hook must not fire just because the redirect target
    // happens to land on a host the media table rules on.
    let err = fetch_following_redirects(&agent, &start, "test-agent", None, move |u: &str| {
        *seen_clone.lock().unwrap() = Some(u.to_string());
        Err("refused for test".to_string())
    })
    .expect_err("the validator always refuses");
    assert!(refusal_detail(&err).contains("refused for test"));
    assert_eq!(
        seen.lock().unwrap().as_deref(),
        Some("https://images.squarespace-cdn.com/content/v1/abc/def/photo.jpg?format=750w"),
        "a page fetch must see the redirect target exactly as the server sent it, \
         never canonicalized onto the media table's query"
    );
}
