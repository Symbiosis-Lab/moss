//! What will change on the live site if the user publishes right now.
//!
//! # Why this lives beside the manifest
//!
//! The manifest owns hash state — `hashes.json`, generation identity, the
//! `(sources, source_to_output)` pair. This module is *classification of* that
//! state, not a second copy of it, so it sits next to the thing it reads rather
//! than inside `deploy/`, which is a client of the manifest API like any other.
//!
//! # Why four verbs and not "changed"
//!
//! A site-wide restyle (a theme swap, one key in `config.toml`) moves the
//! rendered output of every page while the author changed nothing. Collapsed
//! into one "changed" count that reads as *"413 changed"* — indistinguishable
//! from the author having rewritten the site. Splitting authorship (Added,
//! Edited, Deleted) from re-rendering (Restyled) is the whole point:
//!
//! - **source hash moved** → the author touched the file → Edited
//! - **source hash stable, output hash moved** → moss re-rendered it → Restyled
//!
//! This is only answerable because page source hashes are in `SiteHashes.sources`
//! (see `PendingManifest::register_page_source_hash`). The obvious substitute —
//! `FacadeCache`'s per-page hash — folds twelve site-config keys into the hash,
//! so a one-key `config.toml` edit moves every page's facade and reports the
//! restyle as 413 edits. Facade is a correct *render-invalidation* signal and a
//! wrong *authorship* signal. `config_change_is_restyled_not_edited` guards this.
//!
//! # Degraded mode is a state, not a failure
//!
//! With no local record of the last publish — first publish, or the site was
//! published from another machine — there is nothing to diff against and
//! **no classification is attempted**. `classified: false`, every verb zero,
//! and the surfaces say so. Guessing verbs from a partial record is worse than
//! showing a flat count, because a confident wrong answer is not correctable by
//! the person reading it.
//!
//! Pure: no filesystem, no network, no clock. Persistence is
//! [`super::published_record`].

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::SealedManifest;

/// What was live on the site as of the last successful publish.
///
/// Written by `published_record::save` only once the publish is known to have
/// landed — a publish that did not land leaves no record, so the next change
/// set still offers to ship the work. "Landed" is per-target: the server's
/// commit returning 200 for moss hosting, the receiver's `/commit` returning
/// for an OnionPress publish.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Default, PartialEq)]
pub struct PublishedSnapshot {
    pub generation_id: String,
    /// Where this went live, as `<method>:<id>` — `moss:<site_id>`,
    /// `onionpress:<onion host>`. A label for diagnostics: nothing reads it,
    /// and the diff is target-blind. Named `site_id` on disk until 2026-08-08,
    /// when the plugin hosts started writing records too and "site id" stopped
    /// being true for half of them; the alias keeps older records loadable.
    #[serde(alias = "site_id")]
    pub target: String,
    /// RFC3339.
    pub published_at: String,
    /// Source path → source-byte hash. The page half of `SiteHashes.sources`.
    pub sources: HashMap<String, String>,
    /// Source path → output path.
    pub source_to_output: HashMap<String, String>,
    /// Output path → mode-tagged hash, as sent to the server.
    pub files: HashMap<String, String>,
    /// Source path → the note ID live at that path, as of this publish.
    ///
    /// Not part of the change set — nothing in `classify` reads it. It is here
    /// because it is the same fact, captured at the same instant, by the same
    /// writer: joined with `source_to_output` it says which uid was serving at
    /// which URL, which is what rename detection diffs against and what keeps
    /// duplicate-uid resolution from rewriting a published note's identity
    /// (`manifest::live_baseline`). It lived in a second file — a byte copy of
    /// the whole article map, 4.8 MB on a real vault to carry ~7 KB — until
    /// the uid-remint fix landed.
    ///
    /// `#[serde(default)]`, so a record written before this field still loads.
    /// An empty map on a record that exists is NOT "nothing was published": the
    /// reader reports it as unreadable-for-this-purpose rather than guessing.
    ///
    /// Superseded as the baseline's SOURCE by `triples` below (the triples migration) —
    /// kept only as the fallback `live_baseline` reads for a record that
    /// predates `triples`, and as the input `manifest::live_baseline::migrate`
    /// backfills `triples` FROM. Nothing new should read `uids` directly; read
    /// `triples`, or call `manifest::live_baseline::load`.
    #[serde(default)]
    pub uids: HashMap<String, String>,
    /// `{uid, url, source_path}` triples — everything `manifest::live_baseline`
    /// needs, computed once by the writer from a single read of the article
    /// map, and never re-derived by joining `uids` with `source_to_output` at
    /// read time.
    ///
    /// `uids` and `source_to_output` come from different sources (the article
    /// map and the sealed manifest respectively) that can advance
    /// independently, so a `HashMap<source_path, uid>` plus a join is
    /// structurally able to go half-updated: one map moves, the other doesn't,
    /// and the join's `?` then drops the uid from the baseline ENTIRELY rather
    /// than just its URL — which is what let a duplicate-uid collision fall
    /// through to the date/birth-time heuristic and mint a fresh uid into a
    /// live article's frontmatter — the uid-remint bug, reopened one level up
    /// by the gap the half-updated-shape fix closed for the one writer that
    /// could produce it. A triple
    /// cannot be half-updated: it is written whole or not written at all.
    ///
    /// `Option`, not a bare `Vec` with `#[serde(default)]`, so "this record
    /// predates triples" (`None`) is representable and distinct from "a
    /// publish landed and mapped no pages" (`Some(vec![])`) — a bare `Vec`
    /// cannot tell those apart once both deserialize to the same value.
    #[serde(default)]
    pub triples: Option<Vec<LiveEntry>>,
}

/// One live page: the uid that owns it, where it is served, and which source
/// file produced it.
///
/// Persisted as [`PublishedSnapshot::triples`] since the triples migration; also the type
/// `manifest::live_baseline` returns a baseline's entries as, re-exported from
/// there as `live_baseline::LiveEntry`. One type for both roles because a
/// triple needs no join to go from "on disk" to "in memory" — unlike the
/// `uids` + `source_to_output` pair it replaces as the baseline's source.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct LiveEntry {
    pub uid: String,
    /// Pretty URL, exactly as the article map keys it (no leading slash).
    pub url: String,
    /// Vault-relative source path.
    pub source_path: String,
    /// The page's title as of this publish. `#[serde(default)]` for
    /// back-compat: a record written before this field existed deserializes
    /// it as an empty string rather than failing to load.
    #[serde(default)]
    pub title: String,
}

impl PublishedSnapshot {
    /// Does this record describe the target the site publishes to now?
    ///
    /// The diff itself is target-blind, which was safe while moss hosting was
    /// the only writer. It stops being safe with two: publish to OnionPress,
    /// switch the host to moss, and the onion record would say "nothing to
    /// publish" about a moss site that has zero files live — the under-report
    /// direction the author cannot correct from the UI.
    ///
    /// Compared whole, not by method: two moss-hosted sites published from one
    /// folder have different live trees for the same reason two hosts do.
    ///
    /// A record with no `<method>:` prefix predates the plugin paths writing
    /// records at all, so it can only have come from moss hosting — and its
    /// bare value is the `site_id` a `moss:` target carries.
    pub fn describes_target(&self, target: &str) -> bool {
        self.target == target
            // Pre-keyed-layout record: a bare site_id, queried by `moss:<id>`.
            || (!self.target.contains(':')
                && target.strip_prefix("moss:").is_some_and(|id| id == self.target))
            // Pre-slot plugin record (`<method>:<url>`), queried by the bare
            // method a slot now is. The URL half was the defect — a plugin's
            // URL is mutable state, content-addressed targets change it every
            // deploy — so a method match IS the target match. Keep one
            // release, like `legacy_path`.
            || (!target.contains(':')
                && self.target.split_once(':').is_some_and(|(method, _)| method == target))
    }

    /// Capture what a sealed manifest is about to ship.
    ///
    /// `sources` is narrowed to the page half because the asset half is
    /// build-cache state that says nothing about authorship and would double
    /// the record's size.
    ///
    /// Its keys are a SUBSET of `source_to_output`'s, not equal to them: a page
    /// that was mapped but never hashed (the carry-gap case at
    /// [`classify`]'s `(None, Some(_))` arm) is dropped here. So
    /// `source_to_output` — not `sources` — is the authoritative page set, and
    /// anything asking "which pages existed at the last publish?" must read
    /// that. Reading `sources` instead made deletions of unhashed pages
    /// invisible.
    ///
    /// The filter itself is [`page_source_hashes`] — factored out so
    /// `deploy::history`'s snapshot reads the same page set from the same
    /// manifest, and the two records can never drift apart.
    pub fn from_sealed(sealed: &SealedManifest, target: &str, published_at: String) -> Self {
        Self {
            generation_id: sealed.generation_id().to_string(),
            target: target.to_string(),
            published_at,
            sources: page_source_hashes(sealed),
            source_to_output: sealed.source_to_output().clone(),
            files: sealed.files().clone(),
            // Filled by the caller, which is the only place that has the
            // article map — see `deploy::landed`.
            uids: HashMap::new(),
            triples: None,
        }
    }
}

/// Source path → source-byte hash, for the page half of `sealed.sources()`
/// only. `source_to_output()` is the authoritative page set (see
/// [`PublishedSnapshot::from_sealed`]'s doc for why `sources()` alone is
/// wrong), so this joins its keys against `sources()` rather than filtering
/// `sources()` directly.
///
/// Shared by [`PublishedSnapshot::from_sealed`] and `deploy::history`'s
/// publish snapshot, so the deploy baseline and the history record are always
/// reading the same page set out of the same manifest.
pub(crate) fn page_source_hashes(sealed: &SealedManifest) -> HashMap<String, String> {
    sealed
        .source_to_output()
        .keys()
        .filter_map(|src| sealed.sources().get(src).map(|m| (src.clone(), m.hash.clone())))
        .collect()
}

/// What happened to one page since the last publish.
///
/// Deliberately not `Unchanged` — an unchanged page is absent from the change
/// set entirely, so a caller cannot accidentally count it.
///
/// Named `PageVerb`, not `Verb`, because `tasks::Verb` already exists and both
/// are `specta::Type`: specta exports by bare type name into one flat
/// namespace, so two `Verb`s emit two `export type Verb` lines and
/// `bindings.ts` stops compiling. The collision is invisible in Rust — it only
/// appears when the bindings are regenerated.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PageVerb {
    /// The author created this page since the last publish.
    Added,
    /// The author changed this page's source bytes.
    Edited,
    /// The author removed this page.
    Deleted,
    /// The author changed nothing; moss re-rendered the page anyway (theme,
    /// config, template, or a shared component moved).
    Restyled,
}

#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
pub struct ChangedPage {
    pub source_path: String,
    pub verb: PageVerb,
}

/// The resting answer to "what will publishing do?".
#[derive(Clone, Debug, Serialize, specta::Type, PartialEq, Default)]
pub struct ChangeSet {
    /// False when there is no local record of the last publish. The verb counts
    /// are then all zero and meaningless; `flat_upload`/`flat_remove` carry the
    /// server's own count instead, and the UI must say it is not classified.
    pub classified: bool,
    pub added: u32,
    pub edited: u32,
    pub deleted: u32,
    pub restyled: u32,
    /// Changed output files with no source mapping: OG cards, webp variants,
    /// video renditions, feeds, the sitemap. Counted, never enumerated, never
    /// part of the ring — they are consequences of page changes, not peers.
    ///
    /// Only *changed* ones. Counting every unmapped output would report
    /// "1,847 assets" on a publish that changes nothing.
    pub assets: u32,
    /// Outputs that existed at the last publish and are gone now, with no page
    /// behind them: a dropped image variant, a removed video rendition, an
    /// orphaned OG card. The publish will send a `remove` for each.
    ///
    /// Counted separately from `assets` because `assets` walks the CURRENT
    /// output set and so is structurally blind to anything that disappeared —
    /// without this, a publish whose only effect was removing files reported
    /// itself as empty.
    pub removed_assets: u32,
    /// Degraded mode only: the server's own `need` count.
    pub flat_upload: u32,
    /// Degraded mode only: the server's own `remove` count.
    pub flat_remove: u32,
    /// Per-page detail, for the progress panel's second disclosure level.
    /// Empty in degraded mode.
    pub pages: Vec<ChangedPage>,
}

impl ChangeSet {
    /// True when publishing would change nothing on the live site.
    ///
    /// Requires `classified`. In degraded mode every field is zero by
    /// construction, so without that guard this would assert "nothing will
    /// change" about precisely the state where moss knows nothing — the same
    /// confusion between an absence of data and a claim that the surfaces are
    /// built to avoid. An unclassified set is never empty; it is unknown.
    pub fn is_empty(&self) -> bool {
        self.classified
            && self.added == 0
            && self.edited == 0
            && self.deleted == 0
            && self.restyled == 0
            && self.assets == 0
            && self.removed_assets == 0
    }
}

/// Degraded construction from the server's own diff.
///
/// The server's `need`/`remove` is ground truth for what will transfer; it just
/// cannot say *why*. `classified: false` is what stops the surfaces inventing a
/// reason.
pub fn flat(need: usize, remove: usize) -> ChangeSet {
    ChangeSet {
        classified: false,
        flat_upload: need as u32,
        flat_remove: remove as u32,
        ..ChangeSet::default()
    }
}

/// Degraded construction from a reconstructed published FILE SET.
///
/// `previous` is output-path → mode-tagged entry, the same domain as
/// [`SealedManifest::files`]. The counts are exactly what the server's
/// `need`/`remove` would be for that pair — which is why this stays
/// `classified: false` even though it is computed locally and precisely:
/// knowing which files move says nothing about *why*, and the page-level
/// `sources`/`source_to_output` a verb needs are not in a file set.
///
/// Its one caller is [`super::backfill`], which reconstructs `previous` by
/// re-hashing the last deployed generation.
pub fn flat_against(previous: &HashMap<String, String>, current: &SealedManifest) -> ChangeSet {
    let cur = current.files();
    let need = cur.iter().filter(|(path, entry)| previous.get(*path) != Some(*entry)).count();
    let remove = previous.keys().filter(|path| !cur.contains_key(*path)).count();
    flat(need, remove)
}

/// The path→hash diff two flat maps can answer on their own: Added (in
/// `current`, absent from `previous`), Edited (present in both, hash
/// differs), Deleted (in `previous`, absent from `current`). Never produces
/// [`PageVerb::Restyled`] — that verb needs an output path and an
/// output-hash pair neither map carries, which is exactly why this function
/// takes two `HashMap<String, String>` rather than two `SealedManifest`s or
/// `PublishedSnapshot`s: it has no opinion about where a hash comes from.
///
/// Shared by [`classify`], which layers its own output-hash restyle pass and
/// its own `source_to_output`-based deletion loop on top (a hash-only diff
/// cannot see a page that was mapped without a hash, at either end — see
/// `classify`'s carry-gap handling), and by `deploy::history`'s page and
/// site comparisons, whose flat publish records have no such gap: every
/// entry always carries a real hash, so this function alone is the whole
/// answer there.
pub(crate) fn diff_hashes(
    previous: &HashMap<String, String>,
    current: &HashMap<String, String>,
) -> Vec<ChangedPage> {
    let mut pages: Vec<ChangedPage> = current
        .iter()
        .filter_map(|(path, hash)| match previous.get(path) {
            None => Some(ChangedPage { source_path: path.clone(), verb: PageVerb::Added }),
            Some(prev_hash) if prev_hash != hash => {
                Some(ChangedPage { source_path: path.clone(), verb: PageVerb::Edited })
            }
            Some(_) => None,
        })
        .collect();
    pages.extend(previous.keys().filter(|path| !current.contains_key(path.as_str())).map(|path| ChangedPage {
        source_path: path.clone(),
        verb: PageVerb::Deleted,
    }));
    pages.sort_by(|a, b| a.source_path.cmp(&b.source_path));
    pages
}

/// Diff a sealed manifest against the last publish.
///
/// Pure. `previous == None` ⇒ degraded (`classified: false`, zero verbs); see
/// the module docs on why that is not a fallback to guessing.
///
/// | Verb | Test |
/// |---|---|
/// | Added | in `cur.source_to_output`, not in `prev.sources` |
/// | Deleted | in `prev.sources`, not in `cur.source_to_output` |
/// | Edited | source hash differs |
/// | Restyled | source hash equal, mapped output hash differs |
/// | (none) | source hash equal, output hash equal |
///
/// Added/Edited come from [`diff_hashes`] over two plain maps built so a
/// carry-gap page (mapped this build with no source hash) never appears
/// "changed": its current-side value is forward-filled from `prev.sources`
/// when a prior hash exists, which makes the pair compare equal and defers
/// it — same as an ordinary unchanged page — to the restyle pass below.
/// Deleted stays classify's own loop over `source_to_output`, never
/// `diff_hashes`'s: a page mapped without a hash at the LAST publish is
/// absent from `prev.sources` too, so a hash-only diff cannot see it
/// disappear (`deletion_is_reported_even_when_the_page_was_never_hashed`).
pub fn classify(previous: Option<&PublishedSnapshot>, current: &SealedManifest) -> ChangeSet {
    let Some(prev) = previous else {
        return ChangeSet::default();
    };

    let cur_sources = current.sources();
    let cur_map = current.source_to_output();
    let cur_files = current.files();

    let mut set = ChangeSet { classified: true, ..ChangeSet::default() };

    let cur_hashes: HashMap<String, String> = cur_map
        .keys()
        .map(|src| {
            let hash = cur_sources
                .get(src)
                .map(|m| m.hash.clone())
                .unwrap_or_else(|| prev.sources.get(src).cloned().unwrap_or_default());
            (src.clone(), hash)
        })
        .collect();

    let mut handled: std::collections::HashSet<String> = std::collections::HashSet::new();
    for changed in diff_hashes(&prev.sources, &cur_hashes) {
        match changed.verb {
            PageVerb::Added => {
                set.added += 1;
                handled.insert(changed.source_path.clone());
                set.pages.push(changed);
            }
            PageVerb::Edited => {
                set.edited += 1;
                handled.insert(changed.source_path.clone());
                set.pages.push(changed);
            }
            // Deleted here would only be a page absent from `cur_map`
            // entirely — already covered, more completely, by the loop
            // below. Restyled is never produced by `diff_hashes`.
            PageVerb::Deleted | PageVerb::Restyled => {}
        }
    }

    // Everything not already Added/Edited: unchanged hash (real or
    // forward-filled carry-gap) can still be a Restyle if the render moved.
    // Pages whose source hash never made it into this manifest are reported
    // once, in aggregate — never as a verb.
    let mut carry_gaps = 0u32;
    for (src, out) in cur_map {
        if handled.contains(src) {
            continue;
        }
        if !cur_sources.contains_key(src) && prev.sources.contains_key(src) {
            carry_gaps += 1;
        }
        if output_moved(prev, cur_files, src, out) {
            set.restyled += 1;
            set.pages.push(ChangedPage { source_path: src.clone(), verb: PageVerb::Restyled });
        }
    }

    // Walk `source_to_output`, NOT `sources`: the latter drops pages that were
    // mapped without a hash (see `from_sealed`), and a page dropped here is a
    // deletion that never gets reported. The publish would remove the page from
    // the live site while the change set said nothing was being deleted.
    for src in prev.source_to_output.keys() {
        if !cur_map.contains_key(src) {
            set.deleted += 1;
            set.pages.push(ChangedPage { source_path: src.clone(), verb: PageVerb::Deleted });
        }
    }

    if carry_gaps > 0 {
        log::warn!(
            "publish change set: {carry_gaps} page(s) have an output mapping but no source \
             hash, so they were classified by output alone. A register_source_mapping call \
             is missing its paired source-hash registration."
        );
    }

    set.assets = changed_unmapped_outputs(prev, cur_files, cur_map);
    set.removed_assets = removed_unmapped_outputs(prev, cur_files);
    set.pages.sort_by(|a, b| a.source_path.cmp(&b.source_path));
    set
}

/// Did the output this page renders to change since the last publish?
///
/// Reads the *previous* output path for the page, not the current one, so a
/// page that moved (slug override, retitle) compares like for like instead of
/// looking unchanged against an empty slot.
fn output_moved(
    prev: &PublishedSnapshot,
    cur_files: &HashMap<String, String>,
    src: &str,
    cur_out: &str,
) -> bool {
    let prev_out = prev.source_to_output.get(src).map(String::as_str).unwrap_or(cur_out);
    if prev_out != cur_out {
        return true;
    }
    prev.files.get(prev_out) != cur_files.get(cur_out)
}

/// Outputs that were live at the last publish and are absent now.
///
/// Pages are excluded — a vanished page is a `Deleted` verb, and its output
/// disappearing is the same event counted once. What remains is the asset
/// half: variants, renditions, cards. `changed_unmapped_outputs` walks the
/// CURRENT files and therefore cannot see any of this.
fn removed_unmapped_outputs(prev: &PublishedSnapshot, cur_files: &HashMap<String, String>) -> u32 {
    let prev_pages: std::collections::HashSet<&str> =
        prev.source_to_output.values().map(String::as_str).collect();
    prev.files
        .keys()
        .filter(|out| !prev_pages.contains(out.as_str()) && !cur_files.contains_key(*out))
        .count() as u32
}

/// Outputs with no page behind them — variants, feeds, cards — counted only
/// when they actually changed.
fn changed_unmapped_outputs(
    prev: &PublishedSnapshot,
    cur_files: &HashMap<String, String>,
    cur_map: &HashMap<String, String>,
) -> u32 {
    let mapped: std::collections::HashSet<&str> = cur_map.values().map(String::as_str).collect();
    cur_files
        .iter()
        .filter(|(out, hash)| !mapped.contains(out.as_str()) && prev.files.get(*out) != Some(*hash))
        .count() as u32
}

#[cfg(test)]
#[path = "change_set_tests.rs"]
mod tests;
