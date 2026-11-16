use super::*;
use crate::system::redirects::MAX_REDIRECT_HOPS;

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
fn refuses_link_local() {
    // 169.254.169.254 — the AWS/GCP/Azure instance-metadata address, the
    // canonical SSRF target.
    let err = refuse_unsafe_scrape_url("http://169.254.169.254/latest/meta-data/")
        .expect_err("a link-local URL must be refused");
    assert!(err.contains("link-local"), "{err}");
}

#[test]
fn refuses_loopback_and_link_local_written_in_any_ipv6_form() {
    for form in [
        "::ffff:127.0.0.1", "::127.0.0.1", "64:ff9b::127.0.0.1",
        "::ffff:169.254.169.254", "::169.254.169.254", "64:ff9b::169.254.169.254",
    ] {
        refuse_unsafe_scrape_url(&format!("http://[{form}]/")).expect_err(form);
    }
    for form in ["::ffff:93.184.216.34", "::93.184.216.34", "64:ff9b::93.184.216.34"] {
        assert!(refuse_unsafe_scrape_url(&format!("http://[{form}]/")).is_ok(), "{form}");
    }
}

#[test]
fn refuses_localhost_hostname() {
    refuse_unsafe_scrape_url("http://localhost/").expect_err("localhost must be refused");
}

#[test]
fn refuses_names_under_localhost_in_any_case() {
    for url in ["http://a.localhost/", "http://LOCALHOST./", "http://App.LocalHost/x"] {
        refuse_unsafe_scrape_url(url).expect_err(url);
    }
}

#[test]
fn refuses_unspecified_and_multicast_literals() {
    for url in ["http://0.0.0.0/", "http://0.1.2.3/", "http://[::]/", "http://224.0.0.1/", "http://[ff02::1]/"] {
        refuse_unsafe_scrape_url(url).expect_err(url);
    }
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
// A public host that passes `refuse_unsafe_scrape_url` can still
// 302/303/307/308 to a loopback/link-local target, and ureq's own
// redirect-follower (what `proxied_ureq_agent`'s 5-redirect default
// drives) would walk straight into it with no re-check at all. These tests
// exercise the manual follower instead, with a `test_server` as the
// allowed host that issues the redirect: it binds to 127.0.0.1, and only
// that registered address is let through, so the hop meets the real policy.

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
    let mut server = test_server().await;
    let _m = server
        .mock("GET", "/start")
        .with_status(302)
        .with_header("Location", "http://127.0.0.1:1/internal")
        .create_async()
        .await;

    let start = format!("{}/start", server.url());
    let agent = no_redirect_test_agent(&start);
    let client = Client::no_own_site();
    let err = fetch_with_validated_redirects(&agent, &start, &client, None, None, None)
        .expect_err("a redirect to a loopback address must be refused");
    assert_eq!(client.refused_report(), ["Not fetched: http://127.0.0.1:1/internal"], "a refused hop is recorded");
    // Not just `.expect_err(...)`: an ablated per-hop check would still
    // ATTEMPT the connection to 127.0.0.1:1 and get a real connection
    // error back (nothing listens there in the sandbox) — an `Err` for
    // the wrong reason. Asserting the detail is what actually
    // distinguishes "the check refused this" from "the network did".
    assert!(refusal_detail(&err).contains("redirected to a disallowed"), "{}", refusal_detail(&err));
}

#[tokio::test]
async fn a_redirect_to_a_mail_address_is_refused_not_a_panic() {
    let mut server = test_server().await;
    let _m = server
        .mock("GET", "/start")
        .with_status(302)
        .with_header("Location", "mailto:someone@example.test")
        .create_async()
        .await;

    let start = format!("{}/start", server.url());
    let agent = no_redirect_test_agent(&start);
    let err = fetch_with_validated_redirects(&agent, &start, &Client::no_own_site(), None, None, None)
        .expect_err("a redirect to a mailto: address must be refused");
    let detail = refusal_detail(&err);
    assert!(detail.contains("redirected to a disallowed") && detail.contains("mailto"), "{detail}");
}

#[tokio::test]
async fn a_refusal_reason_that_is_not_ascii_is_carried_not_a_panic() {
    let mut server = test_server().await;
    let _m = server
        .mock("GET", "/start")
        .with_status(302)
        .with_header("Location", "/next")
        .create_async()
        .await;

    let start = format!("{}/start", server.url());
    let agent = no_redirect_test_agent(&start);
    let err = fetch_following_redirects(&agent, &start, "test-agent", None, None, |u: &str| {
        if u == start { Ok(()) } else { Err("refusé — not here".to_string()) }
    }, |_| false)
    .expect_err("a refused hop must be an error");
    let detail = refusal_detail(&err);
    assert!(detail.is_ascii() && detail.contains("refus") && detail.contains("not here"), "{detail}");
}

#[tokio::test]
async fn a_redirect_to_link_local_metadata_is_refused() {
    let mut server = test_server().await;
    let _m = server
        .mock("GET", "/start")
        .with_status(302)
        .with_header("Location", "http://169.254.169.254/latest/meta-data/")
        .create_async()
        .await;

    let start = format!("{}/start", server.url());
    let agent = no_redirect_test_agent(&start);
    let err = fetch_with_validated_redirects(&agent, &start, &Client::no_own_site(), None, None, None)
        .expect_err("a redirect to the cloud-metadata address must be refused");
    // Asserting the detail (not just "some Err") matters here: an
    // ablated check would still connect-fail refusing this unroutable
    // address in most sandboxes, which is an Err for the wrong reason —
    // see `a_redirect_to_loopback_is_refused`'s comment.
    assert!(refusal_detail(&err).contains("redirected to a disallowed"), "{}", refusal_detail(&err));
}

#[tokio::test]
async fn a_redirect_to_ipv6_loopback_is_refused() {
    let mut server = test_server().await;
    let _m = server
        .mock("GET", "/start")
        .with_status(302)
        .with_header("Location", "http://[::1]/internal")
        .create_async()
        .await;

    let start = format!("{}/start", server.url());
    let agent = no_redirect_test_agent(&start);
    let err = fetch_with_validated_redirects(&agent, &start, &Client::no_own_site(), None, None, None)
        .expect_err("a redirect to ::1 must be refused");
    assert!(refusal_detail(&err).contains("redirected to a disallowed"), "{}", refusal_detail(&err));
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
    let mut server = test_server().await;
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
    let response = fetch_following_redirects(&agent, &start, "test-agent", None, None, |_| Ok(()), |_| false)
        .expect("a legitimate redirect must still be followed");
    assert_eq!(response.status(), 200);
    assert_eq!(response.get_url(), format!("{}/new-path", server.url()), "fetch_page reports this");
}

#[tokio::test]
async fn a_redirect_chain_longer_than_the_hop_cap_is_refused() {
    let mut server = test_server().await;
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
    let client = Client::no_own_site();
    let err = fetch_with_validated_redirects(&agent, &start, &client, None, None, None)
        .expect_err("a chain past MAX_REDIRECT_HOPS must be refused, not followed forever");
    assert!(client.refused_report().is_empty(), "a long chain is no address refusal");
    assert!(refusal_detail(&err).contains("too many redirects"), "{}", refusal_detail(&err));
}

// ── a fetch's first address ─────────────────────────────────────────
//
// Media, linked files, the logo and sitemaps are addresses a page names,
// on any host. Each is refused before its first request, not only when a
// redirect points there.

const LINK_LOCAL: &str = "http://169.254.0.1";

/// The refusal of a first address, as opposed to a refused hop or a
/// connection that failed.
fn refused_before_any_request(message: &str) -> bool {
    message.starts_with("400 disallowed URL: ")
}

#[tokio::test]
async fn a_first_address_the_validator_refuses_is_never_requested() {
    let (address, hits) = unregistered_loopback_server();
    let start = format!("{address}/page");
    let agent = no_redirect_test_agent(&start);
    let err = crate::system::redirects::get_following_redirects(
        &agent,
        &start,
        "test-agent",
        None,
        None,
        |u: &str| if u == start { Err("not this one".to_string()) } else { Ok(()) },
        |next| next,
        |_| false,
    )
    .expect_err("the validator refuses the first address");
    assert_eq!(refusal_detail(&err), "disallowed URL: not this one");
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_media_download_on_loopback_or_link_local_is_refused_unrequested() {
    let (loopback, hits) = unregistered_loopback_server();
    let tmp = tempfile::tempdir().unwrap();
    for base in [loopback.as_str(), LINK_LOCAL] {
        let client = Client::no_own_site();
        let err = download_asset(&format!("{base}/a.png"), tmp.path(), &client, Some("https://pages.example.test/"))
            .await
            .expect_err(base);
        assert!(refused_before_any_request(&err.to_string()), "{err}");
        assert_eq!(client.refused_report(), [format!("Not fetched: {base}/a.png")]);
    }
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_linked_file_on_loopback_or_link_local_is_refused_unrequested() {
    let (loopback, hits) = unregistered_loopback_server();
    let tmp = tempfile::tempdir().unwrap();
    for base in [loopback.as_str(), LINK_LOCAL] {
        let client = Client::no_own_site();
        let result = download_linked_file(&format!("{base}/f.pdf"), tmp.path(), &client, None, 8, "f.pdf".into()).await;
        let err = result.err().expect(base);
        assert!(refused_before_any_request(&err.to_string()), "{err}");
        assert_eq!(client.refused_report(), [format!("Not fetched: {base}/f.pdf")]);
    }
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(!tmp.path().join("f.pdf").exists());
}

#[tokio::test]
async fn a_raw_fetch_on_loopback_or_link_local_is_refused_unrequested() {
    let (loopback, hits) = unregistered_loopback_server();
    for base in [loopback.as_str(), LINK_LOCAL] {
        let client = Client::no_own_site();
        let err = fetch_raw_bytes(&format!("{base}/sitemap.xml"), &client).await.expect_err(base);
        assert!(refused_before_any_request(&err.to_string()), "{err}");
        assert_eq!(client.refused_report(), [format!("Not fetched: {base}/sitemap.xml")]);
    }
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
}

// ── the person's own site ───────────────────────────────────────────

#[test]
fn a_private_literal_passes_only_as_the_address_the_person_gave() {
    let client = Client::new(Some("http://192.168.1.5:8080/blog"));
    assert!(client.check("http://192.168.1.5:8080/page").is_ok());
    assert!(client.check("http://192.168.1.6:8080/page").is_err(), "another address");
    assert!(client.check("http://192.168.1.5:9090/page").is_err(), "the same address on another port");
    assert!(client.check("https://example.test/page").is_ok(), "names and public addresses are not the rule's business");
    assert!(Client::no_own_site().check("http://192.168.1.5:8080/page").is_err());
    assert!(Client::new(Some("not an address")).check("http://192.168.1.5/").is_err());
    assert!(Client::new(Some("ftp://192.168.1.5/")).check("http://192.168.1.5/").is_err());
}

#[test]
fn the_own_site_is_matched_by_address_value_and_port_not_by_spelling() {
    let client = Client::new(Some("http://192.168.1.5/"));
    assert!(client.check("http://[::ffff:192.168.1.5]/x").is_ok(), "the same address written as IPv6");
    assert!(client.check("https://192.168.1.5/x").is_ok(), "the default ports of http and https are one");
    assert!(client.check("http://192.168.1.5:81/x").is_err());
    let own = Client::new(Some("http://[::ffff:10.0.0.2]:3000/"));
    assert!(own.check("http://10.0.0.2:3000/").is_ok());
}

#[test]
fn a_login_in_the_own_site_address_is_not_kept() {
    let client = Client::new(Some("http://writer:secret@192.168.1.5:3000/"));
    assert!(client.check("http://192.168.1.5:3000/").is_ok());
    assert!(!format!("{client:?}").contains("secret"));
    assert_eq!(client.local_host(), Some("192.168.1.5"));
}

#[test]
fn only_a_private_literal_own_site_is_local() {
    assert_eq!(Client::new(Some("http://10.0.0.7/")).local_host(), Some("10.0.0.7"));
    assert_eq!(Client::new(Some("http://100.64.0.9:80/")).local_host(), Some("100.64.0.9"));
    for own in [Some("http://93.184.216.34/"), Some("https://example.test/"), Some("http://127.0.0.1/"), None] {
        assert_eq!(Client::new(own).local_host(), None, "{own:?}");
    }
}

#[test]
fn the_report_names_twenty_refused_addresses_and_counts_the_rest() {
    let client = Client::no_own_site();
    for i in 1..=25 {
        client.check(&format!("http://10.0.0.{i}/")).expect_err("private");
    }
    let report = client.refused_report();
    assert_eq!(report.len(), 21, "{report:?}");
    assert!(report[..20].iter().all(|l| l.starts_with("Not fetched: http://10.0.0.")));
    assert_eq!(report[20], "Not fetched: 5 more addresses.");
}

#[test]
fn a_refused_address_is_named_once_and_only_a_thousand_are_kept() {
    let client = Client::no_own_site();
    client.check("http://10.0.0.1/a?x=1").unwrap_err();
    client.check("http://10.0.0.1/a?x=2").unwrap_err();
    assert_eq!(client.refused_report(), ["Not fetched: http://10.0.0.1/a"]);
    let signed = Client::no_own_site();
    signed.check("http://reader:secret@10.0.0.2/a#top").unwrap_err();
    assert_eq!(signed.refused_report(), ["Not fetched: http://10.0.0.2/a"], "sign-in details never reach the report");
    for i in 0..1100 {
        client.check(&format!("http://10.1.{}.{}/", i / 250, i % 250)).unwrap_err();
    }
    assert_eq!(client.refused.lock().unwrap().len(), 1000);
}

#[tokio::test]
async fn the_own_site_redirecting_to_another_private_address_is_not_followed() {
    let mut p = private_pair().await;
    let _m = p.own.mock("GET", "/start").with_status(302).with_header("Location", &format!("{}/x", p.other.url())).create_async().await;
    let target = p.other.mock("GET", "/x").expect(0).create_async().await;
    let start = format!("{}/start", p.own.url());
    let agent = no_redirect_test_agent(&start);

    let err = fetch_with_validated_redirects(&agent, &start, &p.client, None, None, None).expect_err("the hop is another private address");

    assert!(refusal_detail(&err).contains("redirected to a disallowed"), "{}", refusal_detail(&err));
    target.assert_async().await;
    assert_eq!(p.client.refused_report(), [format!("Not fetched: {}/x", p.other.url())]);
}

#[tokio::test]
async fn the_own_site_redirecting_to_itself_is_followed() {
    let mut p = private_pair().await;
    let _old = p.own.mock("GET", "/old").with_status(301).with_header("Location", "/new").create_async().await;
    let _new = p.own.mock("GET", "/new").with_status(200).with_body("ok").create_async().await;
    let start = format!("{}/old", p.own.url());
    let agent = no_redirect_test_agent(&start);

    let response = fetch_with_validated_redirects(&agent, &start, &p.client, None, None, None).expect("a hop on the own site");

    assert_eq!(response.get_url(), format!("{}/new", p.own.url()));
    assert!(p.client.refused_report().is_empty());
}

#[tokio::test]
async fn a_linked_file_is_fetched_from_the_own_site_and_not_from_another_private_address() {
    let mut p = private_pair().await;
    let own = p.own.mock("GET", "/f.pdf").with_header("content-type", "application/pdf").with_body("012345").expect(1).create_async().await;
    let other = p.other.mock("GET", "/g.pdf").with_header("content-type", "application/pdf").with_body("012345").expect(0).create_async().await;
    let tmp = tempfile::tempdir().unwrap();

    let saved = download_linked_file(&format!("{}/f.pdf", p.own.url()), tmp.path(), &p.client, None, 100, "f.pdf".into()).await.unwrap();
    let err = download_linked_file(&format!("{}/g.pdf", p.other.url()), tmp.path(), &p.client, None, 100, "g.pdf".into()).await.err().expect("another private address");

    assert!(matches!(saved, Downloaded::Saved(name) if name == "f.pdf"));
    assert!(refused_before_any_request(&err.to_string()), "{err}");
    own.assert_async().await;
    other.assert_async().await;
    assert_eq!(p.client.refused_report(), [format!("Not fetched: {}/g.pdf", p.other.url())]);
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
    let mut server = test_server().await;
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
    download_asset(&url, tmp.path(), &Client::no_own_site(), None)
        .await
        .expect("mock only matches the request carrying MEDIA_ACCEPT; an \
                 unmatched mockito request errors instead of 200-ing");
}

#[tokio::test]
async fn a_media_download_sends_the_pages_origin_as_referer() {
    let mut server = test_server().await;
    let _m = server
        .mock("GET", "/img/a.jpg")
        .match_header("referer", "http://pages.example.test/")
        .with_status(200)
        .with_header("content-type", "image/jpeg")
        .with_body(b"\xff\xd8\xff\xe0fake-jpeg-bytes".as_slice())
        .create_async()
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let url = format!("{}/img/a.jpg", server.url());
    download_asset(&url, tmp.path(), &Client::no_own_site(), Some("http://pages.example.test/notes/1?x=2#top"))
        .await
        .expect("mock only matches a request carrying the page origin as Referer");
}

#[test]
fn referer_is_the_origin_and_never_sent_on_a_downgrade() {
    assert_eq!(
        referer_for("https://pages.example.test/a/b?q=1", "https://cdn.example.test/x.jpg").as_deref(),
        Some("https://pages.example.test/")
    );
    assert_eq!(referer_for("https://pages.example.test/a", "http://cdn.example.test/x.jpg"), None);
    assert_eq!(referer_for("file:///tmp/a.html", "https://cdn.example.test/x.jpg"), None);
    assert_eq!(referer_for("", "https://cdn.example.test/x.jpg"), None);
}

#[tokio::test]
async fn a_linked_file_over_the_cap_is_not_written_and_one_under_it_keeps_its_name() {
    let mut server = test_server().await;
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
    let big = download_linked_file(&format!("{}/files/big.pdf", server.url()), tmp.path(), &Client::no_own_site(), None, 8, "big.pdf".into())
        .await
        .unwrap();
    assert!(matches!(big, Downloaded::TooLarge));
    assert!(!tmp.path().join("big.pdf").exists(), "an oversized file is never written");

    let small =
        download_linked_file(&format!("{}/files/small.pdf", server.url()), tmp.path(), &Client::no_own_site(), None, 8, "small.pdf".into())
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
    let mut server = test_server().await;
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
    fetch_page(&url, &Client::no_own_site(), None)
        .await
        .expect("mock only matches Accept: */*; a page fetch must still send it");
}

#[tokio::test]
async fn a_redirect_target_on_a_ruled_host_is_rewritten_before_validation() {
    let mut server = test_server().await;
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
        None,
        |u: &str| {
            if u == start {
                return Ok(());
            }
            *seen_clone.lock().unwrap() = Some(u.to_string());
            Err("refused for test".to_string())
        },
        |_| false,
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
    let mut server = test_server().await;
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
    let err = fetch_following_redirects(&agent, &start, "test-agent", None, None, |u: &str| {
        if u == start {
            return Ok(());
        }
        *seen_clone.lock().unwrap() = Some(u.to_string());
        Err("refused for test".to_string())
    }, |_| false)
    .expect_err("the validator always refuses");
    assert!(refusal_detail(&err).contains("refused for test"));
    assert_eq!(
        seen.lock().unwrap().as_deref(),
        Some("https://images.squarespace-cdn.com/content/v1/abc/def/photo.jpg?format=750w"),
        "a page fetch must see the redirect target exactly as the server sent it, \
         never canonicalized onto the media table's query"
    );
}

const S: fn(u64) -> Duration = Duration::from_secs;

#[test]
fn says_nothing_before_thirty_seconds() {
    assert_eq!(progress_line(3, 20, S(10), S(29)), None);
}

#[test]
fn says_nothing_when_the_last_file_just_finished() {
    assert_eq!(progress_line(20, 20, S(10), S(300)), None);
}

#[test]
fn counts_and_estimates_from_the_delay() {
    assert_eq!(
        progress_line(40, 280, S(10), S(30)).unwrap(),
        "    ↳ 40 of 280 files for this page; requests are 10 s apart, about 40 min left"
    );
}

#[test]
fn leaves_the_estimate_out_without_a_delay() {
    assert_eq!(
        progress_line(40, 280, Duration::ZERO, S(45)).unwrap(),
        "    ↳ 40 of 280 files for this page"
    );
}

#[test]
fn short_and_long_estimates_read_naturally() {
    assert!(progress_line(1, 4, S(10), S(30)).unwrap().ends_with("about 30 s left"));
    assert!(progress_line(1, 400, S(30), S(30)).unwrap().ends_with("about 3.3 h left"));
}

#[tokio::test]
async fn a_link_of_the_site_that_leads_off_it_is_reported_not_requested() {
    let mut server = test_server().await;
    let _mail = server
        .mock("GET", "/mail")
        .with_status(302)
        .with_header("Location", "mailto:editor@example.test?subject=Hello there")
        .create_async()
        .await;
    let _away = server
        .mock("GET", "/away")
        .with_status(302)
        .with_header("Location", "https://other.example.test/form?ref=127.0.0.1")
        .create_async()
        .await;
    let _scheme = server
        .mock("GET", "/file")
        .with_status(302)
        .with_header("Location", "file:///etc/hosts")
        .create_async()
        .await;
    let _port = server
        .mock("GET", "/port")
        .with_status(302)
        .with_header("Location", "http://127.0.0.1:1/form")
        .create_async()
        .await;
    let agent = no_redirect_test_agent(&server.url());
    let scope = super::super::scope::UrlScope::new(&format!("{}/", server.url())).unwrap();
    let destination = |path: &str| {
        let start = format!("{}{path}", server.url());
        fetch_with_validated_redirects(&agent, &start, &Client::no_own_site(), None, None, Some(&scope))
            .map(|r| crate::system::redirects::redirect_target(&r))
    };

    assert_eq!(destination("/mail").unwrap().as_deref(), Some("mailto:editor@example.test?subject=Hello%20there"));
    assert_eq!(destination("/away").unwrap().as_deref(), Some("https://other.example.test/form?ref=127.0.0.1"));
    assert_eq!(destination("/port").unwrap().as_deref(), Some("http://127.0.0.1:1/form"), "another port is off the site");
    let err = destination("/file").expect_err("any other scheme is still refused");
    assert!(refusal_detail(&err).contains("redirected to a disallowed"), "{}", refusal_detail(&err));
}

#[tokio::test]
async fn a_redirect_that_returns_to_an_address_it_visited_is_a_loop() {
    let mut server = test_server().await;
    let _a = server.mock("GET", "/a").with_status(302).with_header("Location", "/b").create_async().await;
    let _b = server.mock("GET", "/b").with_status(302).with_header("Location", "/a").create_async().await;

    let start = format!("{}/a", server.url());
    let agent = no_redirect_test_agent(&start);
    let err = fetch_following_redirects(&agent, &start, "test-agent", None, None, |_| Ok(()), |_| false)
        .expect_err("a loop must be refused");
    assert!(refusal_detail(&err).contains("itself in a loop"), "{}", refusal_detail(&err));
}

// ── the check at connect ────────────────────────────────────────────

fn answers(ips: &[&str], port: u16) -> Vec<std::net::SocketAddr> {
    ips.iter().map(|ip| std::net::SocketAddr::new(ip.parse().unwrap(), port)).collect()
}

fn kept(client: &Client, host: &str, port: u16, ips: &[&str]) -> std::io::Result<Vec<String>> {
    client.admitted(host, port, answers(ips, port)).map(|kept| kept.iter().map(|a| a.ip().to_string()).collect())
}

fn is_refusal(err: &std::io::Error) -> bool {
    err.get_ref().is_some_and(|inner| inner.is::<Refused>())
}

#[test]
fn a_public_answer_passes_and_every_other_class_is_cut() {
    let client = Client::no_own_site();
    assert_eq!(kept(&client, "example.test", 443, &["93.184.216.34", "2606:2800:220:1::1"]).unwrap(), ["93.184.216.34", "2606:2800:220:1::1"]);
    for ip in ["127.0.0.1", "::1", "169.254.169.254", "fe80::1", "0.0.0.0", "::", "224.0.0.1", "ff02::1", "10.0.0.7", "100.64.0.9", "fd00::1"] {
        assert!(is_refusal(&kept(&client, "example.test", 80, &[ip]).unwrap_err()), "{ip}");
    }
}

#[test]
fn an_answer_in_a_mapped_form_is_judged_as_the_address_it_carries() {
    let client = Client::no_own_site();
    for ip in ["::ffff:127.0.0.1", "::ffff:169.254.169.254", "::ffff:10.0.0.7", "64:ff9b::192.168.1.1"] {
        assert!(is_refusal(&kept(&client, "example.test", 80, &[ip]).unwrap_err()), "{ip}");
    }
    assert!(kept(&client, "example.test", 80, &["::ffff:93.184.216.34"]).is_ok());
}

#[test]
fn a_mixed_answer_is_cut_down_and_one_with_nothing_left_carries_the_refusal() {
    let client = Client::no_own_site();
    assert_eq!(kept(&client, "example.test", 80, &["127.0.0.1", "93.184.216.34", "10.0.0.7"]).unwrap(), ["93.184.216.34"]);
    let err = kept(&client, "example.test", 80, &["127.0.0.1", "10.0.0.7"]).unwrap_err();
    assert!(is_refusal(&err), "{err}");
    assert!(err.to_string().contains("Loopback"), "{err}");
    assert!(kept(&client, "example.test", 80, &[]).unwrap().is_empty(), "no answers at all is a failed lookup, not a refusal");
}

#[test]
fn an_own_name_is_fixed_by_its_first_answer_on_its_own_port() {
    let client = Client::new(Some("http://nas.test/"));
    assert_eq!(client.local_host(), None, "nothing is known before the lookup");
    assert_eq!(kept(&client, "nas.test", 80, &["192.168.1.5", "192.168.1.6"]).unwrap(), ["192.168.1.5", "192.168.1.6"]);
    assert_eq!(client.local_host(), Some("nas.test"));
    assert!(kept(&client, "nas.test", 80, &["192.168.1.9"]).is_err(), "a later answer is not the site the person meant");
    assert!(kept(&client, "nas.test", 443, &["192.168.1.5"]).is_ok(), "80, 443 and none written are one port");
    assert!(kept(&client, "nas.test", 8080, &["192.168.1.5"]).is_err(), "the own address on another port");
    assert!(kept(&client, "other.test", 8080, &["10.0.0.7"]).is_err());
}

#[test]
fn a_second_name_on_an_own_address_passes_by_the_same_rule() {
    let client = Client::new(Some("http://nas.test/"));
    kept(&client, "nas.test", 80, &["192.168.1.5"]).unwrap();
    assert!(kept(&client, "www.nas.test", 80, &["192.168.1.5"]).is_ok());
    assert!(kept(&client, "www.nas.test", 80, &["::ffff:192.168.1.5"]).is_ok());
    assert!(kept(&client, "www.nas.test", 80, &["192.168.1.6"]).is_err());
}

#[test]
fn an_own_name_written_another_way_is_still_the_own_name() {
    for (typed, looked_up) in [("http://NAS.Test/", "nas.test"), ("http://nas.test./", "nas.test"), ("http://nas.test/", "NAS.test."), ("https://nas.test:443/", "nas.test")] {
        let client = Client::new(Some(typed));
        assert!(kept(&client, looked_up, 80, &["192.168.1.5"]).is_ok(), "{typed} {looked_up}");
        assert!(client.local_host().is_some(), "{typed} {looked_up}");
    }
}

#[test]
fn an_empty_first_answer_does_not_fix_the_own_addresses() {
    let client = Client::new(Some("http://nas.test/"));
    kept(&client, "nas.test", 80, &[]).unwrap();
    assert!(kept(&client, "nas.test", 80, &["192.168.1.5"]).is_ok());
    assert_eq!(client.local_host(), Some("nas.test"));
}

#[test]
fn a_public_own_name_is_no_local_site() {
    let client = Client::new(Some("http://example.test/"));
    kept(&client, "example.test", 80, &["93.184.216.34"]).unwrap();
    assert_eq!(client.local_host(), None);
    assert!(kept(&client, "other.test", 80, &["10.0.0.7"]).is_err());
}

#[test]
fn a_literal_own_site_admits_its_address_under_any_name_on_its_port() {
    let client = Client::new(Some("http://192.168.1.5:8080/"));
    assert!(kept(&client, "printer.test", 8080, &["192.168.1.5"]).is_ok());
    assert!(kept(&client, "printer.test", 8080, &["::ffff:192.168.1.5"]).is_ok());
    assert!(kept(&client, "printer.test", 9090, &["192.168.1.5"]).is_err());
    assert!(kept(&client, "printer.test", 8080, &["192.168.1.9"]).is_err());
}

fn port_of(server: &mockito::ServerGuard) -> String {
    server.host_with_port().rsplit_once(':').unwrap().1.to_string()
}

#[tokio::test]
async fn an_own_site_given_as_a_name_is_fetched_and_so_is_its_literal_address() {
    use crate::system::address::AddressClass::Private;
    let mut server = test_server_as(Private).await;
    test_name_as("nas.test", &server, Private);
    let page = server.mock("GET", "/page").with_body("ok").expect(2).create_async().await;
    let client = Client::new(Some(&format!("http://nas.test:{}/", port_of(&server))));

    fetch_page(&format!("http://nas.test:{}/page", port_of(&server)), &client, None).await.expect("the own name");
    assert_eq!(client.local_host(), Some("nas.test"));
    fetch_page(&format!("{}/page", server.url()), &client, None).await.expect("the own address, written as a literal");

    page.assert_async().await;
    assert!(client.refused_report().is_empty());
}

#[tokio::test]
async fn a_name_that_resolves_to_a_public_address_is_fetched() {
    let mut server = test_server().await;
    test_name_as("example.test", &server, crate::system::address::AddressClass::Public);
    let page = server.mock("GET", "/page").with_body("ok").expect(1).create_async().await;

    fetch_page(&format!("http://example.test:{}/page", port_of(&server)), &Client::no_own_site(), None).await.expect("public");

    page.assert_async().await;
}

#[tokio::test]
async fn a_name_that_resolves_to_a_private_address_is_not_fetched_by_any_path() {
    use crate::system::address::AddressClass::Private;
    let mut site = test_server().await;
    let mut printer = test_server_as(Private).await;
    test_name_as("printer.test", &printer, Private);
    let hits = printer.mock("GET", mockito::Matcher::Any).expect(0).create_async().await;
    let base = format!("http://printer.test:{}", port_of(&printer));
    let _hop = site.mock("GET", "/hop").with_status(302).with_header("Location", &format!("{base}/target")).create_async().await;
    let client = Client::no_own_site();
    let tmp = tempfile::tempdir().unwrap();

    let page = fetch_page(&format!("{base}/page"), &client, None).await.err().expect("a page link");
    let image = download_asset(&format!("{base}/img.png"), tmp.path(), &client, Some(&site.url())).await.err().expect("an image");
    let sheet = fetch_raw(&format!("{base}/style.css"), &client).await.err().expect("a stylesheet");
    let hop = fetch_page(&format!("{}/hop", site.url()), &client, None).await.err().expect("a redirect hop");

    for err in [page, image, sheet, hop] {
        assert!(refused_before_any_request(&err.to_string()), "{err}");
    }
    hits.assert_async().await;
    assert_eq!(client.refused_report(), [format!("Not fetched: {base}/img.png"), format!("Not fetched: {base}/page"), format!("Not fetched: {base}/style.css"), format!("Not fetched: {base}/target")]);
}

#[tokio::test]
async fn a_name_on_another_private_address_is_not_fetched_as_the_own_site() {
    use crate::system::address::AddressClass::Private;
    let mut own = test_server_as(Private).await;
    test_name_as("nas.test", &own, Private);
    let mut other = test_server_as(Private).await;
    test_name_as("printer.test", &other, Private);
    let _page = own.mock("GET", "/page").with_body("ok").create_async().await;
    let hits = other.mock("GET", mockito::Matcher::Any).expect(0).create_async().await;
    let client = Client::new(Some(&format!("http://nas.test:{}/", port_of(&own))));

    fetch_page(&format!("http://nas.test:{}/page", port_of(&own)), &client, None).await.expect("the own name");
    let err = fetch_page(&format!("http://printer.test:{}/page", port_of(&other)), &client, None).await.err().expect("another private name");

    assert!(refused_before_any_request(&err.to_string()), "{err}");
    hits.assert_async().await;
}

#[tokio::test]
async fn a_refused_name_is_not_retried() {
    use crate::system::address::AddressClass::Private;
    let printer = test_server_as(Private).await;
    test_name_as("printer.test", &printer, Private);
    let started = Instant::now();

    let err = fetch_page(&format!("http://printer.test:{}/page", port_of(&printer)), &Client::no_own_site(), None).await.err().expect("refused");

    assert!(started.elapsed() < BACKOFF_BASE, "{:?}", started.elapsed());
    assert!(refused_before_any_request(&err.to_string()), "{err}");
}

#[tokio::test]
async fn a_failed_lookup_is_still_tried_three_times() {
    test_name_unanswered("gone.test");
    let started = Instant::now();

    let err = fetch_page("http://gone.test/page", &Client::no_own_site(), None).await.err().expect("no such name");

    assert!(started.elapsed() >= BACKOFF_BASE * 3, "{:?}", started.elapsed());
    assert!(!refused_before_any_request(&err.to_string()), "{err}");
}

#[tokio::test]
async fn behind_a_proxy_the_proxy_does_the_lookup() {
    use crate::system::address::AddressClass::Private;
    // A private stand-in: were the hook installed, the proxy's own address would be refused.
    let mut proxy = test_server_as(Private).await;
    let mut printer = test_server_as(Private).await;
    test_name_as("printer.test", &printer, Private);
    let via_proxy = proxy.mock("GET", mockito::Matcher::Any).with_body("via proxy").expect(1).create_async().await;
    let direct = printer.mock("GET", mockito::Matcher::Any).expect(0).create_async().await;
    let url = format!("http://printer.test:{}/page", port_of(&printer));
    let client = Client::no_own_site();
    let timeout = std::time::Duration::from_secs(5);

    let proxied = agent_for(crate::system::proxy::proxied_agent_builder_via(Some(&proxy.url()), timeout, 0), &client);
    let response = fetch_with_validated_redirects(&proxied, &url, &client, None, None, None).expect("the proxy answers");
    assert_eq!(response.into_string().unwrap(), "via proxy");

    let unproxied = agent_for(crate::system::proxy::proxied_agent_builder_via(None, timeout, 0), &client);
    let err = fetch_with_validated_redirects(&unproxied, &url, &client, None, None, None).expect_err("the same name, looked up here");
    assert!(refusal_in(&err).is_some(), "{err}");
    via_proxy.assert_async().await;
    direct.assert_async().await;
}

#[test]
fn a_lookup_reads_a_bracketed_address_and_fails_on_one_without_a_port() {
    let client = Client::no_own_site();
    assert!(is_refusal(&client.resolve("[fd00::1]:80").unwrap_err()));
    assert!(!is_refusal(&client.resolve("nas.test").unwrap_err()));
}
