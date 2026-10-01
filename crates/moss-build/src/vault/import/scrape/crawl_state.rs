//! Loop-scoped state for one recursive crawl ([`super::run::scrape_to_folder`]),
//! carved into one small record per concern instead of a dozen-plus loose
//! mutables threaded through the loop by hand. Each sub-record answers to
//! its own rules as methods, so the crawl loop reads as a sequence of calls
//! and the rules themselves — the duplicate checks, the cap math, the
//! known-variant check — are unit-testable without an HTTP-mocked crawl.

use std::collections::{HashMap, HashSet, VecDeque};

use super::crawler::path_identity;
use super::sitemap::{block_if_locale_alternate, SitemapDiscovery};

/// The crawl's frontier (cap bookkeeping), duplicate detection, the asset
/// map, and the outcome tally — everything the loop in `scrape_to_folder`
/// mutates across iterations.
pub(crate) struct CrawlState {
    pub(crate) frontier: Frontier,
    pub(crate) cap: CapBudget,
    pub(crate) dedupe: Dedupe,
    pub(crate) assets: AssetMap,
    pub(crate) tally: Tally,
}

impl CrawlState {
    /// Seeds the frontier with the start URL followed by every
    /// sitemap-declared URL — ahead of anything link discovery adds later,
    /// so a sitemap URL wins survivor choice over a link-discovered
    /// duplicate of the same page (see [`Dedupe::is_duplicate_identity`]) —
    /// and exempts every sitemap URL from the page cap (rule 3: a site's
    /// declared page list is always imported).
    pub(crate) fn new(start_url: &str, sitemap: &SitemapDiscovery) -> Self {
        let declared: HashSet<String> = sitemap.urls.iter().cloned().collect();
        Self {
            frontier: Frontier::seeded(start_url.to_string(), &sitemap.urls),
            cap: CapBudget::new(declared),
            dedupe: Dedupe::default(),
            assets: AssetMap::default(),
            tally: Tally::default(),
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

/// The five mutually-exclusive outcomes one popped URL can land in
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(tally.scraped(), 1);
        assert_eq!(tally.failed(), 2);
        assert_eq!(tally.skipped(), 1);
        assert_eq!(tally.duplicate(), 1);
        assert_eq!(tally.unreachable_variants(), 1);
    }
}
