//! Footer file collection: `footer.md` reserved filename + `slot:` frontmatter opt-in.
//!
//! Convention: `footer.md` at site root is detected during scan, rendered
//! through the standard markdown pipeline, and emitted into the
//! `footer-left` slot. It is excluded from page generation (like `_*.md`).
//!
//! Frontmatter-marked files (`slot: footer-left`) are the escape hatch —
//! useful when the author wants a more meaningful filename or wants the
//! file to also generate a page.
//!
//! Conflict resolution: explicit frontmatter wins. If both `footer.md`
//! exists AND another file declares `slot: footer-left`, the
//! frontmatter-marked file takes effect; `footer.md` is skipped (with a
//! build warning).

use crate::build::slots::Slot;
use moss_core::ast::{Block, BlockMeta, Document, Shortcode};
use std::collections::HashMap;
use std::sync::Mutex;

/// Footer slot HTML resolved per language tree.
///
/// `default` applies to every page NOT under a recognized language-tree folder
/// (the root / site-default language) and to languages absent from `by_lang`.
/// `by_lang` maps a language-tree folder name (e.g. `"zh-hans"`) to that
/// language's footer HTML; pages under that folder use it. Injection-time
/// selection lives in `enhance::ResolvedSlots::get_html` via the page path's
/// `lang_tree_prefix`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FooterByLanguage {
    pub default: Option<String>,
    pub by_lang: HashMap<String, String>,
}

/// Which source kind populated a slot in a given bucket, so a later entry in
/// the same pass can decide whether to overwrite or skip.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SlotProvenance {
    /// Filled by a reserved-name page (`footer.md`). Lower precedence —
    /// frontmatter-marked pages may overwrite.
    ReservedName,
    /// Filled by a `slot:` frontmatter declaration. Higher precedence —
    /// reserved-name pages cannot overwrite.
    Frontmatter,
}

/// Determine the slot identifier (if any) for a path *relative to its language
/// tree* based on the reserved-name convention.
///
/// Returns `Some(Slot::Footer)` only for a `footer.md` at the tree root. The
/// caller strips any language-tree prefix first (see `language_tree_split`), so
/// both `footer.md` and `<lang>/footer.md` are recognized, while a deeper
/// `docs/footer.md` is not.
pub fn slot_from_filename(rel_path: &str) -> Option<Slot> {
    if rel_path == "footer.md" {
        Some(Slot::Footer)
    } else {
        None
    }
}

/// Split a source-relative path into its language-tree bucket and the path
/// within that tree. A leading recognized language folder (e.g. `zh-hans/`)
/// becomes the bucket (lowercased); everything else falls in the default
/// bucket (`None`).
///
/// - `"footer.md"` → `(None, "footer.md")`
/// - `"zh-hans/footer.md"` → `(Some("zh-hans"), "footer.md")`
/// - `"zh-hans/docs/footer.md"` → `(Some("zh-hans"), "docs/footer.md")`
pub fn language_tree_split(rel_path: &str) -> (Option<String>, &str) {
    match moss_core::home::lang_tree_prefix(rel_path) {
        // `lang_tree_prefix` returns the first path segment, so stripping it
        // plus its `/` leaves the rest of the path.
        Some(prefix) => {
            let rest = rel_path
                .strip_prefix(prefix)
                .and_then(|r| r.strip_prefix('/'))
                .unwrap_or(rel_path);
            (Some(prefix.to_lowercase()), rest)
        }
        None => (None, rel_path),
    }
}

/// Whether a markdown file at this relative path is reserved for slot use
/// (e.g. `footer.md`) and must NOT generate a page.
///
/// The predicate is filename-equality at the (language-)tree root, not prefix —
/// `footer-blog.md` is a normal page, and so is a deeper `docs/footer.md`. A
/// per-language `zh-hans/footer.md` IS excluded (it is that language's footer
/// slot, not a page).
///
/// The file is still parsed through the standard markdown pipeline so its
/// rendered HTML can be collected by `collect_footer_slots_by_language`; this
/// predicate is consulted at page-emission time only.
pub fn is_excluded_from_pages(rel_path: &str) -> bool {
    let (_, within_tree) = language_tree_split(rel_path);
    slot_from_filename(within_tree).is_some() || within_tree == WELCOME_EMAIL_FILE
}

/// Reserved filename of the welcome email at a language-tree root. It is
/// slot-only like `footer.md` but fills no slot: it feeds
/// `emit::welcome_email` instead, so `slot_from_filename` does not know it.
pub const WELCOME_EMAIL_FILE: &str = "welcome-email.md";

/// What the frontmatter says about whether a document ships, as the caller knows
/// it. Callers fill the fields they have and leave the rest at the default, so a
/// new reason to withhold a page adds a field here and a branch in
/// [`is_slot_only`] without touching callers that do not know it.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShipFacts {
    /// `partial: true`: a transclusion source, never a page.
    pub partial: bool,
}

/// Decides whether a document is slot-only: parsed and resolvable, but never
/// rendered, listed, given an address or counted as a page. Drafts are not
/// slot-only: they render for the preview and are withheld from publishing
/// through `PageMeta.draft` (the manifest's publish view). Today that is a reserved slot file (`footer.md`) or a
/// `partial: true` source. Both producers of `ParsedDocument::slot_only` (the
/// full parse and the page-map pass) call this, and every other check reads the
/// flag they set, so a further reason is added here and nowhere else.
pub fn is_slot_only(rel_path: &str, facts: ShipFacts) -> bool {
    is_excluded_from_pages(rel_path) || facts.partial
}

/// Whether this document's rendered HTML lands in a slot — by reserved name
/// (`footer.md`) or by `slot:` frontmatter.
///
/// Slot HTML is rendered once and injected byte-identically into every page at
/// every depth, so links inside it cannot be relative to the slot file's own
/// position. Callers that emit URLs consult this to switch to root-absolute.
/// A `slot:`-marked file that also generates a page gets absolute links on that
/// page too, which is equally correct there.
pub fn targets_a_slot(rel_path: &str, slot_frontmatter: Option<&str>) -> bool {
    is_excluded_from_pages(rel_path)
        || slot_frontmatter
            .and_then(Slot::from_str)
            .is_some_and(|s| s.is_authorable())
}

/// Resolve footer slots from a set of parsed pages, bucketed by the source
/// file's language tree.
///
/// `pages` should already include every parsed `.md` file (both
/// would-be-pages and slot-only files; the page generator gates emission
/// separately, but slot collection looks at every parsed doc).
///
/// Returns a map from slot name (e.g. `"footer-left"`) to its
/// [`FooterByLanguage`] resolution. Both the root `footer.md` (→ `default`)
/// and per-language `<lang>/footer.md` (→ `by_lang[<lang>]`) reserved files
/// are recognized, plus any page with `slot: footer-left` frontmatter
/// (bucketed by that page's source-path language tree).
///
/// Conflict resolution within a bucket: explicit frontmatter wins over the
/// reserved name. Single-pass; precedence per `(bucket, slot)` is tracked via
/// [`SlotProvenance`] so a late reserved-name page cannot overwrite an earlier
/// frontmatter-marked slot.
pub fn collect_footer_slots_by_language(
    pages: &[crate::build::types::ParsedDocument],
) -> HashMap<String, FooterByLanguage> {
    let mut out: HashMap<String, FooterByLanguage> = HashMap::new();
    // Keyed by (language bucket, slot); the default bucket uses "".
    let mut provenance: HashMap<(String, String), SlotProvenance> = HashMap::new();

    for page in pages {
        let path = page.source_path.as_deref().unwrap_or("");

        // Bucket by SOURCE path (deterministic, unlike content detection).
        // `within_tree` strips any language prefix so reserved-name matching
        // treats `<lang>/footer.md` exactly like a root `footer.md`.
        let (lang_bucket, within_tree) = language_tree_split(path);
        let reserved_slot = slot_from_filename(within_tree);

        // Frontmatter detection (higher precedence). `Slot::from_str` only
        // accepts the canonical names; the `is_authorable()` filter rejects
        // HeadEnd / FooterEnd which authors must not target.
        let frontmatter_slot = page.slot.as_deref().and_then(|slot_str| {
            match Slot::from_str(slot_str).filter(|s| s.is_authorable()) {
                Some(s) => Some(s),
                None => {
                    log::warn!(
                        "Page '{}' has slot: \"{}\" but it's not a recognized author-targetable slot. Recognized: footer-left.",
                        path,
                        slot_str
                    );
                    None
                }
            }
        });

        // Frontmatter always wins; this also covers a reserved-name page that
        // ALSO carries `slot:` frontmatter (overlap → frontmatter precedence).
        if let Some(slot) = frontmatter_slot {
            apply_footer_slot(
                &mut out,
                &mut provenance,
                &lang_bucket,
                slot.as_str(),
                &page.html_content,
                SlotProvenance::Frontmatter,
                path,
            );
        } else if let Some(slot) = reserved_slot {
            apply_footer_slot(
                &mut out,
                &mut provenance,
                &lang_bucket,
                slot.as_str(),
                &page.html_content,
                SlotProvenance::ReservedName,
                path,
            );
        }
    }

    out
}

/// Insert one footer source into the per-language slot map, honoring
/// frontmatter-over-reserved precedence within the `(bucket, slot)` pair.
fn apply_footer_slot(
    out: &mut HashMap<String, FooterByLanguage>,
    provenance: &mut HashMap<(String, String), SlotProvenance>,
    lang_bucket: &Option<String>,
    slot: &str,
    html: &str,
    source: SlotProvenance,
    source_path: &str,
) {
    let prov_key = (lang_bucket.clone().unwrap_or_default(), slot.to_string());

    // Reserved-name must not overwrite a frontmatter-claimed slot in the same
    // bucket (handles the out-of-order case: frontmatter page seen first).
    if source == SlotProvenance::ReservedName
        && provenance.get(&prov_key) == Some(&SlotProvenance::Frontmatter)
    {
        return;
    }

    // Strip preview-only `data-source-*` annotations. The footer is rendered
    // from footer.md but injected as chrome into every OTHER page, where its
    // `data-source-line` values (footer.md's own lines) collide with the host
    // page's — the editor's scroll-sync would resolve e.g. line 1 to the footer
    // <p> at the page bottom and jump the preview there. The footer is never a
    // scroll-sync target, so it needs no source annotations.
    let stripped = crate::build::ship::strip_source_annotations(html);
    let html: &str = stripped.as_ref();

    let footer = out.entry(slot.to_string()).or_default();
    let existing = match lang_bucket {
        Some(l) => footer.by_lang.get(l),
        None => footer.default.as_ref(),
    };
    if source == SlotProvenance::Frontmatter {
        if let Some(existing) = existing {
            if existing != html {
                log::warn!(
                    "Slot '{}' has multiple sources in language bucket '{}'; using frontmatter-marked file '{}'. Remove the duplicate to silence this warning.",
                    slot,
                    prov_key.0,
                    source_path,
                );
            }
        }
    }
    match lang_bucket {
        Some(l) => {
            footer.by_lang.insert(l.clone(), html.to_string());
        }
        None => footer.default = Some(html.to_string()),
    }
    provenance.insert(prov_key, source);
}

/// Whether `<folder_path>/footer.md` exists. Cheap predicate used to gate
/// `generate_native_slots` so a footer-only site (no analytics/comments/email)
/// still triggers slot generation.
pub fn project_has_footer_file(folder_path: &str) -> bool {
    std::path::Path::new(folder_path).join("footer.md").is_file()
}

/// Last-known-good footer/slot content, per folder, carried forward across
/// builds.
///
/// `config.toml` and page sources already have somewhere to fall back to when
/// this build cannot read them — `config.toml` defaults, a page carries
/// forward its last published output. A slot-only source (`footer.md`, or any
/// file opted in via `slot:` frontmatter) had nothing: `footer_by_lang` is
/// built fresh from THIS build's `pages` alone, so a `footer.md` still
/// downloading from the cloud simply is not in that slice, and the chrome it
/// used to fill vanishes from every page on the site — not just the one that
/// could not be read. This is the cache that closes that gap.
///
/// Process-global and keyed by folder path, matching `BuildRecords` and
/// `cloud_ledger`: this describes a folder across its builds, not a window,
/// so a headless build (no `AppState`) must see the same record a windowed
/// one would.
static FOOTER_CACHE: Mutex<Option<HashMap<String, HashMap<String, FooterByLanguage>>>> =
    Mutex::new(None);

fn with_cache<R>(
    f: impl FnOnce(&mut HashMap<String, HashMap<String, FooterByLanguage>>) -> R,
) -> R {
    let mut guard = FOOTER_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

fn cache_key(folder_path: &str) -> String {
    crate::vault_root::resolve_input(folder_path).to_string_lossy().into_owned()
}

/// Fill any `(slot, language bucket)` left empty by `fresh` with this
/// folder's last-known-good content for that same bucket, and record
/// whatever `fresh` DID supply as the new last-known-good.
///
/// Deliberately does not ask *why* a bucket came up empty — a `footer.md`
/// still downloading from the cloud and a `footer.md` the author genuinely
/// deleted look identical from here (both are simply absent from `pages`).
/// The fallback favors showing real, if possibly outdated, content over a
/// site-wide blank chrome; `stale_sources`/`refuse_publish` (see
/// `crate::system::build_records` and `crate::deploy::refuse_publish`) is
/// the backstop that keeps a build built this way from silently publishing —
/// it reads the same cloud-eviction evidence `read_page_source` already
/// records for `footer.md` on the branch that lands it here.
///
/// A build that never had this folder's cache populated (fresh process, or a
/// folder whose footer has always resolved) leaves an absent bucket absent —
/// there is nothing to fall back to, and manufacturing chrome no build ever
/// produced would be the "fabricated" failure mode this exists to avoid.
///
/// Returns a description of every bucket that had to fall back
/// (`"<slot> (default)"` or `"<slot> (<lang>)"`), sorted, for logging.
pub fn apply_last_known_good_fallback(
    folder_path: &str,
    fresh: &mut HashMap<String, FooterByLanguage>,
) -> Vec<String> {
    let key = cache_key(folder_path);
    with_cache(|cache| {
        let cached_for_folder = cache.entry(key).or_default();
        let mut stale = Vec::new();

        let mut slot_names: Vec<String> =
            cached_for_folder.keys().chain(fresh.keys()).cloned().collect();
        slot_names.sort();
        slot_names.dedup();

        for slot_name in &slot_names {
            // Snapshot what THIS build actually produced, before the fallback
            // loop below fills gaps in `fresh` — only genuine output may
            // overwrite the cache; a value we just copied FROM the cache
            // must not be copied back into it as if it were fresh.
            let produced_default = fresh.get(slot_name).and_then(|f| f.default.clone());
            let produced_langs: HashMap<String, String> = fresh
                .get(slot_name)
                .map(|f| f.by_lang.clone())
                .unwrap_or_default();

            if let Some(cached_footer) = cached_for_folder.get(slot_name) {
                let entry = fresh.entry(slot_name.clone()).or_default();
                if entry.default.is_none() {
                    if let Some(html) = &cached_footer.default {
                        entry.default = Some(html.clone());
                        stale.push(format!("{slot_name} (default)"));
                    }
                }
                for (lang, html) in &cached_footer.by_lang {
                    if !entry.by_lang.contains_key(lang) {
                        entry.by_lang.insert(lang.clone(), html.clone());
                        stale.push(format!("{slot_name} ({lang})"));
                    }
                }
            }

            let cache_entry = cached_for_folder.entry(slot_name.clone()).or_default();
            if let Some(html) = produced_default {
                cache_entry.default = Some(html);
            }
            for (lang, html) in produced_langs {
                cache_entry.by_lang.insert(lang, html);
            }
        }

        stale.sort();
        stale
    })
}

// `render_footer_pages_from_disk` was removed in PR7b.
//
// It was a temporary stand-in that read `<folder>/footer.md` on demand,
// synthesized a `title: ""` line into the frontmatter to suppress the
// auto-injected article H1, and ran the file through the normal markdown
// pipeline so `collect_footer_slots_by_language` had a `ParsedDocument` to consume.
//
// After PR7b, `footer.md` flows through the same parse pass every other
// page uses (it's not retain-filtered out of `documents` anymore). Two
// structural flags drive the right behavior end to end:
//
// - `ParsedDocument.slot_only` (set by `is_excluded_from_pages`) tells
//   the page-emission loop to skip the file, and tells the heading rule
//   (`moss_core::heading::HeadingInputs.slot_only`) to suppress the
//   auto-injected H1 — no frontmatter rewriting required.
// - `pipeline::run` returns `Vec<ParsedDocument>` (PR7b signature change)
//   so the caller in `build.rs` hands the typed slice to
//   `generate_native_slots`, where `collect_footer_slots_by_language` walks it.

// Footer columns: each heading in `footer.md` starts a section.
//
// The footer is the rendered `footer.md`, injected as chrome into every page.
// A heading and the content under it, up to the next heading of the same or a
// higher level, are wrapped in a `<section>`; content before the first
// heading is a section of its own. A footer with no heading is left as it is,
// so only a footer that asks for columns changes shape. The default
// stylesheet lays sibling sections out as columns.
//
// Only headings written directly in the footer split it: one inside a list,
// a quote or a `::: {.x}` block stays with its parent. The split is made on
// the document's top-level blocks, before rendering, so raw HTML in the
// footer cannot confuse it.
//
// The footer's headings also carry no id and no permalink (the pipeline
// clears them with `moss_core::ast::clear_heading_ids`). The footer sits beside
// the page's own headings on every page, so `## Contact` in both would give two
// elements one id, and a `#` glyph in the footer links to nothing a reader
// wants. An id the author wrote into a raw `<h2 id="…">` is untouched.

/// Wrap `doc`'s top-level blocks in `<section>`s at its headings, and the
/// blocks after its first thematic break in a `<div class="footer-bottom">`,
/// by inserting raw-HTML blocks around each run. The break itself is dropped.
/// A footer with neither headings nor a break is left as it is; one with a
/// break but no headings gets its top part as a single section.
pub(crate) fn section_footer_blocks(doc: &mut Document) {
    // A closing hero stays a direct child of the footer, after its columns.
    let closing = ends_in_hero(doc).then(|| (doc.blocks.pop(), doc.block_meta.pop()));
    wrap_footer_sections(doc);
    if let Some((Some(mut block), Some(meta))) = closing {
        mark_closing_hero(&mut block);
        doc.blocks.push(block);
        doc.block_meta.push(meta);
    }
}

/// Whether the document's last block is a `:::hero`.
pub(crate) fn ends_in_hero(doc: &Document) -> bool {
    matches!(doc.blocks.last(), Some(Block::Shortcode(Shortcode::Hero(_))))
}

/// Make a footer's closing hero the page's closing picture: the class
/// `moss-hero-closing` is what the stylesheet lays out edge to edge, and
/// `mobile=overlay` keeps the words on the picture on a phone, where a hero
/// otherwise stacks them below it.
fn mark_closing_hero(block: &mut Block) {
    if let Block::Shortcode(Shortcode::Hero(hero)) = block {
        if !hero.classes.is_empty() {
            hero.classes.push(' ');
        }
        hero.classes.push_str("moss-hero-closing");
        hero.mobile = Some("overlay".to_string());
    }
}

fn wrap_footer_sections(doc: &mut Document) {
    let brk = doc.blocks.iter().position(|b| matches!(b, Block::ThematicBreak));
    let top_end = brk.unwrap_or(doc.blocks.len());
    // A heading opens a section unless it is deeper than the one already open.
    let mut open_level = 0u8;
    let mut starts: Vec<usize> = Vec::new();
    for (i, block) in doc.blocks[..top_end].iter().enumerate() {
        if let Block::Heading { level, .. } = block {
            if starts.is_empty() || *level <= open_level {
                starts.push(i);
                open_level = *level;
            }
        }
    }
    if starts.is_empty() && brk.is_none() {
        return;
    }
    let total = doc.blocks.len();
    let mut cuts = vec![0];
    cuts.extend(starts);
    cuts.push(top_end);
    let mut pending = std::mem::take(&mut doc.blocks).into_iter().zip(std::mem::take(&mut doc.block_meta));
    let mut wrapped = Vec::with_capacity(total + 2 * cuts.len() + 2);
    for pair in cuts.windows(2) {
        push_run(&mut wrapped, &mut pending, "<section>\n", "</section>\n", pair[1] - pair[0]);
    }
    if brk.is_some() {
        pending.next(); // the break
        push_run(&mut wrapped, &mut pending, "<div class=\"footer-bottom\">\n", "</div>\n", total - top_end - 1);
    }
    (doc.blocks, doc.block_meta) = wrapped.into_iter().unzip();
}

/// Move the next `len` blocks from `pending` into `out`, between two raw-HTML
/// blocks; nothing when `len` is 0.
fn push_run(
    out: &mut Vec<(Block, BlockMeta)>,
    pending: &mut impl Iterator<Item = (Block, BlockMeta)>,
    open: &str,
    close: &str,
    len: usize,
) {
    if len == 0 {
        return;
    }
    out.push((Block::Other(open.to_string()), BlockMeta::default()));
    out.extend(pending.take(len));
    out.push((Block::Other(close.to_string()), BlockMeta::default()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::types::ParsedDocument;

    fn make_page(source: &str, html: &str, lang: crate::i18n::Language) -> ParsedDocument {
        ParsedDocument {
            source_path: Some(source.to_string()),
            html_content: html.to_string(),
            lang,
            ..Default::default()
        }
    }

    /// A fresh cache key per test — `FOOTER_CACHE` is process-global and
    /// `cargo test` runs these concurrently, so two tests sharing a folder
    /// path would read each other's fallback content.
    fn unique_folder() -> String {
        format!("/tmp/moss-footer-fallback-test-{}", uuid::Uuid::new_v4())
    }

    // Note: `Slot` enum round-trip is tested in `crate::build::slots::tests`.
    // The tests here exercise the footer-specific logic only.

    #[test]
    fn test_slot_from_filename_root_footer() {
        assert_eq!(slot_from_filename("footer.md"), Some(Slot::Footer));
    }

    #[test]
    fn test_slot_from_filename_ignores_subdir_paths() {
        assert_eq!(slot_from_filename("zh-hans/footer.md"), None);
        assert_eq!(slot_from_filename("docs/footer.md"), None);
    }

    #[test]
    fn test_slot_from_filename_ignores_unrelated_files() {
        assert_eq!(slot_from_filename("index.md"), None);
        assert_eq!(slot_from_filename("about.md"), None);
        assert_eq!(slot_from_filename("footer-left.md"), None);
        assert_eq!(slot_from_filename("footer-right.md"), None);
    }

    #[test]
    fn test_footer_md_excluded_from_pages() {
        assert!(is_excluded_from_pages("footer.md"));
    }

    #[test]
    fn test_footer_md_in_subdir_not_excluded() {
        // A non-language subdir is a normal folder — its footer.md is a page.
        assert!(!is_excluded_from_pages("docs/footer.md"));
        // Sibling-name false-positives must not trigger exclusion.
        assert!(!is_excluded_from_pages("footer-blog.md"));
        assert!(!is_excluded_from_pages("about.md"));
    }

    #[test]
    fn test_language_tree_footer_md_excluded() {
        // A `footer.md` at the root of a recognized language tree IS a footer
        // slot for that language, so it must be excluded from page emission
        // (otherwise it leaks out as a standalone `zh-hans/footer/` page).
        assert!(is_excluded_from_pages("zh-hans/footer.md"));
        assert!(is_excluded_from_pages("zh-hant/footer.md"));
        // ...but only at the tree root, not deeper inside it.
        assert!(!is_excluded_from_pages("zh-hans/docs/footer.md"));
    }

    #[test]
    fn test_by_language_root_footer_is_default() {
        use crate::i18n::Language;
        let pages = vec![
            make_page("footer.md", "<p>EN</p>", Language::En),
            make_page("index.md", "<p>home</p>", Language::En),
        ];
        let slots = collect_footer_slots_by_language(&pages);
        let footer = slots.get("footer-left").expect("footer-left present");
        assert_eq!(footer.default.as_deref(), Some("<p>EN</p>"));
        assert!(footer.by_lang.is_empty());
    }

    #[test]
    fn test_footer_slot_strips_source_line_annotations() {
        // The footer is rendered from footer.md (with preview source-line
        // annotations) but injected into every OTHER page. Its data-source-line
        // values reference footer.md's lines; if they leak into the host page the
        // editor's scroll-sync resolves e.g. line 1 to the footer <p> at the page
        // bottom and jumps the preview there. The slot HTML must be annotation-free.
        use crate::i18n::Language;
        let pages = vec![make_page(
            "footer.md",
            r#"<p data-source-line="1"><a href="/privacy">Privacy</a> · 2026</p>"#,
            Language::En,
        )];
        let slots = collect_footer_slots_by_language(&pages);
        let html = slots
            .get("footer-left")
            .and_then(|f| f.default.as_deref())
            .expect("footer-left present");
        assert!(
            !html.contains("data-source-line"),
            "footer slot must not carry source annotations: {html}"
        );
        assert!(
            html.contains(r#"<a href="/privacy">"#),
            "footer content must be preserved after stripping: {html}"
        );
    }

    #[test]
    fn test_by_language_buckets_language_footers() {
        // The core fix: a per-language `<lang>/footer.md` is bucketed under that
        // language, while the root `footer.md` stays the default.
        use crate::i18n::Language;
        let pages = vec![
            make_page("footer.md", "<p>EN</p>", Language::En),
            make_page("zh-hans/footer.md", "<p>ZH</p>", Language::ZhHans),
            make_page("zh-hant/footer.md", "<p>ZHT</p>", Language::ZhHant),
        ];
        let slots = collect_footer_slots_by_language(&pages);
        let footer = slots.get("footer-left").expect("footer-left present");
        assert_eq!(footer.default.as_deref(), Some("<p>EN</p>"));
        assert_eq!(footer.by_lang.get("zh-hans").map(String::as_str), Some("<p>ZH</p>"));
        assert_eq!(footer.by_lang.get("zh-hant").map(String::as_str), Some("<p>ZHT</p>"));
    }

    #[test]
    fn test_by_language_language_only_no_root_default() {
        // A site with only a language footer (no root footer.md) leaves the
        // default bucket empty; the language bucket is still populated.
        use crate::i18n::Language;
        let pages = vec![make_page("zh-hans/footer.md", "<p>ZH</p>", Language::ZhHans)];
        let slots = collect_footer_slots_by_language(&pages);
        let footer = slots.get("footer-left").expect("footer-left present");
        assert_eq!(footer.default, None);
        assert_eq!(footer.by_lang.get("zh-hans").map(String::as_str), Some("<p>ZH</p>"));
    }

    #[test]
    fn test_by_language_frontmatter_overrides_reserved_per_bucket() {
        // Frontmatter precedence still holds, now applied within each bucket.
        use crate::i18n::Language;
        let mut overriding = make_page("zh-hans/attribution.md", "<p>explicit-zh</p>", Language::ZhHans);
        overriding.slot = Some("footer-left".to_string());
        let pages = vec![
            make_page("footer.md", "<p>EN</p>", Language::En),
            make_page("zh-hans/footer.md", "<p>convention-zh</p>", Language::ZhHans),
            overriding,
        ];
        let slots = collect_footer_slots_by_language(&pages);
        let footer = slots.get("footer-left").expect("footer-left present");
        assert_eq!(footer.default.as_deref(), Some("<p>EN</p>"));
        assert_eq!(footer.by_lang.get("zh-hans").map(String::as_str), Some("<p>explicit-zh</p>"));
    }

    #[test]
    fn test_by_language_frontmatter_wins_regardless_of_order() {
        // Per-bucket provenance: a late reserved-name page must not overwrite an
        // earlier frontmatter-marked slot in the same bucket.
        use crate::i18n::Language;
        let mut overriding = make_page("attribution.md", "<p>explicit</p>", Language::En);
        overriding.slot = Some("footer-left".to_string());
        let pages = vec![overriding, make_page("footer.md", "<p>convention</p>", Language::En)];
        let slots = collect_footer_slots_by_language(&pages);
        assert_eq!(
            slots.get("footer-left").and_then(|f| f.default.as_deref()),
            Some("<p>explicit</p>"),
        );
    }

    #[test]
    fn test_by_language_unknown_slot_not_emitted() {
        use crate::i18n::Language;
        let mut bad = make_page("note.md", "<p>misuse</p>", Language::En);
        bad.slot = Some("sidebar-bottom".to_string());
        let slots = collect_footer_slots_by_language(&[bad]);
        assert!(slots.is_empty(), "unknown slot must not emit");
    }

    #[test]
    fn test_by_language_non_authorable_slot_rejected() {
        // Slots recognized by `Slot::from_str` but whose `is_authorable()` is
        // false must be rejected when authors target them via frontmatter —
        // `head-end` is head-section script injection, and the rest are
        // reserved for moss-native widget injection. A future regression that
        // flips `is_authorable()` would silently expose an internal injection
        // point.
        //
        // Derived from `Slot::ALL`, not a literal list: `from_str` used to
        // recognize three names and now recognizes seven, so a hardcoded pair
        // would leave four newly-parseable slots unguarded.
        use crate::i18n::Language;
        let non_authorable_slots: Vec<&str> = crate::build::slots::Slot::ALL
            .into_iter()
            .filter(|slot| !slot.is_authorable())
            .map(|slot| slot.as_str())
            .collect();
        assert!(non_authorable_slots.len() >= 2, "the guard must cover something");
        for non_authorable in non_authorable_slots {
            let mut bad = make_page(
                &format!("attempt-{non_authorable}.md"),
                "<script>alert(1)</script>",
                Language::En,
            );
            bad.slot = Some(non_authorable.to_string());
            let slots = collect_footer_slots_by_language(&[bad]);
            assert!(slots.is_empty(), "slot:{non_authorable} must not be author-targetable");
        }
    }

    // PR7b: the `render_footer_pages_from_disk` tests were
    // deleted along with the function. The behavior they covered —
    // footer.md flows through the markdown pipeline, the auto-injected
    // article H1 is suppressed even when the author writes
    // `title: "Custom"` — is now exercised by:
    //
    // - `crates/moss-core/src/heading/state.rs` tests `slot_only_hides_heading_*`
    //   (the heading rule consults `HeadingInputs.slot_only`).
    // - `crates/moss-build/src/build/markdown/pipeline.rs` integration: every
    //   markdown file routed through `process_markdown_file` populates
    //   `ParsedDocument.slot_only` from `is_excluded_from_pages`, so the
    //   pipeline that produces the `documents` slice naturally handles
    //   footer.md correctly. Snapshot tests on real sites (including a
    //   bilingual one with a zh-hans/footer.md) guard end-to-end
    //   byte-equivalence with the pre-PR7b output.

    #[test]
    fn test_footer_md_via_pipeline_suppresses_h1() {
        // Direct exercise of the new path: a footer.md parsed via
        // `process_markdown_file` lands with `slot_only = true` and an
        // HTML body that does NOT contain the auto-injected article
        // title H1. This is the structural replacement for the deleted
        // `test_render_footer_pages_from_disk_renders_markdown`.
        let empty_map = HashMap::new();
        let doc = crate::build::markdown::process_markdown_file(
            "footer.md",
            "[link](https://example.com) text",
            "",
            &empty_map,
            false,
            crate::i18n::Language::En,
            None,
            crate::build::markdown::SiteMarkdown::default(),
            None,
            None,
            None,
            crate::build::markdown::PageContext::default(),
        )
        .expect("footer.md should parse through the standard pipeline");
        assert!(doc.slot_only, "footer.md must be flagged slot_only");
        assert!(
            doc.html_content.contains("<a"),
            "rendered HTML should still contain the anchor: {}",
            doc.html_content
        );
        assert!(
            !doc.html_content.contains("moss-article-title"),
            "slot_only must suppress the article-title H1: {}",
            doc.html_content
        );
    }

    #[test]
    fn test_footer_md_via_pipeline_suppresses_h1_even_with_authored_title() {
        // PR7b shift: an authored `title: "..."` on a slot file still
        // populates the chrome label / `doc.title` but does NOT bring
        // back the auto-injected article-level H1. The old
        // `render_footer_pages_from_disk` honored an authored title;
        // the new flow treats slot files as structural and reserves
        // article chrome for actual articles. The Custom string can
        // still surface via the frontmatter -> doc.title -> any chrome
        // template that reads it.
        let empty_map = HashMap::new();
        let doc = crate::build::markdown::process_markdown_file(
            "footer.md",
            "---\ntitle: Custom Footer Title\n---\n\n[link](https://example.com)",
            "",
            &empty_map,
            false,
            crate::i18n::Language::En,
            None,
            crate::build::markdown::SiteMarkdown::default(),
            None,
            None,
            None,
            crate::build::markdown::PageContext::default(),
        )
        .expect("footer.md should parse");
        assert!(doc.slot_only);
        assert_eq!(doc.title, "Custom Footer Title");
        assert!(
            !doc.html_content.contains("moss-article-title"),
            "slot_only suppresses article H1 regardless of title: {}",
            doc.html_content
        );
    }

    /// A heading in slot HTML renders without the permalink mark; the same
    /// heading in a page body keeps it.
    #[test]
    fn heading_in_footer_slot_has_no_permalink_mark() {
        let render = |path: &str, frontmatter: &str| {
            let empty_map = HashMap::new();
            crate::build::markdown::process_markdown_file(
                path,
                &format!("{frontmatter}## Contact\n\nWrite to us."),
                "",
                &empty_map,
                false,
                crate::i18n::Language::En,
                None,
                crate::build::markdown::SiteMarkdown::default(),
                None,
                None,
                None,
                crate::build::markdown::PageContext::default(),
            )
            .expect("parses")
            .html_content
        };
        assert!(!render("footer.md", "").contains("moss-heading-anchor"));
        assert!(!render("note.md", "---\nslot: footer-left\n---\n\n").contains("moss-heading-anchor"));
        assert!(render("note.md", "").contains("moss-heading-anchor"));
    }

    // -----------------------------------------------------------------
    // apply_last_known_good_fallback
    // -----------------------------------------------------------------

    #[test]
    fn fallback_fills_a_bucket_this_build_could_not_produce() {
        let folder = unique_folder();

        // Build 1: footer.md read fine.
        let mut fresh = HashMap::new();
        fresh.insert(
            "footer-left".to_string(),
            FooterByLanguage { default: Some("<p>real footer</p>".to_string()), by_lang: HashMap::new() },
        );
        let stale = apply_last_known_good_fallback(&folder, &mut fresh);
        assert!(stale.is_empty(), "nothing fell back on the first build: {stale:?}");
        assert_eq!(fresh["footer-left"].default.as_deref(), Some("<p>real footer</p>"));

        // Build 2: footer.md unreadable — this build's own pass produced nothing.
        let mut fresh = HashMap::new();
        let stale = apply_last_known_good_fallback(&folder, &mut fresh);
        assert_eq!(
            fresh.get("footer-left").and_then(|f| f.default.as_deref()),
            Some("<p>real footer</p>"),
            "the last real footer must survive a build that could not read footer.md"
        );
        assert_eq!(stale, vec!["footer-left (default)".to_string()]);
    }

    #[test]
    fn fallback_leaves_an_empty_bucket_empty_when_nothing_was_ever_cached() {
        // No prior build of this folder ever populated the cache — there is
        // nothing real to fall back to, so the bucket stays absent rather
        // than fabricating chrome no build ever produced.
        let folder = unique_folder();
        let mut fresh: HashMap<String, FooterByLanguage> = HashMap::new();
        let stale = apply_last_known_good_fallback(&folder, &mut fresh);
        assert!(stale.is_empty());
        assert!(fresh.is_empty());
    }

    #[test]
    fn fallback_updates_when_real_content_returns() {
        let folder = unique_folder();

        let mut fresh = HashMap::new();
        fresh.insert(
            "footer-left".to_string(),
            FooterByLanguage { default: Some("<p>v1</p>".to_string()), by_lang: HashMap::new() },
        );
        apply_last_known_good_fallback(&folder, &mut fresh);

        // A build with nothing falls back to v1.
        let mut fresh = HashMap::new();
        apply_last_known_good_fallback(&folder, &mut fresh);
        assert_eq!(fresh["footer-left"].default.as_deref(), Some("<p>v1</p>"));

        // footer.md is readable again, with new content — the cache must
        // move on to it, not stay pinned to v1 forever.
        let mut fresh = HashMap::new();
        fresh.insert(
            "footer-left".to_string(),
            FooterByLanguage { default: Some("<p>v2</p>".to_string()), by_lang: HashMap::new() },
        );
        let stale = apply_last_known_good_fallback(&folder, &mut fresh);
        assert!(stale.is_empty(), "v2 is real content, not a fallback: {stale:?}");

        let mut fresh = HashMap::new();
        apply_last_known_good_fallback(&folder, &mut fresh);
        assert_eq!(fresh["footer-left"].default.as_deref(), Some("<p>v2</p>"));
    }

    #[test]
    fn fallback_is_per_bucket_not_all_or_nothing() {
        // A per-language footer (`by_lang`) missing this build must not
        // disturb the default bucket, or a different language's fallback.
        let folder = unique_folder();

        let mut fresh = HashMap::new();
        fresh.insert(
            "footer-left".to_string(),
            FooterByLanguage {
                default: Some("<p>EN</p>".to_string()),
                by_lang: HashMap::from([("zh-hans".to_string(), "<p>ZH</p>".to_string())]),
            },
        );
        apply_last_known_good_fallback(&folder, &mut fresh);

        // This build's own pass produced the default and a DIFFERENT
        // language, but not zh-hans (as if zh-hans/footer.md alone were
        // still downloading).
        let mut fresh = HashMap::new();
        fresh.insert(
            "footer-left".to_string(),
            FooterByLanguage {
                default: Some("<p>EN v2</p>".to_string()),
                by_lang: HashMap::from([("zh-hant".to_string(), "<p>ZHT</p>".to_string())]),
            },
        );
        let stale = apply_last_known_good_fallback(&folder, &mut fresh);

        let footer = &fresh["footer-left"];
        assert_eq!(footer.default.as_deref(), Some("<p>EN v2</p>"), "fresh default must win, not fall back");
        assert_eq!(footer.by_lang.get("zh-hant").map(String::as_str), Some("<p>ZHT</p>"), "fresh zh-hant must win");
        assert_eq!(footer.by_lang.get("zh-hans").map(String::as_str), Some("<p>ZH</p>"), "zh-hans must fall back to its own last-known-good");
        assert_eq!(stale, vec!["footer-left (zh-hans)".to_string()]);
    }

    fn doc_of(markdown: &str) -> Document {
        moss_core::ast::parse(markdown)
    }

    fn sectioned_html(markdown: &str) -> String {
        let mut doc = doc_of(markdown);
        section_footer_blocks(&mut doc);
        moss_core::ast::render_document(&doc, &moss_core::ast::DefaultHooks::new())
    }

    #[test]
    fn a_footer_without_headings_is_unchanged() {
        let md = "Lead text\n\n- One\n";
        let plain = moss_core::ast::render_document(&doc_of(md), &moss_core::ast::DefaultHooks::new());
        assert_eq!(sectioned_html(md), plain);
    }

    #[test]
    fn each_heading_starts_a_section_and_the_lead_is_its_own() {
        let out = sectioned_html("Lead\n\n## Site\n\n- A\n\n## Contact\n\nMail\n");
        assert_eq!(out.matches("<section>").count(), 3, "{out}");
        assert!(out.starts_with("<section>\n<p>Lead</p>\n</section>\n<section>\n<h2"), "{out}");
        assert!(out.ends_with("<p>Mail</p>\n</section>\n"), "{out}");
    }

    #[test]
    fn a_deeper_heading_stays_in_its_section_and_a_higher_one_closes_it() {
        let out = sectioned_html("## A\n\n### A1\n\nx\n\n## B\n\n# C\n");
        assert_eq!(out.matches("<section>").count(), 3, "{out}");
        assert!(out.contains("</h3>\n<p>x</p>\n</section>"), "{out}");
    }

    #[test]
    fn a_heading_inside_another_block_does_not_split() {
        let out = sectioned_html("- ## Item\n\n> ## Quoted\n\nx\n");
        assert!(!out.contains("<section>"), "{out}");
    }

    #[test]
    fn a_thematic_break_ends_the_columns_and_starts_the_bottom_row() {
        let out = sectioned_html("## A\n\nx\n\n## B\n\ny\n\n---\n\nLegal text\n\nMore\n");
        assert_eq!(out.matches("<section>").count(), 2, "{out}");
        assert!(out.contains("</section>\n<div class=\"footer-bottom\">\n<p>Legal text</p>\n<p>More</p>\n</div>\n"), "{out}");
        assert!(!out.contains("<hr"), "{out}");
    }

    #[test]
    fn a_break_without_headings_makes_the_top_one_section() {
        let out = sectioned_html("Top\n\n---\n\nBottom\n");
        assert_eq!(out, "<section>\n<p>Top</p>\n</section>\n<div class=\"footer-bottom\">\n<p>Bottom</p>\n</div>\n");
    }

    #[test]
    fn a_trailing_break_leaves_no_empty_bottom_row() {
        assert!(!sectioned_html("## A\n\nx\n\n---\n").contains("footer-bottom"));
    }

    #[test]
    fn raw_html_does_not_change_where_sections_split() {
        let md = "## One\n\n<script>for(var i=0;i<3;i++){}</script>\n\n## Two\n\n<ul><li>a<li>b</ul>\n\n## Three\n";
        assert_eq!(sectioned_html(md).matches("<section>").count(), 3);
    }
}
