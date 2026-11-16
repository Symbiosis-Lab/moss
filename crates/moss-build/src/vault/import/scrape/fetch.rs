//! Pure HTTP fetch infrastructure for the scrape pipeline: SSRF-safe
//! redirect validation, Retry-After-aware retry, and the two raw-fetch
//! primitives everything else (a page fetch, a media download, a sitemap
//! document fetch) is built on. No [`super::crawl_state::CrawlState`], no
//! crawl orchestration — just the HTTP calls themselves, so [`super::sitemap`]
//! can depend on this file without depending on [`super::run`], the
//! orchestrator that in turn depends on this one.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use super::crawl_state::HostPacer;
use super::scope::UrlScope;

/// Strip query + fragment from a URL, leaving a clean canonical form for the
/// `origin` frontmatter. Falls back to the input if it doesn't parse.
pub(crate) fn strip_query(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut u) => {
            u.set_query(None);
            u.set_fragment(None);
            u.to_string()
        }
        Err(_) => url.to_string(),
    }
}

/// Transient failures worth one more attempt: transport-level errors
/// (None) and 429/5xx. Client errors (4xx) are permanent.
fn should_retry_status(status: Option<u16>) -> bool {
    match status {
        None => true,
        Some(429) => true,
        Some(s) => s >= 500,
    }
}

/// Longest delay [`call_with_retry_n`] will ever honor from a `Retry-After`
/// header, regardless of what the host asked for — a host asking for an
/// hour is asking the wrong tool, and a crawl that waited it out verbatim
/// could stall the whole run on one URL.
const MAX_RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

/// Parse a `Retry-After` header value (RFC 9110 §10.2.3): either an integer
/// delta-seconds, or an HTTP-date — `chrono`'s RFC 2822 parser accepts it,
/// since the HTTP-date grammar (`Sun, 06 Nov 1994 08:49:37 GMT`) is itself a
/// valid, non-obsolete RFC 2822 date-time. Returns `None` for a
/// missing/unparsable header, a non-positive delta-seconds, or a date
/// already in the past — [`call_with_retry_n`] falls back to its own
/// exponential backoff in all of those cases, same as a 429/5xx with no
/// header at all. Zero is treated as "no useful wait", same as a negative
/// value or a past date, rather than as license to retry with no delay at
/// all: a 429 pairing its rate-limit refusal with `Retry-After: 0` is a
/// contradiction the backoff schedule is a safer response to than an
/// instant retry would be, and the two branches below would otherwise
/// disagree about it — an integer `0` would have been honored while an
/// HTTP-date computing to the same "now" was already rejected. The result
/// is capped at [`MAX_RETRY_AFTER`].
fn parse_retry_after(value: &str) -> Option<std::time::Duration> {
    let value = value.trim();
    if let Ok(secs) = value.parse::<i64>() {
        return (secs > 0).then(|| std::time::Duration::from_secs(secs as u64).min(MAX_RETRY_AFTER));
    }
    let when = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let delta = when.with_timezone(&chrono::Utc) - chrono::Utc::now();
    let secs = delta.num_seconds();
    (secs > 0).then(|| std::time::Duration::from_secs(secs as u64).min(MAX_RETRY_AFTER))
}

/// Debug builds only: let imports reach this machine, for a local test
/// server. A process flag rather than the environment variable, which a test
/// cannot set while other tests' threads read the environment.
#[cfg(debug_assertions)]
pub fn allow_loopback() {
    ALLOW_LOOPBACK.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(debug_assertions)]
static ALLOW_LOOPBACK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the debug loopback allowance covers `host`: `localhost` and loopback
/// literals only. Local development is explicit and cannot be enabled in a
/// release build.
#[cfg_attr(not(debug_assertions), allow(unused_variables))]
fn loopback_allowed(host: &str) -> bool {
    #[cfg(debug_assertions)]
    if ALLOW_LOOPBACK.load(std::sync::atomic::Ordering::Relaxed) || std::env::var("MOSS_IMPORT_ALLOW_LOOPBACK").as_deref() == Ok("1") {
        return match url::Host::parse(host) {
            Ok(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
            Ok(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Ok(url::Host::Ipv6(ip)) => ip.is_loopback(),
            Err(_) => false,
        };
    }
    false
}

/// Refuse a URL that is not a plain `http`/`https` request to a public or
/// private-network host.
///
/// The import path hands `start_url` (and, via [`fetch_with_validated_redirects`],
/// every redirect target reached while fetching it) straight to ureq — an
/// attacker-controlled webview could otherwise point the app's own network
/// stack at `file://` (reading local files into the imported vault, where
/// they get published) or an internal service (`http://127.0.0.1:<port>`, a
/// link-local cloud-metadata address). A URL that passes this check on
/// `start_url` alone is not enough: a public host the check allowed can
/// itself 302/303/307/308 to a loopback/link-local target, so
/// [`fetch_with_validated_redirects`] re-runs the same rows on every hop
/// rather than trusting ureq's own redirect-following (which does not
/// re-validate at all).
///
/// A private-network literal passes here: whether it is the person's own site
/// is [`Client::check`]'s decision, which knows the address the person gave.
///
/// This gate reads the URL string. A name passes it and is checked when the
/// fetch connects, against every address it resolves to ([`Client::resolve`]).
/// Behind a system proxy the proxy does the lookup, so only addresses written
/// as literals are checked.
pub fn refuse_unsafe_scrape_url(url_str: &str) -> Result<(), String> {
    let url = url::Url::parse(url_str).map_err(|e| format!("invalid URL: {e}"))?;
    refuse_unsafe(&url).map(|_| ())
}

/// The rows every import address meets, from the typed address to each hop:
/// the scheme, the debug loopback allowance, local names, and literals of the
/// classes no import may reach. `Ok(Some(ip))` is a literal that passed, as the
/// IPv4 address it carries; `Ok(None)` a name or an allowed loopback.
fn refuse_unsafe(url: &url::Url) -> Result<Option<std::net::IpAddr>, String> {
    use crate::system::address::{carried_ipv4, classify, is_local_name, AddressClass};
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(format!(
                "URL scheme '{other}' is not allowed - only http/https"
            ))
        }
    }
    if url.host_str().is_some_and(loopback_allowed) {
        return Ok(None);
    }
    // `url::Host::Ipv4`/`Ipv6` carry parsed addresses directly — `host_str()`
    // returns an IPv6 literal WITH its brackets (`"[::1]"`), which fails a
    // plain `str::parse::<IpAddr>()` and would silently let every bracketed
    // IPv6 literal through unchecked.
    let ip: std::net::IpAddr = match url.host() {
        Some(url::Host::Domain(d)) if is_local_name(d) => {
            return Err("URL must not target localhost".to_string());
        }
        Some(url::Host::Domain(_)) => return Ok(None),
        Some(url::Host::Ipv4(v4)) => v4.into(),
        Some(url::Host::Ipv6(v6)) => v6.into(),
        None => return Err("URL has no host".to_string()),
    };
    if matches!(classify(ip), AddressClass::Loopback | AddressClass::LinkLocal | AddressClass::Unspecified | AddressClass::Multicast) {
        return Err(format!("URL must not target a loopback, link-local or unspecified address ({ip})"));
    }
    Ok(Some(carried_ipv4(ip)))
}

/// `agent.get(url)`, following redirects by hand so every hop's target is
/// re-validated by [`Client::check`] before it is followed —
/// `agent` must be built with `redirects(0)` ([`agent_builder`]), or ureq
/// would already have followed (and connected to) the first hop before this
/// function ever saw it.
///
/// A `Location` header is resolved against the URL that sent it (it may be
/// relative) before being checked and followed. A 3xx with no `Location`,
/// or one that fails to parse even as a relative reference, is returned
/// as-is rather than chased.
///
/// `accept`, when given, is sent as the `Accept` header on every hop
/// (including the start URL) — see [`crate::vault::import::media::MEDIA_ACCEPT`].
/// `None` leaves ureq's own default (`Accept: */*`) in place, for a page
/// fetch.
///
/// With `scope` (a link being crawled), a hop off the site or to a
/// `mailto:`/`tel:` address is not requested but handed back as the 3xx
/// response, so the caller sees where the link leads; every other target is
/// still validated and followed, and a non-http(s) target still refused.
///
fn fetch_with_validated_redirects(
    agent: &ureq::Agent,
    start_url: &str,
    client: &Client,
    accept: Option<&str>,
    referer: Option<&str>,
    scope: Option<&UrlScope>,
) -> Result<ureq::Response, ureq::Error> {
    fetch_following_redirects(agent, start_url, client.user_agent(), accept, referer, |url| client.check(url), |next| {
        scope.is_some_and(|scope| leaves_site(scope, next))
    })
}

/// What one import fetches with: the user agent, the address the person gave,
/// and the addresses it refused. Every fetch of the import carries the same
/// client, so a refusal anywhere reaches one report.
#[derive(Clone, Debug)]
pub(crate) struct Client {
    user_agent: String,
    own: Option<OwnSite>,
    refused: std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<String>>>,
}

/// The private-network site the person gave, as a literal or a name. A
/// redirect never changes it.
#[derive(Clone, Debug)]
struct OwnSite {
    /// As the `url` crate writes it, the form a request's host is built from.
    host: String,
    /// The site's port, see [`site_port`].
    port: Option<u16>,
    /// Set once: a literal's at construction, a name's by the first lookup of it.
    addrs: std::sync::Arc<std::sync::OnceLock<OwnAddrs>>,
}

#[derive(Debug)]
struct OwnAddrs {
    ips: Vec<std::net::IpAddr>,
    private: bool,
}

/// A site's port: 80, 443 and none written are one value. An http site
/// redirects to https on the same machine, so this is the one place a port
/// other than the typed one is admitted.
fn site_port(port: Option<u16>) -> Option<u16> {
    port.filter(|p| !matches!(p, 80 | 443))
}

impl OwnSite {
    fn serves(&self, host: &str, port: u16) -> bool {
        let bare = |h: &str| h.strip_suffix('.').unwrap_or(h).to_ascii_lowercase();
        bare(&self.host) == bare(host) && self.port == site_port(Some(port))
    }
}

/// What a connect-time refusal carries, so a refused name is told from a
/// failed lookup and not retried.
#[derive(Debug)]
struct Refused(String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

/// The refusal a failed connect carries, if it was ours.
fn refusal_in(err: &ureq::Error) -> Option<String> {
    use std::error::Error as _;
    // ureq keeps the resolver's `io::Error` as the transport error's source.
    let refused = err.source()?.downcast_ref::<std::io::Error>()?.get_ref()?.downcast_ref::<Refused>()?;
    Some(refused.0.clone())
}

/// Most refused addresses a client keeps, so a page of private links cannot
/// grow the record without bound.
const MAX_REFUSED: usize = 1000;
/// Refused addresses the report names one by one.
const REPORTED_REFUSED: usize = 20;

impl Client {
    /// A client for `own_site`, the address the person typed or confirmed. One
    /// that is not an http(s) address is no own site.
    pub(crate) fn new(own_site: Option<&str>) -> Self {
        let own = own_site.and_then(|s| url::Url::parse(s.trim()).ok()).filter(|u| matches!(u.scheme(), "http" | "https")).and_then(|u| {
            let addrs = std::sync::Arc::new(std::sync::OnceLock::new());
            if let Some(ip) = literal_ip(&u) {
                let private = class_of(&u, ip) == crate::system::address::AddressClass::Private;
                let _ = addrs.set(OwnAddrs { ips: vec![ip], private });
            }
            Some(OwnSite { host: u.host_str()?.to_string(), port: site_port(u.port()), addrs })
        });
        Self { user_agent: super::service::default_user_agent(), own, refused: Default::default() }
    }

    pub(crate) fn no_own_site() -> Self {
        Self::new(None)
    }

    pub(crate) fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// Whether `url` may be requested; a refusal is recorded for the report.
    /// Run on the start address and on every redirect hop against that hop's
    /// own address.
    pub(crate) fn check(&self, url: &str) -> Result<(), String> {
        let decided = url::Url::parse(url).map_err(|e| format!("invalid URL: {e}")).and_then(|u| self.decide(&u));
        if decided.is_err() {
            self.record_refused(url);
        }
        decided
    }

    fn record_refused(&self, url: &str) {
        let mut refused = self.refused.lock().unwrap();
        if refused.len() < MAX_REFUSED {
            refused.insert(reportable(url));
        }
    }

    fn decide(&self, url: &url::Url) -> Result<(), String> {
        #[cfg(test)]
        if let Some(class) = test_servers::class_of(url) {
            return match literal_ip(url) {
                Some(ip) => self.admit(class, ip, url.port()),
                None => Ok(()),
            };
        }
        match refuse_unsafe(url)? {
            Some(ip) => self.admit(crate::system::address::classify(ip), ip, url.port()),
            None => Ok(()),
        }
    }

    /// A public address is fetched; a private-network one only as the person's
    /// own site.
    fn admit(&self, class: crate::system::address::AddressClass, ip: std::net::IpAddr, port: Option<u16>) -> Result<(), String> {
        use crate::system::address::AddressClass::{Private, Public};
        let own = self.own.as_ref().is_some_and(|own| own.port == site_port(port) && own.addrs.get().is_some_and(|a| a.ips.contains(&ip)));
        match class {
            Public => Ok(()),
            Private if own => Ok(()),
            Private => Err(format!("URL must not target a private-network address ({ip}) other than the one you gave")),
            other => Err(format!("URL must not target a {other:?} address ({ip})")),
        }
    }

    /// The own site's host when it is on a private network.
    pub(crate) fn local_host(&self) -> Option<&str> {
        self.own.as_ref().filter(|own| own.addrs.get().is_some_and(|a| a.private)).map(|own| own.host.as_str())
    }

    /// ureq's lookup for `netloc` (`host:port`): the answers [`Self::admitted`]
    /// keeps. The first lookup of the own site's name fixes its addresses.
    pub(crate) fn resolve(&self, netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
        let answers = lookup(netloc)?;
        let (host, port) = netloc
            .rsplit_once(':')
            .and_then(|(h, p)| Some((h, p.parse::<u16>().ok()?)))
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "an address without a port"))?;
        self.admitted(host, port, answers)
    }

    /// The `answers` for `host:port` the import may connect to. When none is
    /// left the error carries [`Refused`]: an empty list would read as a failed
    /// lookup and be retried.
    fn admitted(&self, host: &str, port: u16, answers: Vec<std::net::SocketAddr>) -> std::io::Result<Vec<std::net::SocketAddr>> {
        use crate::system::address::{carried_ipv4, classify, AddressClass};
        let class_of_ip = |ip| registered_class(host, port).unwrap_or_else(|| classify(ip));
        if let Some(own) = self.own.as_ref().filter(|own| own.serves(host, port)).filter(|_| !answers.is_empty()) {
            own.addrs.get_or_init(|| {
                let ips: Vec<_> = answers.iter().map(|a| carried_ipv4(a.ip())).collect();
                let private = ips.iter().any(|ip| class_of_ip(*ip) == AddressClass::Private);
                OwnAddrs { ips, private }
            });
        }
        let mut refusal = None;
        let kept: Vec<_> = answers
            .into_iter()
            .filter(|answer| {
                let ip = carried_ipv4(answer.ip());
                let class = class_of_ip(ip);
                let verdict = if class == AddressClass::Loopback && loopback_allowed(host) { Ok(()) } else { self.admit(class, ip, Some(port)) };
                match verdict {
                    Ok(()) => true,
                    Err(message) => {
                        refusal.get_or_insert(message);
                        false
                    }
                }
            })
            .collect();
        match refusal {
            Some(message) if kept.is_empty() => Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, Refused(message))),
            _ => Ok(kept),
        }
    }

    /// The refused addresses, sorted: `Not fetched:` lines for the first 20,
    /// then how many more.
    pub(crate) fn refused_report(&self) -> Vec<String> {
        let refused = self.refused.lock().unwrap();
        let mut lines: Vec<String> = refused.iter().take(REPORTED_REFUSED).map(|a| format!("Not fetched: {a}")).collect();
        if let Some(more) = refused.len().checked_sub(REPORTED_REFUSED).filter(|n| *n > 0) {
            lines.push(format!("Not fetched: {more} more {}.", if more == 1 { "address" } else { "addresses" }));
        }
        lines
    }
}

/// `url` as a report may show it: no query, fragment or sign-in details.
fn reportable(url: &str) -> String {
    let Ok(mut u) = url::Url::parse(url) else { return "an address that could not be read".to_string() };
    u.set_query(None);
    u.set_fragment(None);
    let _ = u.set_username("");
    let _ = u.set_password(None);
    u.to_string()
}

/// The answers to a lookup of `netloc`; under test a registered name answers
/// what it was registered with.
fn lookup(netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
    #[cfg(test)]
    if let Some(answer) = test_servers::NAMES.lock().unwrap().get(netloc) {
        return Ok(answer.iter().copied().collect());
    }
    std::net::ToSocketAddrs::to_socket_addrs(netloc).map(|answers| answers.collect())
}

/// The class a test registered `host:port` as.
#[cfg_attr(not(test), allow(unused_variables))]
fn registered_class(host: &str, port: u16) -> Option<crate::system::address::AddressClass> {
    #[cfg(test)]
    if let Some(class) = test_servers::class_of_key(&format!("{host}:{port}")) {
        return Some(class);
    }
    None
}

/// The class of a literal `ip` address named by `url`; a test server's is the
/// class it was registered as.
#[cfg_attr(not(test), allow(unused_variables))]
fn class_of(url: &url::Url, ip: std::net::IpAddr) -> crate::system::address::AddressClass {
    #[cfg(test)]
    if let Some(class) = test_servers::class_of(url) {
        return class;
    }
    crate::system::address::classify(ip)
}

/// The literal host of `url`, as the IPv4 address an IPv6 form carries.
fn literal_ip(url: &url::Url) -> Option<std::net::IpAddr> {
    let ip = match url.host()? {
        url::Host::Ipv4(v4) => v4.into(),
        url::Host::Ipv6(v6) => v6.into(),
        url::Host::Domain(_) => return None,
    };
    Some(crate::system::address::carried_ipv4(ip))
}

/// A mockito server the import may fetch from, as an address of `class`: a
/// `Public` one is fetched for every client, a `Private` one only as the own
/// site. mockito keeps its servers bound for the life of the process, so a
/// registered port never passes to another listener.
#[cfg(test)]
pub(crate) async fn test_server_as(class: crate::system::address::AddressClass) -> mockito::ServerGuard {
    let server = mockito::Server::new_async().await;
    test_servers::STARTED.lock().unwrap().insert(server.host_with_port(), class);
    server
}

/// Registers `name` as another way to reach `server`, of `class`: the name
/// resolves to the server's loopback address.
#[cfg(test)]
pub(crate) fn test_name_as(name: &str, server: &mockito::ServerGuard, class: crate::system::address::AddressClass) {
    let answer: std::net::SocketAddr = server.host_with_port().parse().unwrap();
    let key = format!("{name}:{}", answer.port());
    test_servers::NAMES.lock().unwrap().insert(key.clone(), Some(answer));
    test_servers::STARTED.lock().unwrap().insert(key, class);
}

/// Registers `name` on port 80 as a name whose lookup answers no address.
#[cfg(test)]
pub(crate) fn test_name_unanswered(name: &str) {
    let key = format!("{name}:80");
    test_servers::NAMES.lock().unwrap().insert(key.clone(), None);
    test_servers::STARTED.lock().unwrap().insert(key, crate::system::address::AddressClass::Public);
}

/// [`test_server_as`] a public address, the usual stand-in for a site.
#[cfg(test)]
pub(crate) async fn test_server() -> mockito::ServerGuard {
    test_server_as(crate::system::address::AddressClass::Public).await
}

/// Two private-network stand-ins: `own`, the address the person gave, and
/// `other`, one a page names. `client` has `own` alone as its own site.
#[cfg(test)]
pub(crate) struct PrivatePair {
    pub own: mockito::ServerGuard,
    pub other: mockito::ServerGuard,
    pub client: Client,
}

#[cfg(test)]
pub(crate) async fn private_pair() -> PrivatePair {
    let own = test_server_as(crate::system::address::AddressClass::Private).await;
    let other = test_server_as(crate::system::address::AddressClass::Private).await;
    let client = Client::new(Some(&own.url()));
    PrivatePair { own, other, client }
}

/// A loopback address no test registered, answering every request with an
/// empty 200, and how many requests reached it.
#[cfg(test)]
pub(crate) fn unregistered_loopback_server() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = hits.clone();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = stream.read(&mut [0; 4096]);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    (address, hits)
}

#[cfg(test)]
mod test_servers {
    use crate::system::address::AddressClass;

    pub(super) static STARTED: std::sync::Mutex<std::collections::BTreeMap<String, AddressClass>> =
        std::sync::Mutex::new(std::collections::BTreeMap::new());

    pub(super) static NAMES: std::sync::Mutex<std::collections::BTreeMap<String, Option<std::net::SocketAddr>>> =
        std::sync::Mutex::new(std::collections::BTreeMap::new());

    pub(super) fn class_of(url: &url::Url) -> Option<AddressClass> {
        let (host, port) = (url.host_str()?, url.port_or_known_default()?);
        class_of_key(&format!("{host}:{port}"))
    }

    pub(super) fn class_of_key(key: &str) -> Option<AddressClass> {
        STARTED.lock().unwrap().get(key).copied()
    }
}

/// Whether `next` is a contact address or an http(s) URL off the site.
fn leaves_site(scope: &UrlScope, next: &str) -> bool {
    let Ok(url) = url::Url::parse(next) else { return false };
    match url.scheme() {
        "mailto" | "tel" => true,
        "http" | "https" => super::scope::left_site_by_redirect(scope, next),
        _ => false,
    }
}

/// [`fetch_with_validated_redirects`]'s body, taking the per-hop validator as
/// a parameter so the redirect-following MECHANISM (hop resolution, the hop
/// cap, handing back a Location-less or unparsable 3xx as-is) is testable
/// independently of the SSRF POLICY (`Client::check`) — every
/// test server a unit test can stand up is itself a loopback address, so a
/// test proving a legitimate same-host redirect is still followed cannot
/// use the real policy without tripping its own loopback refusal. Production
/// always calls the [`fetch_with_validated_redirects`] wrapper above, which
/// always wires in the real policy; no caller outside this file's tests
/// should call this directly.
///
/// On an asset download (`accept` is `Some`), every redirect target also
/// goes through [`crate::vault::import::media::original_media_url`] before
/// it is validated or followed — the single place that rewrite applies to a
/// hop, so a CDN row never needs to be re-applied by each caller. Without
/// it, a `static1.squarespace.com` URL this function was handed already
/// rewritten (by the caller, before the URL was ever committed to markdown)
/// 301s to `images.squarespace-cdn.com` with a plain `content-type=` query
/// that dropped the rewrite, and that redirect target would download
/// un-rewritten. A page fetch (`accept: None`) never applies this rewrite —
/// it has no media URL to canonicalize, and the two ruled hosts are
/// asset-only CDNs no page redirect has a legitimate reason to land on.
fn fetch_following_redirects(
    agent: &ureq::Agent,
    start_url: &str,
    user_agent: &str,
    accept: Option<&str>,
    referer: Option<&str>,
    validate: impl Fn(&str) -> Result<(), String>,
    reports: impl Fn(&str) -> bool,
) -> Result<ureq::Response, ureq::Error> {
    // A redirect can land on a ruled CDN host carrying a query the table never
    // produced, so asset hops are re-canonicalized; a page fetch has no media
    // URL to canonicalize and must not have one invented for it.
    let rewrite = |next: String| {
        if accept.is_some() {
            crate::vault::import::media::original_media_url(&next).unwrap_or(next)
        } else {
            next
        }
    };
    crate::system::redirects::get_following_redirects(agent, start_url, user_agent, accept, referer, validate, rewrite, reports)
}

/// A fetch's final failure, carrying the HTTP status code as DATA when the
/// failure was a response status — as opposed to a transport failure, a
/// body-read failure, or a redirect refusal before any status came back —
/// so a caller that needs to know *why* a fetch failed (chiefly
/// [`observe_pace`], deciding whether a host just rate-limited this crawl)
/// matches on `status` instead of re-parsing the text composed for display.
/// `Display` prints the same `"{code} {status_text}"` / `"HTTP error: …"`
/// text [`PageOutcome::Failed`] and the crawl's `log::warn!` calls already
/// expect, and `From<FetchError> for String` lets every caller outside this
/// file's status-aware ones (`fetch_raw`/`fetch_raw_bytes`, whose callers in
/// `sitemap.rs` only ever check `.is_ok()`) keep using `?` unchanged.
#[derive(Debug, Clone)]
pub(crate) struct FetchError {
    status: Option<u16>,
    message: String,
}

impl FetchError {
    fn message(message: String) -> Self {
        Self { status: None, message }
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<FetchError> for String {
    fn from(e: FetchError) -> String {
        e.message
    }
}

/// The wait before the first retry; each later one doubles it.
const BACKOFF_BASE: std::time::Duration = std::time::Duration::from_millis(500);

/// `ureq::get` with up to `tries` attempts, for transient failures only (see
/// [`should_retry_status`]). Permanent failures (4xx other than 429) return
/// immediately regardless of `tries`.
///
/// The delay before each retry is polite rather than blind: a 429 or 503
/// that named a `Retry-After` is honored (parsed by [`parse_retry_after`],
/// capped at [`MAX_RETRY_AFTER`]); anything else — no header, an unparsable
/// one, or a transport-level error with no response at all — falls back to
/// exponential backoff (500ms, then 1s, then 2s, …) the same as before this
/// header was read at all.
///
/// Note: ureq maps any non-2xx response to `Error::Status` on `.call()`
/// itself, so this single retry loop also replaces what used to be a
/// separate manual `status >= 400` check in `fetch_page`.
///
/// `accept` is forwarded to [`fetch_with_validated_redirects`] unchanged —
/// `None` for a page fetch (ureq's own `Accept: */*` default), `Some(
/// media::MEDIA_ACCEPT)` for an asset download.
fn call_with_retry_n(
    url: &str,
    client: &Client,
    timeout: std::time::Duration,
    tries: u32,
    accept: Option<&str>,
    referer: Option<&str>,
    scope: Option<&UrlScope>,
) -> Result<ureq::Response, FetchError> {
    let mut last_err = FetchError::message(String::new());
    // What the PREVIOUS attempt's response asked for via `Retry-After`,
    // consumed by the sleep before the NEXT attempt. Reset on every
    // iteration (not just when absent), so a Retry-After-less response
    // following one that had it doesn't keep honoring a now-stale wait.
    let mut retry_after: Option<std::time::Duration> = None;
    for attempt in 0..tries {
        if attempt > 0 {
            let delay = retry_after.take().unwrap_or_else(|| BACKOFF_BASE * 2u32.pow(attempt - 1));
            std::thread::sleep(delay);
        }
        // Proxy-aware like every other moss HTTP client (system::proxy):
        // a direct connect times out on hosts only reachable via proxy.
        let agent = agent_for(agent_builder(url, timeout), client);
        match fetch_with_validated_redirects(&agent, url, client, accept, referer, scope) {
            Ok(resp) => return Ok(resp),
            Err(ureq::Error::Status(code, resp)) => {
                let err = FetchError { status: Some(code), message: format!("{} {}", code, resp.status_text()) };
                if !should_retry_status(Some(code)) {
                    return Err(err);
                }
                retry_after = resp.header("Retry-After").and_then(parse_retry_after);
                last_err = err;
            }
            Err(e) => {
                if let Some(refusal) = refusal_in(&e) {
                    let at = if let ureq::Error::Transport(t) = &e { t.url().map(|u| u.as_str()) } else { None };
                    client.record_refused(at.unwrap_or(url));
                    return Err(FetchError { status: Some(400), message: format!("400 disallowed URL: {refusal}") });
                }
                // Transport-level error (timeout, connection reset, TLS...)
                // — no response at all, so no status code either.
                retry_after = None;
                last_err = FetchError::message(format!("HTTP error: {}", e));
            }
        }
    }
    Err(last_err)
}

/// The agent builder for `url`, through the system's proxy when it has one.
fn agent_builder(url: &str, timeout: std::time::Duration) -> (ureq::AgentBuilder, bool) {
    // A registered address exists only in this process: no proxy could reach it.
    #[cfg(test)]
    if url::Url::parse(url).ok().is_some_and(|u| test_servers::class_of(&u).is_some()) {
        return crate::system::proxy::proxied_agent_builder_via(None, timeout, 0);
    }
    crate::system::proxy::proxied_agent_builder(url, timeout, 0)
}

/// `builder`'s agent, connecting only to addresses `client` admits. Behind a
/// proxy the proxy is what ureq looks up, so the hook is left off.
fn agent_for((builder, proxied): (ureq::AgentBuilder, bool), client: &Client) -> ureq::Agent {
    if proxied {
        return builder.build();
    }
    let client = client.clone();
    builder.resolver(move |netloc: &str| client.resolve(netloc)).build()
}

/// [`call_with_retry_n`] with the pipeline's standard 3 attempts (1 initial
/// + 2 retries) — every page and asset fetch goes through this one.
fn call_with_retry(
    url: &str,
    client: &Client,
    timeout: std::time::Duration,
    accept: Option<&str>,
    referer: Option<&str>,
    scope: Option<&UrlScope>,
) -> Result<ureq::Response, FetchError> {
    const TRIES: u32 = 3;
    call_with_retry_n(url, client, timeout, TRIES, accept, referer, scope)
}

/// Translates a fetch's `Result` into the bare `Result<(), Option<u16>>`
/// [`HostPacer::observe`] reacts to — `status` is read off
/// [`FetchError::status`], the typed code [`call_with_retry_n`] captured
/// straight from `ureq::Error::Status`, not re-derived from the message text
/// a wording change could drift out from under a string match. The
/// grow/decay decision itself lives on `HostPacer` (`crawl_state.rs`) so the
/// pacing module stays usable without this file's HTTP types in scope; this
/// is the one line of glue between them, called right after every page or
/// asset fetch in the crawl loop.
pub(crate) fn observe_pace<T>(pacer: &mut HostPacer, host: &str, result: &Result<T, FetchError>) {
    pacer.observe(host, result.as_ref().map(|_| ()).map_err(|e| e.status));
}

/// Fetch a page's body, alongside its declared Content-Type — the caller
/// decides whether the response is a page worth importing at all
/// ([`looks_like_html_page`]) before doing anything else with the body.
///
/// `accept: None` — a page fetch is never asked to localize a transcoded
/// variant, so it keeps ureq's own `Accept: */*` default unchanged.
///
/// `scope` is the imported site's when `url` is a link found on it: a
/// redirect from such a link to another host or to a `mailto:`/`tel:` address
/// is not requested, and the address it pointed at comes back as the final URL.
pub(crate) async fn fetch_page(
    url: &str,
    client: &Client,
    scope: Option<&UrlScope>,
) -> Result<(String, String, String), FetchError> {
    let url = url.to_string();
    let client = client.clone();
    let scope = scope.cloned();
    tokio::task::spawn_blocking(move || {
        let response = call_with_retry(
            &url,
            &client,
            std::time::Duration::from_secs(30),
            None,
            None,
            scope.as_ref(),
        )?;
        let content_type = response.content_type().to_string();
        let final_url = crate::system::redirects::redirect_target(&response)
            .unwrap_or_else(|| response.get_url().to_string());

        response
            .into_string()
            .map(|body| (body, content_type, final_url))
            .map_err(|e| FetchError::message(format!("Failed to read response: {}", e)))
    })
    .await
    .map_err(|e| FetchError::message(format!("Task error: {}", e)))?
}

/// Bytes read from [`fetch_raw`]/[`fetch_raw_bytes`] beyond this are refused
/// rather than buffered — mirrors ureq's own `into_string` cap
/// (`INTO_STRING_LIMIT`, 10 MiB), so reading raw bytes instead of a String
/// (needed to detect a gzip-compressed sitemap before any lossy UTF-8
/// decoding happens) doesn't also drop the memory bound that method gave
/// every other fetch in this file for free. A hostile or misconfigured
/// sitemap is refused the same way an oversized ordinary page response
/// already is, rather than buffered without limit.
const MAX_RAW_FETCH_BYTES: usize = 10 * 1024 * 1024;

/// A URL's raw response bytes, in a single attempt — the primitive
/// [`fetch_raw`] wraps with a lossy UTF-8 decode, and that a sitemap
/// document fetch calls directly instead so gzip-compressed bytes (see
/// `sitemap::decode_sitemap_bytes`) survive to be decompressed before any
/// text conversion happens. See [`fetch_raw`] for why a single attempt, no
/// content-type gate, and the same SSRF-revalidating fetch a page fetch
/// itself uses.
pub(crate) async fn fetch_raw_bytes(url: &str, client: &Client) -> Result<Vec<u8>, FetchError> {
    let url = url.to_string();
    let client = client.clone();
    tokio::task::spawn_blocking(move || {
        let response = call_with_retry_n(
            &url,
            &client,
            std::time::Duration::from_secs(15),
            1,
            None,
            None,
            None,
        )?;
        let mut buf = Vec::new();
        response
            .into_reader()
            .take((MAX_RAW_FETCH_BYTES + 1) as u64)
            .read_to_end(&mut buf)
            .map_err(|e| FetchError::message(format!("Failed to read response: {}", e)))?;
        if buf.len() > MAX_RAW_FETCH_BYTES {
            return Err(FetchError::message(format!(
                "response body exceeds {MAX_RAW_FETCH_BYTES} bytes"
            )));
        }
        Ok(buf)
    })
    .await
    .map_err(|e| FetchError::message(format!("Task error: {}", e)))?
}

/// A URL's raw body with no content-type gate, decoded as UTF-8 (lossily —
/// a non-UTF-8 body simply fails whatever the caller does with it next,
/// same as any other malformed document), in a single attempt — for
/// `robots.txt`, which is always plain text, unlike a sitemap document
/// itself (see [`fetch_raw_bytes`], which that fetch uses directly so a
/// gzip-compressed body isn't corrupted by this function's lossy decode
/// before decompression runs). Neither `robots.txt` nor a sitemap document
/// is HTML (so [`looks_like_html_page`] is never asked to judge one) or
/// worth [`call_with_retry`]'s multi-second backoff: most sites declare no
/// sitemap at all, so a 404 here is the ordinary, expected case, not a
/// transient failure worth waiting out before falling through to a
/// link-only crawl.
pub(crate) async fn fetch_raw(url: &str, client: &Client) -> Result<String, FetchError> {
    let bytes = fetch_raw_bytes(url, client).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// A short, stable name for a media URL, so the same image fetched twice
/// lands on one file. Was `scrape/assets.rs`, whose `AssetManager` had no
/// production caller and went with it.
pub(crate) fn hash_url(url: &str) -> String {
    format!("{:016x}", xxhash_rust::xxh3::xxh3_64(url.as_bytes()))
}

/// Largest single file a page's link may pull into the site.
pub(crate) const LINKED_FILE_MAX_BYTES: u64 = 100 * 1024 * 1024;

/// The page's origin as `Referer`; nothing on an https to http downgrade.
fn referer_for(page_url: &str, target_url: &str) -> Option<String> {
    let page = url::Url::parse(page_url).ok()?;
    let target = url::Url::parse(target_url).ok()?;
    if !matches!(page.scheme(), "http" | "https") || (page.scheme() == "https" && target.scheme() == "http") {
        return None;
    }
    Some(format!("{}/", page.origin().ascii_serialization()))
}

/// What a download did: wrote the file, or refused it for its size.
pub(crate) enum Downloaded {
    Saved(String),
    TooLarge,
}

pub(crate) async fn download_asset(
    url: &str,
    assets_dir: &Path,
    client: &Client,
    page_url: Option<&str>,
) -> Result<String, FetchError> {
    match download_to(url, assets_dir, client, page_url, hashed_filename, u64::MAX).await? {
        Downloaded::Saved(filename) => Ok(filename),
        Downloaded::TooLarge => Err(FetchError::message(format!("{url} is too large to download"))),
    }
}

fn hashed_filename(url: &str, _assets_dir: &Path, content_type: &str) -> String {
    format!("{}.{}", hash_url(url), content_type_to_extension(content_type))
}

/// [`download_asset`] for a file a page links to (a PDF, a score, a
/// document): same request, same folder, but the file is saved under
/// `filename` (see [`linked_filename`]) rather than a hash. A file over
/// `max_bytes` is not written.
pub(crate) async fn download_linked_file(
    url: &str,
    assets_dir: &Path,
    client: &Client,
    page_url: Option<&str>,
    max_bytes: u64,
    filename: String,
) -> Result<Downloaded, FetchError> {
    download_to(url, assets_dir, client, page_url, move |_, _, _| filename, max_bytes).await
}

/// The name a linked file keeps: the last path segment of its URL,
/// decoded and passed through the importer's filename policy, so
/// `/s/Violin%20Part.PDF` is `Violin Part.pdf`. A URL with no usable name
/// falls back to its hash.
pub(crate) fn linked_filename(url: &str) -> String {
    let last_segment = url::Url::parse(url)
        .ok()
        .and_then(|u| u.path_segments().and_then(|s| s.last().map(str::to_string)))
        .unwrap_or_default();
    let decoded = urlencoding::decode(&last_segment)
        .map(|d| d.into_owned())
        .unwrap_or(last_segment);
    let (stem, ext) = match decoded.rsplit_once('.') {
        Some((s, e)) if !e.is_empty() => (s.to_string(), e.to_ascii_lowercase()),
        _ => (decoded, "bin".to_string()),
    };
    let stem = super::writer::sanitize_filename(&stem);
    if stem.is_empty() {
        format!("{}.{}", hash_url(url), ext)
    } else {
        format!("{stem}.{ext}")
    }
}

async fn download_to(
    url: &str,
    assets_dir: &Path,
    client: &Client,
    page_url: Option<&str>,
    name: impl FnOnce(&str, &Path, &str) -> String + Send + 'static,
    max_bytes: u64,
) -> Result<Downloaded, FetchError> {
    let referer = page_url.and_then(|page| referer_for(page, url));
    let url_clone = url.to_string();
    let assets_dir = assets_dir.to_path_buf();
    let client = client.clone();

    tokio::task::spawn_blocking(move || {
        // `Some(MEDIA_ACCEPT)`: a media download wants the file as uploaded,
        // not whatever format a negotiating CDN would substitute for ureq's
        // own `Accept: */*` default — see that constant's doc comment.
        let response = call_with_retry(
            &url_clone,
            &client,
            std::time::Duration::from_secs(60),
            Some(crate::vault::import::media::MEDIA_ACCEPT),
            referer.as_deref(),
            None,
        )?;

        if response
            .header("content-length")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .is_some_and(|len| len > max_bytes)
        {
            return Ok(Downloaded::TooLarge);
        }
        let filename = name(&url_clone, &assets_dir, response.content_type());

        // A response without a content-length is buffered up to the cap.
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| FetchError::message(format!("Failed to read asset: {}", e)))?;
        if bytes.len() as u64 > max_bytes {
            return Ok(Downloaded::TooLarge);
        }

        // Made with its first file: an import with no media leaves no empty folder.
        fs::create_dir_all(&assets_dir)
            .map_err(|e| FetchError::message(format!("Failed to create assets directory: {e}")))?;
        let file_path = assets_dir.join(&filename);
        // allow:raw_write the author's own source content, not `.moss/build.nosync/` output — a downloaded media file
        fs::write(&file_path, &bytes)
            .map_err(|e| FetchError::message(format!("Failed to write asset: {}", e)))?;

        Ok(Downloaded::Saved(filename))
    })
    .await
    .map_err(|e| FetchError::message(format!("Task error: {}", e)))?
}

pub(crate) fn content_type_to_extension(content_type: &str) -> &str {
    match content_type {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/ogg" => "ogg",
        "audio/wav" => "wav",
        "application/pdf" => "pdf",
        _ => "bin",
    }
}

// ── Progress line for a slow page ────────────────────────────────────

/// Quiet period between two lines, and the time a page's downloads may take
/// before the first one.
const INTERVAL: Duration = Duration::from_secs(30);

/// The line to print now, or `None` when `since_last_line` is under 30 s or
/// nothing is left to download. `delay` is the host's current spacing between
/// requests; zero leaves the spacing and the estimate out.
pub(crate) fn progress_line(done: usize, total: usize, delay: Duration, since_last_line: Duration) -> Option<String> {
    if since_last_line < INTERVAL || done >= total {
        return None;
    }
    let mut line = format!("    ↳ {done} of {total} files for this page");
    if !delay.is_zero() {
        let left = delay.saturating_mul((total - done) as u32);
        line.push_str(&format!(
            "; requests are {} apart, about {} left",
            human(delay),
            human(left)
        ));
    }
    Some(line)
}

fn human(d: Duration) -> String {
    let secs = d.as_secs_f64().round() as u64;
    match secs {
        0..=89 => format!("{secs} s"),
        90..=5399 => format!("{} min", (secs + 30) / 60),
        _ => format!("{:.1} h", secs as f64 / 3600.0),
    }
}

/// Counts one page's file downloads as they finish and prints the line.
pub(crate) struct FileProgress {
    total: usize,
    done: usize,
    last_line: Instant,
}

impl FileProgress {
    pub(crate) fn new(total: usize) -> Self {
        Self { total, done: 0, last_line: Instant::now() }
    }

    /// Call after each download, successful or not.
    pub(crate) fn finished_one(&mut self, delay: Duration) {
        self.done += 1;
        if let Some(line) = progress_line(self.done, self.total, delay, self.last_line.elapsed()) {
            eprintln!("{line}");
            self.last_line = Instant::now();
        }
    }
}

#[cfg(test)]
#[path = "fetch_tests.rs"]
mod tests;
