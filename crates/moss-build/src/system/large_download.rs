//! Resilient fetch for a LARGE artifact over a hostile network.
//!
//! `build::assets::download` is right for site assets: small, in-memory, one
//! shot, bounded by a total-duration timeout. Those exact properties make it
//! wrong for a 167 MB stack DMG pulled across the GFW, where three distinct
//! failures were observed on one machine within 24 hours:
//!
//! 1. **Slow but alive.** The asset host throttled to ~83 KB/s. A 300 s
//!    *total-duration* timeout silently encodes "must sustain ≥ 560 KB/s" —
//!    the transfer was progressing normally and was killed anyway. Here the
//!    deadline is a STALL timeout instead (no bytes for [`STALL_SECS`]), so a
//!    slow link finishes and only a genuinely wedged one aborts.
//!
//! 2. **Mid-stream drops.** Retrying an in-memory download restarts at byte 0,
//!    so on a link that drops every few minutes a large file can *never*
//!    complete. Here bytes land in a `.part` file and each attempt resumes with
//!    `Range: bytes=N-` (the CDN answers 206). Progress is cumulative across
//!    attempts, and across whole app runs.
//!
//! 3. **One transport dies, the other lives.** The proxy path failed TLS
//!    (`SSL_ERROR_SYSCALL`) at the same moment the direct path worked, having
//!    been the reverse the day before. Attempts therefore ALTERNATE proxy and
//!    direct rather than committing to whichever was configured.
//!
//! Resuming cuts both ways, so the `.part` is treated as *disposable state,
//! never as an invariant*: a partial that can no longer become the artifact is
//! deleted rather than retried against. A `.part` longer than the remote file
//! draws a 416 from every resumed request, and one that trips the [`MAX_BYTES`]
//! guard trips it again on every attempt — in both cases the bytes on disk are
//! what makes the failure permanent, so they go. See
//! [`response_invalidates_partial`].
//!
//! The verified artifact is cached, so a retry after a *later* stage fails
//! (mount/start) costs no network at all — the caller deletes the cache once
//! the install fully succeeds.
//!
//! Network I/O is kept to one thin function; every decision it makes is a pure
//! function tested below.

use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Abort an attempt after this long with NO bytes arriving. Not a cap on total
/// duration — a slow transfer that keeps delivering bytes runs to completion.
const STALL_SECS: u64 = 90;
/// TCP/TLS connect budget per attempt.
const CONNECT_SECS: u64 = 30;
/// Transport alternates across this many attempts before giving up.
const MAX_ATTEMPTS: u32 = 6;
/// Refuse absurd artifacts (a stack DMG is ~200 MB).
const MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// Which route an attempt takes. The two are tried in alternation because
/// either one can be the broken one on a given day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Proxy,
    Direct,
}

/// Attempt N's route. Odd attempts prefer the system proxy when one exists
/// (it is usually the fast path on a censored network); even attempts go
/// direct. With no proxy configured every attempt is direct.
pub fn plan_transport(attempt: u32, has_proxy: bool) -> Transport {
    if !has_proxy || attempt % 2 == 0 {
        Transport::Direct
    } else {
        Transport::Proxy
    }
}

/// What to do with a resumed request's response status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeOutcome {
    /// Server honoured `Range` (206) — append to what we already have.
    Append,
    /// Server ignored `Range` and sent the whole body (200) — the partial file
    /// must be discarded or the result is a corrupt concatenation.
    RestartFromZero,
}

/// Decide how to treat a response to a (possibly) ranged request.
pub fn resume_outcome(status: u16, requested_offset: u64) -> ResumeOutcome {
    if requested_offset > 0 && status == 206 {
        ResumeOutcome::Append
    } else {
        ResumeOutcome::RestartFromZero
    }
}

/// Does this response status mean the bytes already in `.part` can never
/// become the artifact, so the partial must be thrown away?
///
/// The wedge this exists for is **416 Range Not Satisfiable**. A `.part` that
/// is LONGER than the remote artifact — a truncated release re-cut at the same
/// URL, a partial written by a different build, a resume against a shorter
/// mirror — asks for `bytes=<past-the-end>-` and the server refuses. Every
/// attempt in the loop then fails identically, and so does every retry the user
/// makes tomorrow, because nothing on the failing path ever deletes the file:
/// the install could only be un-wedged by hand-deleting moss's cache. Keeping
/// bytes the server has just said are unusable buys nothing, so they go and
/// the next attempt starts clean.
///
/// 404 and 410 are the same shape one step further out: the artifact these
/// bytes are part OF is gone, so they lead nowhere either.
///
/// Every other status leaves the partial alone, and the list is deliberately
/// this short. A 403 (an expired signed URL, a CDN edge refusing this hop) or
/// a 429 (rate limit) says nothing whatsoever about the bytes on disk, yet the
/// blanket `4xx` rule this replaced deleted up to 167 MB on one — and on the
/// 83 KB/s censored link this module exists for, re-fetching that costs ~35
/// minutes per occurrence. A 5xx is the same argument, already conceded: the
/// server is having a bad minute and a retry should RESUME. 2xx is not a
/// failure at all.
pub fn response_invalidates_partial(status: u16) -> bool {
    matches!(status, 416 | 404 | 410)
}

/// Throw away a partial that can no longer lead to the artifact, so the next
/// attempt (and the user's next retry, days later) starts from zero instead of
/// replaying the same failure against the same bytes.
fn discard_partial(part: &Path, label: &str, why: &str) {
    if part.exists() {
        log::warn!(
            "{label} discarding partial {} — {why}; the next attempt restarts from zero",
            part.display()
        );
        let _ = std::fs::remove_file(part);
    }
}

/// Bytes already on disk for a partial download (0 when absent/unreadable).
pub fn resume_offset(part: &Path) -> u64 {
    std::fs::metadata(part).map(|m| m.len()).unwrap_or(0)
}

/// `Range` header value for resuming at `offset` (none at offset 0).
pub fn range_header(offset: u64) -> Option<String> {
    (offset > 0).then(|| format!("bytes={offset}-"))
}

/// Stream a file's sha256 without loading it into memory.
pub fn file_sha256(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// True when `path` is a complete artifact matching `expected_sha256`.
pub fn cached_file_is_valid(path: &Path, expected_sha256: &str) -> bool {
    if !path.is_file() {
        return false;
    }
    match file_sha256(path) {
        Ok(actual) => actual == expected_sha256.to_lowercase(),
        Err(_) => false,
    }
}

/// Promote a `.part` that already hashes correctly into the finished artifact.
/// Answers "was there nothing left to do?".
///
/// Called BEFORE each attempt as well as after one, because a `.part` can
/// already BE the artifact with no request made at all: the previous run can die
/// between the final byte and the rename (a fallible syscall of its own), or
/// during the hash pass. An unverified complete partial then resumes at
/// `bytes=<its-full-length>-`, the server answers 416, and
/// [`response_invalidates_partial`] deletes a byte-perfect 167 MB download that
/// only ever needed renaming. A partial moss cannot READ is not the artifact
/// either, and is treated the same way.
fn promote_if_complete(
    part: &Path,
    dest: &Path,
    expected_sha256: &str,
    label: &str,
) -> Result<bool, String> {
    if !cached_file_is_valid(part, expected_sha256) {
        return Ok(false);
    }
    std::fs::rename(part, dest).map_err(|e| format!("finalize {}: {e}", dest.display()))?;
    log::info!("{label} download complete + sha256 verified");
    Ok(true)
}

/// Progress callback: (bytes_done, total_bytes_if_known).
pub type ProgressFn<'a> = dyn Fn(u64, Option<u64>) + 'a;

/// Fetch `url` into `dest` (resuming `dest.part`), verifying `expected_sha256`.
///
/// Returns as soon as a byte-exact artifact is at `dest` — including instantly
/// when a valid one is already cached there. `label` prefixes log lines.
pub fn fetch_verified(
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    label: &str,
    on_progress: Option<&ProgressFn<'_>>,
) -> Result<(), String> {
    if cached_file_is_valid(dest, expected_sha256) {
        log::info!("{label} using cached artifact at {} (sha256 OK)", dest.display());
        if let Some(cb) = on_progress {
            let total = std::fs::metadata(dest).map(|m| m.len()).ok();
            cb(total.unwrap_or(0), total);
        }
        return Ok(());
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let part = part_path(dest);
    let has_proxy = crate::system::proxy::resolve_proxy_for_url(url).is_some();

    let mut last_err = String::new();
    for attempt in 1..=MAX_ATTEMPTS {
        // Before any request: the partial may already be the whole artifact.
        if promote_if_complete(&part, dest, expected_sha256, label)? {
            return Ok(());
        }

        let transport = plan_transport(attempt, has_proxy);
        let offset = resume_offset(&part);
        log::info!(
            "{label} attempt {attempt}/{MAX_ATTEMPTS} via {transport:?}{}",
            if offset > 0 {
                format!(" resuming at {offset} bytes")
            } else {
                String::new()
            }
        );

        match stream_to_part(url, &part, offset, transport, label, on_progress) {
            Ok(()) if promote_if_complete(&part, dest, expected_sha256, label)? => return Ok(()),
            Ok(()) => {
                // The body ended cleanly and the result is still not the
                // artifact, so these are the wrong bytes rather than too few of
                // them — keeping them would poison every retry. (Too few does
                // not land here: given a Content-Length, ureq turns a truncated
                // body into an `UnexpectedEof` read error, which is the arm
                // below, and KEEPS the partial. A response without one cannot be
                // told apart from a complete body, so that case is
                // accepted-lossy and restarts clean.)
                log::error!(
                    "{label} completed download does not match sha256 {expected_sha256} — discarding partial and retrying clean"
                );
                let _ = std::fs::remove_file(&part);
                last_err = "sha256 mismatch".to_string();
            }
            Err(e) => {
                log::warn!("{label} attempt {attempt}/{MAX_ATTEMPTS} failed: {e}");
                last_err = e;
            }
        }

        if attempt < MAX_ATTEMPTS {
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    Err(format!(
        "download failed after {MAX_ATTEMPTS} attempts: {last_err}"
    ))
}

/// The partial-download sidecar for a destination path.
pub fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_os_string();
    s.push(".part");
    PathBuf::from(s)
}

/// One `ureq` agent, built the same way for every outbound request this
/// module makes: identified as moss, stall-timed rather than duration-timed,
/// and routed through the system proxy when `transport` calls for it.
///
/// Extracted from `stream_to_part` so `fetch_text`/`fetch_bytes` inherit the
/// same proxy handling instead of a second, hand-rolled `ureq::get` that
/// would miss it — see `mac-https-proxy-breaks-curl-resolve`.
fn agent(url: &str, transport: Transport, label: &str) -> ureq::Agent {
    // A stall timeout, deliberately NOT a total-duration timeout: a 167 MB
    // artifact on a throttled link legitimately takes many minutes.
    let mut builder = ureq::AgentBuilder::new()
        // Identify moss, like every other outbound request — see
        // `system::user_agent`. A 167 MB fetch from a censored network is
        // the request most likely to draw a bot challenge, and a challenge page
        // arrives as a perfectly ordinary 200 that lands in the `.part` and
        // fails the hash, burning an attempt for a reason nothing here can see.
        .user_agent(&crate::system::user_agent())
        .timeout_connect(Duration::from_secs(CONNECT_SECS))
        .timeout_read(Duration::from_secs(STALL_SECS));
    if transport == Transport::Proxy {
        if let Some(proxy_url) = crate::system::proxy::resolve_proxy_for_url(url) {
            match ureq::Proxy::new(&proxy_url) {
                Ok(proxy) => builder = builder.proxy(proxy),
                Err(e) => log::warn!("{label} resolved proxy '{proxy_url}' is invalid ({e})"),
            }
        }
    }
    builder.build()
}

/// Fetch `url` as text, retrying across `MAX_ATTEMPTS` alternating transports
/// like the streaming path, but single-shot per attempt: no `.part` file, no
/// resume — for a small payload such as `latest.json` that either arrives
/// whole or is retried from scratch. No caller yet (E2' step 3); `moss
/// desktop install` is the first (step 7).
pub fn fetch_text(url: &str, label: &str) -> Result<String, String> {
    fetch_once(url, label, |resp| {
        resp.into_string().map_err(|e| format!("{label} reading body failed: {e}"))
    })
}

/// Fetch `url` as bytes, same retry behaviour as [`fetch_text`] — for a
/// release tarball that is verified in memory before anything touches disk.
pub fn fetch_bytes(url: &str, label: &str) -> Result<Vec<u8>, String> {
    fetch_once(url, label, |resp| {
        let mut buf = Vec::new();
        resp.into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| format!("{label} reading body failed: {e}"))?;
        Ok(buf)
    })
}

/// Shared retry loop for [`fetch_text`] and [`fetch_bytes`]: alternate
/// transport across attempts, hand the response to `read` on a 2xx.
fn fetch_once<T>(
    url: &str,
    label: &str,
    read: impl Fn(ureq::Response) -> Result<T, String>,
) -> Result<T, String> {
    let has_proxy = crate::system::proxy::resolve_proxy_for_url(url).is_some();
    let mut last_err = String::new();
    for attempt in 1..=MAX_ATTEMPTS {
        let transport = plan_transport(attempt, has_proxy);
        match agent(url, transport, label).get(url).call() {
            Ok(resp) => return read(resp),
            Err(e) => {
                log::warn!("{label} attempt {attempt}/{MAX_ATTEMPTS} failed: {e}");
                last_err = e.to_string();
            }
        }
        if attempt < MAX_ATTEMPTS {
            std::thread::sleep(Duration::from_secs(2));
        }
    }
    Err(format!("{label} failed after {MAX_ATTEMPTS} attempts: {last_err}"))
}

/// One transfer attempt: open the (possibly ranged) stream and append to
/// `part` until EOF. The only function here that touches the network for the
/// resumable streaming path.
fn stream_to_part(
    url: &str,
    part: &Path,
    offset: u64,
    transport: Transport,
    label: &str,
    on_progress: Option<&ProgressFn<'_>>,
) -> Result<(), String> {
    let mut req = agent(url, transport, label).get(url);
    if let Some(range) = range_header(offset) {
        req = req.set("Range", &range);
    }
    let response = match req.call() {
        Ok(r) => r,
        // ureq reports every non-2xx as an error, so this is where a 416 to a
        // resumed request lands. Judged by `response_invalidates_partial`
        // rather than inline, because "may these bytes survive?" is the whole
        // difference between a retryable blip and a permanently wedged cache.
        Err(ureq::Error::Status(code, _)) if response_invalidates_partial(code) => {
            discard_partial(part, label, &format!("server answered HTTP {code}"));
            return Err(format!("HTTP {code} for {url}"));
        }
        Err(e) => return Err(format!("HTTP request failed for {url}: {e}")),
    };

    let outcome = resume_outcome(response.status(), offset);
    let body_len: Option<u64> = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());

    let start_at = match outcome {
        ResumeOutcome::Append => offset,
        ResumeOutcome::RestartFromZero => {
            if offset > 0 {
                log::warn!("{label} server ignored Range (status {}) — restarting from 0", response.status());
            }
            0
        }
    };
    let total = body_len.map(|len| start_at + len);
    if let Some(t) = total {
        if t > MAX_BYTES {
            // The size guard trips on `start_at + body_len`, so an absurd
            // `.part` trips it just as surely as an absurd artifact — and
            // leaving that `.part` in place would make the guard fire again on
            // every future attempt. Discard, then refuse.
            let msg = format!(
                "artifact too large: {} MB (max {} MB)",
                t / (1024 * 1024),
                MAX_BYTES / (1024 * 1024)
            );
            discard_partial(part, label, &msg);
            return Err(msg);
        }
    }

    let mut file = if start_at > 0 {
        // allow:raw_write the .part download file sits in the app-data cache, not build output
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(part)
            .map_err(|e| format!("open {}: {e}", part.display()))?;
        // Truncate any bytes past the offset the server is resuming from.
        f.set_len(start_at)
            .map_err(|e| format!("truncate {}: {e}", part.display()))?;
        f.seek(std::io::SeekFrom::Start(start_at))
            .map_err(|e| format!("seek {}: {e}", part.display()))?;
        f
    } else {
        // allow:raw_write the .part download file sits in the app-data cache, not build output
        std::fs::File::create(part).map_err(|e| format!("create {}: {e}", part.display()))?
    };

    let mut reader = response.into_reader();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = start_at;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("read body: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("write {}: {e}", part.display()))?;
        done += n as u64;
        if done > MAX_BYTES {
            // Same reasoning as the Content-Length guard: an oversized `.part`
            // left on disk re-trips this on every attempt forever. Close the
            // handle first so the bytes are actually reclaimed.
            let msg = format!("artifact exceeded {} MB", MAX_BYTES / (1024 * 1024));
            drop(file);
            discard_partial(part, label, &msg);
            return Err(msg);
        }
        if let Some(cb) = on_progress {
            cb(done, total);
        }
    }
    file.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn transport_alternates_only_when_a_proxy_exists() {
        // With a proxy: odd attempts proxied, even attempts direct — so one
        // dead route can never consume every attempt.
        assert_eq!(plan_transport(1, true), Transport::Proxy);
        assert_eq!(plan_transport(2, true), Transport::Direct);
        assert_eq!(plan_transport(3, true), Transport::Proxy);
        // Without one, every attempt is direct.
        for a in 1..=4 {
            assert_eq!(plan_transport(a, false), Transport::Direct);
        }
    }

    #[test]
    fn resume_is_only_honoured_on_a_206() {
        assert_eq!(resume_outcome(206, 100), ResumeOutcome::Append);
        // 200 to a ranged request means the server sent the WHOLE body;
        // appending it would corrupt the file.
        assert_eq!(resume_outcome(200, 100), ResumeOutcome::RestartFromZero);
        // No offset requested => nothing to resume.
        assert_eq!(resume_outcome(200, 0), ResumeOutcome::RestartFromZero);
        assert_eq!(resume_outcome(206, 0), ResumeOutcome::RestartFromZero);
    }

    #[test]
    fn only_a_status_about_these_bytes_poisons_the_partial() {
        // The wedge: a `.part` longer than the artifact makes the server answer
        // 416 to every resume, forever, unless the bytes are dropped.
        assert!(response_invalidates_partial(416), "416 is the wedge");
        // A moved/withdrawn artifact: whatever is on disk is not it.
        assert!(response_invalidates_partial(404));
        assert!(response_invalidates_partial(410));
        // Statuses about the REQUEST, not the bytes. Discarding on these threw
        // away up to 167 MB — ~35 minutes at the 83 KB/s this module exists for
        // — over a rate limit or an expired signed URL.
        assert!(!response_invalidates_partial(429), "rate limit: try again later");
        assert!(!response_invalidates_partial(403));
        assert!(!response_invalidates_partial(400));
        // Success statuses are not failures at all.
        assert!(!response_invalidates_partial(200));
        assert!(!response_invalidates_partial(206));
        // A server having a bad minute says nothing about our bytes — resuming
        // 167 MB beats re-downloading it.
        assert!(!response_invalidates_partial(500));
        assert!(!response_invalidates_partial(503));
    }

    #[test]
    fn a_complete_partial_is_verified_and_promoted_rather_than_re_requested() {
        // The loss this prevents: a `.part` that is already byte-perfect (the
        // rename or the hash pass died after the last byte landed) was never
        // hashed on the way in. It resumed at `bytes=<full-length>-`, drew a
        // 416, and 167 MB was deleted. An unroutable URL proves the promotion
        // happens before — and instead of — any request.
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("stack.dmg");
        let part = part_path(&dest);
        std::fs::write(&part, b"abc").unwrap();
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

        fetch_verified("http://0.0.0.0:1/never", &dest, abc, "[test]", None)
            .expect("a complete partial finishes the download offline");

        assert_eq!(std::fs::read(&dest).unwrap(), b"abc", "promoted byte-for-byte");
        assert!(!part.exists(), "and moved, not copied — no second 167 MB");
    }

    #[test]
    fn a_partial_that_is_not_the_artifact_is_still_resumed_from() {
        // The other half: promotion must not fire on bytes that merely exist,
        // or a truncated `.part` would be published as the artifact.
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("stack.dmg");
        let part = part_path(&dest);
        std::fs::write(&part, b"ab").unwrap();
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

        assert!(fetch_verified("http://0.0.0.0:1/never", &dest, abc, "[test]", None).is_err());
        assert!(!dest.exists(), "an incomplete partial is never promoted");
        assert_eq!(resume_offset(&part), 2, "and survives to be resumed");
    }

    #[test]
    fn discarding_a_partial_removes_it_and_tolerates_its_absence() {
        let tmp = TempDir::new().unwrap();
        let part = tmp.path().join("a.dmg.part");
        // Nothing to discard is not an error — the caller must not have to check.
        discard_partial(&part, "[test]", "no reason");

        std::fs::write(&part, vec![0u8; 128]).unwrap();
        discard_partial(&part, "[test]", "oversized");
        assert!(!part.exists(), "the poisoned bytes are gone");
        assert_eq!(resume_offset(&part), 0, "the next attempt starts from zero");
    }

    #[test]
    fn resume_offset_and_range_header() {
        let tmp = TempDir::new().unwrap();
        let part = tmp.path().join("a.dmg.part");
        assert_eq!(resume_offset(&part), 0, "absent => start at 0");
        assert_eq!(range_header(0), None, "no Range at offset 0");

        std::fs::write(&part, vec![7u8; 4096]).unwrap();
        assert_eq!(resume_offset(&part), 4096);
        assert_eq!(range_header(4096).as_deref(), Some("bytes=4096-"));
    }

    #[test]
    fn part_path_is_a_sidecar_of_dest() {
        assert_eq!(
            part_path(Path::new("/tmp/onionpress-v1.dmg")),
            PathBuf::from("/tmp/onionpress-v1.dmg.part")
        );
    }

    #[test]
    fn file_sha256_streams_and_matches_known_vector() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("empty");
        std::fs::write(&f, b"").unwrap();
        assert_eq!(
            file_sha256(&f).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let g = tmp.path().join("abc");
        std::fs::write(&g, b"abc").unwrap();
        assert_eq!(
            file_sha256(&g).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn cached_file_validity_is_content_addressed() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("artifact.dmg");
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

        assert!(!cached_file_is_valid(&f, abc), "absent => not valid");
        std::fs::write(&f, b"abc").unwrap();
        assert!(cached_file_is_valid(&f, abc), "matching sha => reusable");
        assert!(
            cached_file_is_valid(&f, &abc.to_uppercase()),
            "expected hash is case-insensitive"
        );

        std::fs::write(&f, b"tampered").unwrap();
        assert!(!cached_file_is_valid(&f, abc), "wrong bytes => re-download");
    }

    #[test]
    fn a_valid_cache_short_circuits_without_touching_the_network() {
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("cached.dmg");
        std::fs::write(&dest, b"abc").unwrap();
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

        // An unroutable URL proves no request is made on the cache-hit path.
        let seen = std::cell::Cell::new((0u64, None));
        let cb: Box<ProgressFn<'_>> = Box::new(|d, t| seen.set((d, t)));
        fetch_verified("http://0.0.0.0:1/never", &dest, abc, "[test]", Some(&*cb))
            .expect("cache hit must succeed offline");

        assert_eq!(seen.get(), (3, Some(3)), "cache hit reports 100% progress");
    }
}
