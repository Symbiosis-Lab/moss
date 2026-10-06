//! Loop-scoped state for one recursive crawl ([`super::run::scrape_to_folder`]),
//! carved into one small record per concern instead of a dozen-plus loose
//! mutables threaded through the loop by hand. Each sub-record answers to
//! its own rules as methods, so the crawl loop reads as a sequence of calls
//! and the rules themselves — the duplicate checks, the cap math, the
//! known-variant check — are unit-testable without an HTTP-mocked crawl.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use super::crawler::{host_of, path_identity};
use super::sitemap::{block_if_locale_alternate, SitemapDiscovery};
use crate::vault::import::widgets::WidgetCount;

/// The crawl's frontier (cap bookkeeping), duplicate detection, the asset
/// map, the outcome tally, and per-host request pacing — everything the
/// loop in `scrape_to_folder` mutates across iterations.
pub(crate) struct CrawlState {
    pub(crate) frontier: Frontier,
    pub(crate) cap: CapBudget,
    pub(crate) dedupe: Dedupe,
    pub(crate) assets: AssetMap,
    pub(crate) tally: Tally,
    pub(crate) pacer: HostPacer,
}

impl CrawlState {
    /// Seeds the frontier with the start URL followed by every
    /// sitemap-declared URL — ahead of anything link discovery adds later,
    /// so a sitemap URL wins survivor choice over a link-discovered
    /// duplicate of the same page (see [`Dedupe::is_duplicate_identity`]) —
    /// and exempts every sitemap URL from the page cap (rule 3: a site's
    /// declared page list is always imported). The start URL's host also
    /// seeds the pacer's floor from the site's own declared `Crawl-delay`,
    /// if `robots.txt` named one, so a site that already told every crawler
    /// how fast it wants to be hit is never paced faster than that — even
    /// before this crawl has seen a single 429.
    pub(crate) fn new(start_url: &str, sitemap: &SitemapDiscovery) -> Self {
        let declared: HashSet<String> = sitemap.urls.iter().cloned().collect();
        let mut pacer = HostPacer::default();
        if let Some(floor) = sitemap.crawl_delay {
            pacer.set_floor(&host_of(start_url), floor);
        }
        Self {
            frontier: Frontier::seeded(start_url.to_string(), &sitemap.urls),
            cap: CapBudget::new(declared),
            dedupe: Dedupe::default(),
            assets: AssetMap::default(),
            tally: Tally::default(),
            pacer,
        }
    }

    /// A discovered link (or manifest-declared page) the sitemap itself
    /// already named as a `hreflang` alternate of a page it declared — see
    /// [`block_if_locale_alternate`]. The one place this crawl's frontier
    /// and duplicate tally are both touched from outside their own records,
    /// so the loop itself never reaches into either directly.
    pub(crate) fn block_locale_alternate(
        &mut self,
        link: &str,
        locale_alternates: &HashSet<String>,
    ) -> bool {
        block_if_locale_alternate(
            link,
            locale_alternates,
            &mut self.frontier.visited,
            &mut self.tally.duplicate,
        )
    }
}

/// The crawl's BFS queue and the set of URLs already popped and processed.
/// A URL entering the queue more than once — two sibling pages that both
/// link to it — is ordinary; `visited` is what keeps it from being fetched
/// twice.
#[derive(Default)]
pub(crate) struct Frontier {
    queue: VecDeque<String>,
    visited: HashSet<String>,
}

impl Frontier {
    fn seeded(start_url: String, sitemap_urls: &[String]) -> Self {
        let mut queue = VecDeque::new();
        queue.push_back(start_url);
        queue.extend(sitemap_urls.iter().cloned());
        Self { queue, visited: HashSet::new() }
    }

    /// Next URL to process, or `None` once the queue is empty.
    pub(crate) fn pop(&mut self) -> Option<String> {
        self.queue.pop_front()
    }

    pub(crate) fn is_visited(&self, url: &str) -> bool {
        self.visited.contains(url)
    }

    /// Marks `url` processed — called once, right before it is fetched, so
    /// a duplicate-queued copy of it is skipped by `is_visited` instead of
    /// being fetched twice.
    pub(crate) fn visit(&mut self, url: &str) {
        self.visited.insert(url.to_string());
    }

    /// Puts a popped-but-not-yet-processed URL back at the front — used
    /// only when the page cap stops the crawl mid-pop, so the leftover
    /// count `remaining_unvisited_count` computes still includes it.
    pub(crate) fn requeue_front(&mut self, url: String) {
        self.queue.push_front(url);
    }

    /// Queues `url` unless it has already been processed. Scope is not
    /// this type's concern — the caller filters with `is_within_scope`
    /// first.
    pub(crate) fn enqueue_if_unvisited(&mut self, url: String) {
        if !self.visited.contains(&url) {
            self.queue.push_back(url);
        }
    }

    /// Discovered, in-scope URLs left behind when the cap stopped the
    /// crawl — deduplicated, since the same not-yet-visited URL can have
    /// been queued more than once by different pages that both linked to it
    /// before either was processed.
    pub(crate) fn remaining_unvisited_count(&self) -> usize {
        self.queue.iter().filter(|u| !self.visited.contains(*u)).collect::<HashSet<_>>().len()
    }
}

/// Page-cap bookkeeping. A sitemap-declared URL is exempt from the cap
/// (rule 3), so `progress` tracks only link-discovered pages — it
/// undercounts the crawl's real page count by however many were declared,
/// which is why the loop reads `is_declared` before deciding whether to
/// touch this record at all.
pub(crate) struct CapBudget {
    declared: HashSet<String>,
    progress: usize,
    capped: bool,
}

impl CapBudget {
    fn new(declared: HashSet<String>) -> Self {
        Self { declared, progress: 0, capped: false }
    }

    pub(crate) fn is_declared(&self, url: &str) -> bool {
        self.declared.contains(url)
    }

    /// False once `progress` has reached `max_pages`. `max_pages: None`
    /// means uncapped — always true.
    pub(crate) fn has_room(&self, max_pages: Option<usize>) -> bool {
        !max_pages.is_some_and(|cap| self.progress >= cap)
    }

    pub(crate) fn mark_capped(&mut self) {
        self.capped = true;
    }

    pub(crate) fn capped(&self) -> bool {
        self.capped
    }

    /// Counts one more link-discovered page against the cap — a no-op for
    /// a sitemap-declared one, which never counts against it.
    pub(crate) fn record_progress(&mut self, is_declared: bool) {
        if !is_declared {
            self.progress += 1;
        }
    }
}

/// The two duplicate rules and the paths they've confirmed, so a later
/// failed fetch on one of those paths can tell a query-string variant of
/// content already on disk from a page genuinely gone missing (see
/// [`Dedupe::is_known_variant`]).
#[derive(Default)]
pub(crate) struct Dedupe {
    /// Identity of every page WRITTEN so far — its own canonical URL when
    /// it declared one, else the URL it was fetched from.
    imported_identities: HashSet<String>,
    /// Body hashes of every page WRITTEN so far, keyed by its own
    /// `path_identity`. Consulted only by `is_duplicate_body`, the
    /// fallback for a page that declares no canonical of its own.
    written_path_bodies: HashMap<String, HashSet<u64>>,
    /// Path identities where this crawl has already CONFIRMED a duplicate —
    /// set by both rules below, read only by `is_known_variant`.
    known_duplicate_paths: HashSet<String>,
}

impl Dedupe {
    /// Rule 1: a canonical identity already imported makes this page a
    /// duplicate of it. Compared only against identities already WRITTEN,
    /// never against an unvisited target still sitting in the queue — a
    /// page whose own canonical target later fails to fetch must still be
    /// written itself, not dropped on a bet that never paid off. Call this
    /// before any of the more expensive snapshot-fetch/article-extraction
    /// work runs, so a duplicate never pays for work it will discard.
    pub(crate) fn is_duplicate_identity(&mut self, url: &str, canonical: &str) -> bool {
        let is_dup = self.imported_identities.contains(canonical);
        if is_dup {
            self.confirm_duplicate_path(url);
        }
        is_dup
    }

    /// Rule 2, the fallback for a page that declares no canonical of its
    /// own: a body byte-identical to an already-written page's at the SAME
    /// path (query string and fragment ignored) is a query-string variant
    /// of it. The path match is required — two different real pages that
    /// happen to render the same short templated body must never collapse
    /// into one.
    pub(crate) fn is_duplicate_body(&mut self, url: &str, body_hash: u64) -> bool {
        let Some(id) = path_identity(url) else { return false };
        let is_dup =
            self.written_path_bodies.get(&id).is_some_and(|hashes| hashes.contains(&body_hash));
        if is_dup {
            self.known_duplicate_paths.insert(id);
        }
        is_dup
    }

    /// A URL whose fetch failed outright but whose own path has already
    /// been CONFIRMED (by either rule above) to produce duplicates — a
    /// query-string variant of content already safely on disk, not a page
    /// gone missing. Deliberately narrower than "any path that has
    /// produced a written page": an old-CMS site that routes distinct
    /// posts through the same path (`?p=1`, `?p=2`, each self-canonical)
    /// must not lose a later sibling's failure stub just because an
    /// earlier sibling at that path happened to import first.
    pub(crate) fn is_known_variant(&self, url: &str) -> bool {
        path_identity(url).is_some_and(|id| self.known_duplicate_paths.contains(&id))
    }

    /// Records a just-written page's identity so a later duplicate of it —
    /// by canonical (rule 1) or by same-path body (rule 2) — is caught.
    /// Recorded regardless of whether THIS page itself had a canonical:
    /// rule 2 matches against any already-written page at that path, not
    /// only ones that also lacked a canonical of their own.
    pub(crate) fn record_written(&mut self, url: &str, canonical: Option<&str>, body_hash: u64) {
        self.imported_identities
            .insert(canonical.map(str::to_string).unwrap_or_else(|| url.to_string()));
        if let Some(id) = path_identity(url) {
            self.written_path_bodies.entry(id).or_default().insert(body_hash);
        }
    }

    /// The one place `known_duplicate_paths` is written from a confirmed
    /// rule-1 match. Rule 2 already has its path id in hand from its own
    /// lookup and inserts it directly rather than recomputing it here —
    /// both rules used to paste this same insert themselves.
    fn confirm_duplicate_path(&mut self, url: &str) {
        if let Some(id) = path_identity(url) {
            self.known_duplicate_paths.insert(id);
        }
    }
}

/// Remote media URL → local relative path, filled in as each page's media
/// downloads. Read by `compose_note`, which is shared with the single-file
/// import arm — that arm builds its own short-lived map with no crawl
/// state at all, so this type exists only for the crawl's loop-scoped one.
#[derive(Default)]
pub(crate) struct AssetMap(HashMap<String, String>);

impl AssetMap {
    pub(crate) fn contains(&self, remote: &str) -> bool {
        self.0.contains_key(remote)
    }

    pub(crate) fn insert(&mut self, remote: String, local: String) {
        self.0.insert(remote, local);
    }

    pub(crate) fn as_map(&self) -> &HashMap<String, String> {
        &self.0
    }
}

/// Doubling step a host's pacing interval grows to on its FIRST 429/503 —
/// multiplying a ZERO interval by two would never leave zero, so growth
/// needs a nonzero seed before it has anything to double from. Matches the
/// retry backoff's own first step (`run.rs`'s `call_with_retry_n`), so a
/// host that has started rate-limiting sees one familiar number either way.
const PACING_START_INTERVAL: Duration = Duration::from_millis(500);

/// Ceiling the BACKOFF growth in [`HostPacer::record_rate_limited`] will
/// ever double `interval` past — repeated 429/503s are this crawl's own
/// retry budget failing over and over, and a crawl backing off forever on
/// that alone is indistinguishable from one that gave up. Does NOT bound
/// [`HostPacer::set_floor`] — a site's declared `Crawl-delay` is a request,
/// not a failure signal, and is held to its own, more generous
/// [`MAX_CRAWL_DELAY_FLOOR`] instead. `record_rate_limited` and
/// `record_success` both still clamp the result to `floor` afterward, so a
/// host whose floor exceeds this cap is never paced faster than its floor
/// even while backing off.
const MAX_PACING_INTERVAL: Duration = Duration::from_secs(2);

/// Ceiling a `robots.txt` `Crawl-delay` floor ([`HostPacer::set_floor`]) is
/// held to — mirrors `run.rs`'s `MAX_RETRY_AFTER` (also 60s, also a
/// server-declared wait this crawl honors up to a point): a host naming an
/// hour is asking the wrong tool, and a crawl that waited it out verbatim
/// could stall the whole run on one host. Deliberately a separate, larger
/// constant from [`MAX_PACING_INTERVAL`] rather than reusing it — a site
/// that explicitly told every crawler "wait N seconds" gets more deference
/// than this crawl's own backoff growth from repeated failures does; capping
/// both at the same 2s would mean a site asking for anything past that is
/// paced faster than it asked, silently breaking the promise `CrawlState::
/// new`'s doc comment makes ("never paced faster than that").
const MAX_CRAWL_DELAY_FLOOR: Duration = Duration::from_secs(60);

/// Per-host request pacing: a minimum interval enforced between requests to
/// the same host, so a recursive crawl's own request rate backs off
/// automatically once a host starts returning 429/503, instead of hammering
/// it at the pipeline's full speed until every retry budget
/// (`call_with_retry_n`, in `run.rs`) is spent. Starts at zero for a host
/// never rate-limited — an ordinary site is never slowed down — grows on
/// 429/503 ([`record_rate_limited`](HostPacer::record_rate_limited)), and
/// decays back down on success
/// ([`record_success`](HostPacer::record_success)), never below `floor` (a
/// site's own declared `robots.txt` `Crawl-delay`, zero when it named
/// none). Every mutator maintains `interval >= floor` as it goes, so a
/// reader never needs to re-apply the floor itself.
///
/// Pure and time-injected — `now` is always a caller-supplied [`Instant`],
/// never read internally — so the backoff/decay math is unit-testable
/// without a real sleep; `run.rs`'s `wait_for_pace` is what actually calls
/// [`Instant::now`] and sleeps.
#[derive(Default)]
pub(crate) struct HostPacer {
    hosts: HashMap<String, HostPace>,
}

#[derive(Clone, Copy)]
struct HostPace {
    /// Current minimum spacing between requests to this host.
    interval: Duration,
    /// Highest `interval` this host has ever reached this crawl, kept even
    /// after a later success decays `interval` back down — what
    /// [`HostPacer::rate_limited_hosts`] reports, since "this host needed
    /// slowing down" is the fact worth surfacing, not whatever the interval
    /// happened to be at the very last request.
    peak: Duration,
    /// A hard minimum `interval` is never decayed below — see
    /// [`HostPacer::set_floor`].
    floor: Duration,
    /// When the last request to this host went out, so the next one can be
    /// measured against it. `None` for a host no request has been sent to
    /// yet, and [`HostPacer::wait_duration`] always returns zero for one.
    last_request: Option<Instant>,
}

impl Default for HostPace {
    fn default() -> Self {
        Self {
            interval: Duration::ZERO,
            peak: Duration::ZERO,
            floor: Duration::ZERO,
            last_request: None,
        }
    }
}

impl HostPacer {
    /// How long the caller must still wait, from `now`, before a request to
    /// `host` honors its pacing interval. Zero for a host never seen before,
    /// or whose last request was already far enough in the past.
    pub(crate) fn wait_duration(&self, host: &str, now: Instant) -> Duration {
        let Some(pace) = self.hosts.get(host) else { return Duration::ZERO };
        let Some(last) = pace.last_request else { return Duration::ZERO };
        // `interval` is always already `>= floor` — every mutator below
        // maintains that — so this reads it directly rather than
        // re-applying `.max(floor)`.
        pace.interval.saturating_sub(now.saturating_duration_since(last))
    }

    /// Records that a request to `host` is going out at `now` — called
    /// right before the request, once `wait_duration`'s wait (if any) has
    /// elapsed, so the NEXT request's `wait_duration` measures from it.
    pub(crate) fn record_request(&mut self, host: &str, now: Instant) {
        self.hosts.entry(host.to_string()).or_default().last_request = Some(now);
    }

    /// Sleeps out `host`'s current pacing interval, if any, then records a
    /// request about to go out at the moment the wait ends — call right
    /// before every page or asset fetch in the crawl loop, so a host's own
    /// politeness interval (grown by a PRIOR fetch to it — see
    /// [`observe`](HostPacer::observe)) is honored before the NEXT request
    /// reaches the network, rather than only slowing the one request already
    /// retrying against it. A no-op for a host this crawl has never slowed
    /// down, so an ordinary site's requests go out exactly as fast as before
    /// pacing existed. The crate's one real sleep, kept on this type rather
    /// than the crawl loop (`run.rs`) so the loop reads as one call instead
    /// of the wait/record pair — [`wait_duration`](HostPacer::wait_duration)
    /// and [`record_request`](HostPacer::record_request) remain separately
    /// callable for the timing math the unit tests above exercise without a
    /// real sleep.
    pub(crate) async fn wait(&mut self, host: &str) {
        let wait = self.wait_duration(host, Instant::now());
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        self.record_request(host, Instant::now());
    }

    /// Reacts to one fetch's terminal outcome against `host`: `Ok(())`
    /// decays the interval (the host is no longer under strain); a status
    /// this crawl reads as the host asking it to slow down (429/503) grows
    /// it; anything else — no status at all (a transport error, a redirect
    /// refusal, a body-read failure), or a status outside that pair (a 404,
    /// a permanent 400) — leaves the interval alone, since none of those is
    /// a signal from the HOST that this crawl is going too fast, only that
    /// this one request failed for its own reason. Takes the bare status
    /// rather than an HTTP-client error type so this pacing module stays
    /// usable without ureq in scope — `fetch.rs`'s `observe_pace` does the
    /// one-line translation from its own `FetchError`.
    pub(crate) fn observe(&mut self, host: &str, outcome: Result<(), Option<u16>>) {
        match outcome {
            Ok(()) => self.record_success(host),
            Err(Some(429)) | Err(Some(503)) => self.record_rate_limited(host),
            Err(_) => {}
        }
    }

    /// Doubles `host`'s pacing interval (seeding it to
    /// [`PACING_START_INTERVAL`] first, if it was still zero), capped at
    /// [`MAX_PACING_INTERVAL`] — called after a request to `host` comes back
    /// 429 or 503 having outlasted every retry.
    pub(crate) fn record_rate_limited(&mut self, host: &str) {
        let pace = self.hosts.entry(host.to_string()).or_default();
        let grown = if pace.interval.is_zero() {
            PACING_START_INTERVAL
        } else {
            pace.interval * 2
        };
        pace.interval = grown.min(MAX_PACING_INTERVAL).max(pace.floor);
        pace.peak = pace.peak.max(pace.interval);
    }

    /// Halves `host`'s pacing interval — called after a request to `host`
    /// succeeds, so a host that has gone quiet is gradually trusted with a
    /// faster request rate again. Never drops below `floor`. A no-op host
    /// this pacer has never touched simply stays at zero.
    pub(crate) fn record_success(&mut self, host: &str) {
        if let Some(pace) = self.hosts.get_mut(host) {
            pace.interval = (pace.interval / 2).max(pace.floor);
        }
    }

    /// Sets `host`'s hard floor — a site's own declared `robots.txt`
    /// `Crawl-delay`, capped at [`MAX_CRAWL_DELAY_FLOOR`] rather than the
    /// (much smaller) backoff ceiling [`MAX_PACING_INTERVAL`] — and raises
    /// its current interval to at least that floor immediately, rather than
    /// waiting for the next grow/decay event to notice it.
    pub(crate) fn set_floor(&mut self, host: &str, floor: Duration) {
        let floor = floor.min(MAX_CRAWL_DELAY_FLOOR);
        let pace = self.hosts.entry(host.to_string()).or_default();
        pace.floor = floor;
        pace.interval = pace.interval.max(floor);
    }

    /// `host`'s current pacing interval — zero for a host never seen or
    /// never rate-limited. Exposed for tests; the crawl loop itself only
    /// ever needs `wait_duration`.
    #[cfg(test)]
    pub(crate) fn current_interval(&self, host: &str) -> Duration {
        self.hosts.get(host).map(|pace| pace.interval).unwrap_or(Duration::ZERO)
    }

    /// Every host this pacer ever rate-limited, with the peak interval it
    /// reached — sorted by host for a deterministic report. Empty when no
    /// host this crawl talked to ever returned 429/503.
    pub(crate) fn rate_limited_hosts(&self) -> Vec<(String, Duration)> {
        let mut out: Vec<(String, Duration)> = self
            .hosts
            .iter()
            .filter(|(_, pace)| !pace.peak.is_zero())
            .map(|(host, pace)| (host.clone(), pace.peak))
            .collect();
        out.sort();
        out
    }
}

/// The six mutually-exclusive outcomes one popped URL can land in
/// ([`super::run::PageOutcome`]). Never derived from a set's size — e.g.
/// two duplicates can share one path identity in `Dedupe`, so
/// `known_duplicate_paths.len()` is not `duplicate`.
#[derive(Default)]
pub(crate) struct Tally {
    scraped: usize,
    failed: usize,
    skipped: usize,
    duplicate: usize,
    unreachable_variants: usize,
    unreachable_files: usize,
    widgets_carried: usize,
    widgets_dropped: usize,
}

impl Tally {
    pub(crate) fn scraped(&self) -> usize {
        self.scraped
    }

    pub(crate) fn failed(&self) -> usize {
        self.failed
    }

    pub(crate) fn skipped(&self) -> usize {
        self.skipped
    }

    pub(crate) fn duplicate(&self) -> usize {
        self.duplicate
    }

    pub(crate) fn unreachable_variants(&self) -> usize {
        self.unreachable_variants
    }

    pub(crate) fn unreachable_files(&self) -> usize {
        self.unreachable_files
    }

    pub(crate) fn widgets_carried(&self) -> usize {
        self.widgets_carried
    }

    pub(crate) fn widgets_dropped(&self) -> usize {
        self.widgets_dropped
    }

    pub(crate) fn record_scraped(&mut self) {
        self.scraped += 1;
    }

    pub(crate) fn record_failed(&mut self) {
        self.failed += 1;
    }

    pub(crate) fn record_skipped(&mut self) {
        self.skipped += 1;
    }

    pub(crate) fn record_duplicate(&mut self) {
        self.duplicate += 1;
    }

    pub(crate) fn record_unreachable_variant(&mut self) {
        self.unreachable_variants += 1;
    }

    pub(crate) fn record_unreachable_file(&mut self) {
        self.unreachable_files += 1;
    }

    pub(crate) fn record_widgets(&mut self, found: &WidgetCount) {
        self.widgets_carried += found.carried;
        self.widgets_dropped += found.dropped;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── CrawlState::new (the Crawl-delay-to-pacer wiring) ───────────────

    #[test]
    fn crawl_state_new_seeds_the_pacer_floor_from_a_sitemap_crawl_delay() {
        let sitemap = SitemapDiscovery {
            crawl_delay: Some(Duration::from_secs(1)),
            ..SitemapDiscovery::default()
        };
        let state = CrawlState::new("https://example.test/", &sitemap);
        assert_eq!(
            state.pacer.current_interval("example.test"),
            Duration::from_secs(1),
            "a declared Crawl-delay must floor the pacer before any request is made"
        );
    }

    #[test]
    fn crawl_state_new_leaves_the_pacer_unpaced_when_no_crawl_delay_is_declared() {
        let state = CrawlState::new("https://example.test/", &SitemapDiscovery::default());
        assert_eq!(state.pacer.current_interval("example.test"), Duration::ZERO);
    }

    // ── Frontier ──────────────────────────────────────────────────────

    #[test]
    fn frontier_seeds_the_start_url_ahead_of_sitemap_urls() {
        let mut frontier = Frontier::seeded(
            "https://example.com/".to_string(),
            &["https://example.com/a".to_string(), "https://example.com/b".to_string()],
        );
        assert_eq!(frontier.pop().as_deref(), Some("https://example.com/"));
        assert_eq!(frontier.pop().as_deref(), Some("https://example.com/a"));
        assert_eq!(frontier.pop().as_deref(), Some("https://example.com/b"));
        assert_eq!(frontier.pop(), None);
    }

    #[test]
    fn frontier_enqueue_if_unvisited_skips_an_already_visited_url() {
        let mut frontier = Frontier::default();
        frontier.visit("https://example.com/a");
        frontier.enqueue_if_unvisited("https://example.com/a".to_string());
        frontier.enqueue_if_unvisited("https://example.com/b".to_string());
        assert_eq!(frontier.pop().as_deref(), Some("https://example.com/b"));
        assert_eq!(frontier.pop(), None, "the already-visited URL must never be queued");
    }

    #[test]
    fn frontier_remaining_unvisited_count_dedupes_the_queue() {
        let mut frontier = Frontier::default();
        frontier.enqueue_if_unvisited("https://example.com/a".to_string());
        frontier.enqueue_if_unvisited("https://example.com/a".to_string());
        frontier.enqueue_if_unvisited("https://example.com/b".to_string());
        frontier.visit("https://example.com/b");
        assert_eq!(
            frontier.remaining_unvisited_count(),
            1,
            "the duplicate queue entry collapses and the visited one doesn't count"
        );
    }

    // ── CapBudget ─────────────────────────────────────────────────────

    #[test]
    fn cap_budget_has_room_until_progress_reaches_the_cap() {
        let mut cap = CapBudget::new(HashSet::new());
        assert!(cap.has_room(Some(2)));
        cap.record_progress(false);
        assert!(cap.has_room(Some(2)));
        cap.record_progress(false);
        assert!(!cap.has_room(Some(2)), "progress has reached the cap");
    }

    #[test]
    fn cap_budget_has_room_is_always_true_with_no_cap() {
        let mut cap = CapBudget::new(HashSet::new());
        for _ in 0..1000 {
            cap.record_progress(false);
        }
        assert!(cap.has_room(None));
    }

    #[test]
    fn cap_budget_a_declared_url_never_consumes_progress() {
        let mut cap = CapBudget::new(HashSet::new());
        cap.record_progress(true);
        cap.record_progress(true);
        assert!(cap.has_room(Some(1)), "declared pages must never count against the cap");
    }

    #[test]
    fn cap_budget_is_declared_reads_the_seeded_set() {
        let mut declared = HashSet::new();
        declared.insert("https://example.com/a".to_string());
        let cap = CapBudget::new(declared);
        assert!(cap.is_declared("https://example.com/a"));
        assert!(!cap.is_declared("https://example.com/b"));
    }

    // ── Dedupe ────────────────────────────────────────────────────────

    #[test]
    fn dedupe_rule_1_matches_an_already_imported_canonical() {
        let mut dedupe = Dedupe::default();
        dedupe.record_written("https://example.com/real", None, 1);
        assert!(
            dedupe.is_duplicate_identity("https://example.com/variant", "https://example.com/real")
        );
        assert!(
            !dedupe.is_duplicate_identity("https://example.com/other", "https://example.com/nope"),
            "an unimported canonical is not a duplicate"
        );
    }

    #[test]
    fn dedupe_rule_2_matches_the_same_path_with_an_identical_body() {
        let mut dedupe = Dedupe::default();
        dedupe.record_written("https://example.com/note", None, 42);
        assert!(dedupe.is_duplicate_body("https://example.com/note?ref=share", 42));
    }

    #[test]
    fn dedupe_rule_2_never_matches_a_different_path() {
        let mut dedupe = Dedupe::default();
        dedupe.record_written("https://example.com/boston", None, 7);
        assert!(
            !dedupe.is_duplicate_body("https://example.com/chicago", 7),
            "an identical body at a different path must not collapse two real pages"
        );
    }

    #[test]
    fn dedupe_is_known_variant_only_after_a_confirmed_duplicate_on_that_path() {
        let mut dedupe = Dedupe::default();
        assert!(!dedupe.is_known_variant("https://example.com/note?itemId=2"));
        dedupe.record_written("https://example.com/note", None, 9);
        dedupe.is_duplicate_body("https://example.com/note?itemId=1", 9);
        assert!(
            dedupe.is_known_variant("https://example.com/note?itemId=2"),
            "a confirmed duplicate anywhere on this path marks the whole path"
        );
    }

    // ── AssetMap ──────────────────────────────────────────────────────

    #[test]
    fn asset_map_records_and_looks_up_a_local_path() {
        let mut assets = AssetMap::default();
        assert!(!assets.contains("https://example.com/a.png"));
        assets
            .insert("https://example.com/a.png".to_string(), "./assets/imported/x.png".to_string());
        assert!(assets.contains("https://example.com/a.png"));
        assert_eq!(
            assets.as_map().get("https://example.com/a.png").map(String::as_str),
            Some("./assets/imported/x.png")
        );
    }

    // ── HostPacer ─────────────────────────────────────────────────────

    #[test]
    fn host_pacer_never_seen_host_needs_no_wait() {
        let pacer = HostPacer::default();
        assert_eq!(pacer.wait_duration("example.test", Instant::now()), Duration::ZERO);
    }

    /// The core gap: after a host returns 429/503, the NEXT request to it
    /// must be spaced out by the newly-grown interval, not fired off
    /// immediately the way an ordinary site's requests are.
    #[test]
    fn host_pacer_paces_the_next_request_after_a_rate_limit() {
        let mut pacer = HostPacer::default();
        let t0 = Instant::now();
        pacer.record_request("example.test", t0);
        pacer.record_rate_limited("example.test");

        assert_eq!(
            pacer.wait_duration("example.test", t0),
            PACING_START_INTERVAL,
            "right after the rate-limited request, the full interval is still owed"
        );
        assert_eq!(
            pacer.wait_duration("example.test", t0 + PACING_START_INTERVAL / 2),
            PACING_START_INTERVAL / 2,
            "halfway through the interval, half of it remains"
        );
        assert_eq!(
            pacer.wait_duration("example.test", t0 + PACING_START_INTERVAL),
            Duration::ZERO,
            "once the interval has fully elapsed, nothing more is owed"
        );
    }

    #[test]
    fn host_pacer_another_host_is_unaffected() {
        let mut pacer = HostPacer::default();
        let t0 = Instant::now();
        pacer.record_request("limited.test", t0);
        pacer.record_rate_limited("limited.test");
        pacer.record_request("quiet.test", t0);

        assert!(pacer.wait_duration("limited.test", t0) > Duration::ZERO);
        assert_eq!(
            pacer.wait_duration("quiet.test", t0),
            Duration::ZERO,
            "a host this pacer never rate-limited must never be slowed down"
        );
    }

    #[test]
    fn host_pacer_grows_by_doubling_and_never_exceeds_the_cap() {
        let mut pacer = HostPacer::default();
        pacer.record_rate_limited("example.test");
        assert_eq!(pacer.current_interval("example.test"), PACING_START_INTERVAL);
        pacer.record_rate_limited("example.test");
        assert_eq!(pacer.current_interval("example.test"), PACING_START_INTERVAL * 2);
        // Repeated rate limiting must never push the interval past the cap,
        // however many times it fires.
        for _ in 0..10 {
            pacer.record_rate_limited("example.test");
        }
        assert_eq!(pacer.current_interval("example.test"), MAX_PACING_INTERVAL);
    }

    #[test]
    fn host_pacer_decays_on_success_and_bottoms_out_at_zero() {
        let mut pacer = HostPacer::default();
        pacer.record_rate_limited("example.test"); // -> 500ms
        pacer.record_rate_limited("example.test"); // -> 1000ms
        assert_eq!(pacer.current_interval("example.test"), Duration::from_millis(1000));

        pacer.record_success("example.test");
        assert_eq!(pacer.current_interval("example.test"), Duration::from_millis(500));
        pacer.record_success("example.test");
        assert_eq!(pacer.current_interval("example.test"), Duration::from_millis(250));

        // Each success only halves (integer division), so clearing 1000ms
        // of nanosecond-granular duration down to exactly zero takes ~30
        // halvings in total (2^30 > 1_000_000_000) — 40 more is a safe
        // margin, not a tight bound.
        for _ in 0..40 {
            pacer.record_success("example.test");
        }
        assert_eq!(
            pacer.current_interval("example.test"),
            Duration::ZERO,
            "enough successes must fully clear the interval, not just approach zero"
        );
    }

    #[test]
    fn host_pacer_crawl_delay_sets_a_floor_decay_never_crosses() {
        let mut pacer = HostPacer::default();
        pacer.set_floor("example.test", Duration::from_secs(1));
        // The floor applies immediately, before any request or rate limit.
        assert_eq!(pacer.current_interval("example.test"), Duration::from_secs(1));

        for _ in 0..20 {
            pacer.record_success("example.test");
        }
        assert_eq!(
            pacer.current_interval("example.test"),
            Duration::from_secs(1),
            "Crawl-delay is a hard floor — success must never decay below it"
        );
    }

    #[test]
    fn host_pacer_floor_is_capped_at_the_max_crawl_delay_floor() {
        let mut pacer = HostPacer::default();
        pacer.set_floor("example.test", Duration::from_secs(600));
        assert_eq!(pacer.current_interval("example.test"), MAX_CRAWL_DELAY_FLOOR);
    }

    /// The gap a single shared cap would have left: a `Crawl-delay` above
    /// [`MAX_PACING_INTERVAL`] (the much smaller backoff-growth ceiling)
    /// must still win, because the site explicitly asked for it — a
    /// declared delay is a request, not this crawl's own repeated-failure
    /// signal, and must not be paced faster than it names just because that
    /// happens to exceed what unprompted backoff would ever grow to.
    #[test]
    fn host_pacer_crawl_delay_above_the_backoff_cap_still_wins() {
        let mut pacer = HostPacer::default();
        let declared = Duration::from_secs(10);
        assert!(declared > MAX_PACING_INTERVAL, "the case this test exists to cover");
        pacer.set_floor("example.test", declared);
        assert_eq!(
            pacer.current_interval("example.test"),
            declared,
            "a Crawl-delay above the backoff ceiling must not be silently clipped to it"
        );

        // Even while actively rate-limited, the interval never drops below
        // the declared floor — nor does growth get re-clipped to the
        // (smaller) backoff cap on top of it.
        pacer.record_rate_limited("example.test");
        assert_eq!(
            pacer.current_interval("example.test"),
            declared,
            "backoff growth must not override a floor already above its own cap"
        );
    }

    #[test]
    fn host_pacer_rate_limited_hosts_reports_the_peak_not_the_decayed_value() {
        let mut pacer = HostPacer::default();
        pacer.record_rate_limited("a.test"); // -> 500ms
        pacer.record_rate_limited("a.test"); // -> 1000ms
        pacer.record_success("a.test"); // decays back to 500ms
        pacer.record_request("b.test", Instant::now()); // seen, never rate-limited

        assert_eq!(
            pacer.rate_limited_hosts(),
            vec![("a.test".to_string(), Duration::from_millis(1000))],
            "the peak survives the later decay, and an unaffected host is never listed"
        );
    }

    // ── HostPacer::observe (the grow/decay DECISION — `fetch.rs`'s
    // `observe_pace` only translates its own error type into the bare
    // `Result<(), Option<u16>>` this takes, so the decision itself is
    // tested at the layer that makes it) ────────────────────────────────

    #[test]
    fn host_pacer_observe_grows_on_429_or_503() {
        let mut pacer = HostPacer::default();
        pacer.observe("example.test", Err(Some(429)));
        assert!(pacer.current_interval("example.test") > Duration::ZERO);

        let mut pacer = HostPacer::default();
        pacer.observe("example.test", Err(Some(503)));
        assert!(pacer.current_interval("example.test") > Duration::ZERO);
    }

    #[test]
    fn host_pacer_observe_ignores_a_status_outside_429_and_503_and_a_statusless_failure() {
        let mut pacer = HostPacer::default();
        pacer.observe("example.test", Err(Some(404)));
        pacer.observe("example.test", Err(None));
        assert_eq!(
            pacer.current_interval("example.test"),
            Duration::ZERO,
            "a 404, or a failure that carries no status at all (transport error, \
             redirect refusal), is not a rate-limit signal"
        );
    }

    #[test]
    fn host_pacer_observe_decays_on_ok() {
        let mut pacer = HostPacer::default();
        pacer.record_rate_limited("example.test");
        let before = pacer.current_interval("example.test");
        pacer.observe("example.test", Ok(()));
        assert!(pacer.current_interval("example.test") < before);
    }

    // ── Tally ─────────────────────────────────────────────────────────

    #[test]
    fn tally_counts_each_outcome_independently() {
        let mut tally = Tally::default();
        tally.record_scraped();
        tally.record_failed();
        tally.record_failed();
        tally.record_skipped();
        tally.record_duplicate();
        tally.record_unreachable_variant();
        tally.record_unreachable_file();
        assert_eq!(tally.scraped(), 1);
        assert_eq!(tally.failed(), 2);
        assert_eq!(tally.skipped(), 1);
        assert_eq!(tally.duplicate(), 1);
        assert_eq!(tally.unreachable_variants(), 1);
        assert_eq!(tally.unreachable_files(), 1);
    }
}
