//! The listing group model — membership as a value, not N×H edges (ADR-044).
//!
//! A listing host reads each listed child's **raw body** at render time
//! (`resolve_page_description` falls back to an excerpt of `content`), and
//! folder membership is a URL prefix rather than a link, so `DepGraph` has no
//! edge for it. Lacking an edge type, `render/blocking.rs` used to substitute
//! "render every page that could possibly be a member's parent" — 114 of 214
//! pages on the reference vault, on every save, including saves that changed
//! no output byte (moss#968 Finding 2).
//!
//! The replacement is *change pruning on a derived projection*: a **listing
//! group** is the unit of dependency, and a host re-renders iff a group it
//! reads has a moved digest.
//!
//! Three rules, all from ADR-044, all load-bearing:
//!
//! 1. **Everything a host can observe about a child is a digest input** — the
//!    card fields *and* the resolved sort/style/group plan, which the card
//!    fields do not cover.
//! 2. **The projection is built by blanking a clone, never by listing fields
//!    in.** Same construction, and the same reason, as `facade.rs`: a field
//!    added to `ParsedDocument` lands in the projection by default, so the rot
//!    direction is "one extra full render", never "a stale card that renders
//!    perfectly". [`project_child`] is where that blank-out lives, and it is
//!    the one place a new listing read must be reflected.
//! 3. **Membership is computed by the renderer's own selector**
//!    ([`select_children_by_slug`]), never re-derived. Any rule the renderer
//!    honours the digest honours, structurally.

use std::collections::{BTreeMap, HashMap};

use moss_core::PageKind;
use serde::{Deserialize, Serialize};

use crate::build::facade::debug_hash;
use crate::build::folder_embed::{
    effective_group_for_axis, resolve_children_config, select_children_by_slug,
};
use crate::build::types::ParsedDocument;
use crate::types::content::ProjectStructure;

/// `children_depth`, reduced to the two values the selector branches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Depth {
    Direct,
    All,
}

impl Depth {
    fn from_frontmatter(value: Option<&str>, default_all: bool) -> Self {
        match value {
            Some("all") => Depth::All,
            Some(_) => Depth::Direct,
            None if default_all => Depth::All,
            None => Depth::Direct,
        }
    }

    fn is_flatten(self) -> bool {
        matches!(self, Depth::All)
    }
}

/// Everything `select_children_by_slug` branches on. Two hosts with the same
/// key read the same listing, so they share one digest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GroupKey {
    /// Slugified folder id; `""` for the site root.
    pub folder_slug: String,
    pub depth: Depth,
    pub scope_default_tree: bool,
    pub exclude_nav: bool,
}

impl GroupKey {
    /// Stable string form — the cache key. Deliberately not `Debug`: this
    /// value crosses builds inside `dep-cache.json`, so its shape is a format,
    /// not a rendering convenience.
    pub fn id(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.folder_slug,
            if self.depth.is_flatten() { "all" } else { "direct" },
            self.scope_default_tree as u8,
            self.exclude_nav as u8,
        )
    }
}

/// Three independently-diffable halves, so a log line can say *what* moved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupDigest {
    /// Ordered member url list — moves on add / delete / rename / reorder.
    pub membership: String,
    /// Fold of each member's projection hash.
    pub contents: String,
    /// The member-derived half of the resolved listing plan: inferred axis,
    /// auto-detected style, effective group.
    pub plan: String,
}

/// This build's digests, keyed by [`GroupKey::id`].
#[derive(Debug, Clone, Default)]
pub struct ListingGroups {
    digests: BTreeMap<String, GroupDigest>,
}

impl ListingGroups {
    pub fn digest(&self, key: &GroupKey) -> Option<&GroupDigest> {
        self.digests.get(&key.id())
    }

    pub fn len(&self) -> usize {
        self.digests.len()
    }

    /// The map to persist. Only digests cross builds — the group graph itself
    /// is rebuilt from scratch every build (≈120 groups × 214 docs of
    /// `starts_with` is sub-millisecond), so there is nothing to maintain
    /// incrementally.
    pub fn into_map(self) -> BTreeMap<String, GroupDigest> {
        self.digests
    }

    /// Compute every group any listing host reads.
    pub fn build(
        documents: &[ParsedDocument],
        project: &ProjectStructure,
        math: bool,
    ) -> Self {
        // One projection per document, not per (document, group) pair: the
        // projection is a function of the document and the corpus, never of
        // the group that lists it.
        let all_refs: Vec<&ParsedDocument> = documents.iter().collect();
        let projections: HashMap<&str, String> = documents
            .iter()
            .map(|doc| (doc.url_path.as_str(), project_child(doc, &all_refs, project, math)))
            .collect();

        let mut keys: BTreeMap<String, GroupKey> = BTreeMap::new();
        for doc in documents {
            if !hosts_listing(doc) {
                continue;
            }
            if let Some(read) = groups_read_by(doc, documents) {
                for key in read {
                    keys.entry(key.id()).or_insert(key);
                }
            }
        }

        let digests = keys
            .into_iter()
            .map(|(id, key)| {
                let members = select_children_by_slug(
                    &key.folder_slug,
                    key.depth.is_flatten(),
                    key.scope_default_tree,
                    key.exclude_nav,
                    documents,
                    project,
                );
                (id, digest_of(&members, &projections, math))
            })
            .collect();

        Self { digests }
    }
}

fn digest_of(
    members: &[&ParsedDocument],
    projections: &HashMap<&str, String>,
    math: bool,
) -> GroupDigest {
    let membership: Vec<&str> = members.iter().map(|d| d.url_path.as_str()).collect();
    let contents: Vec<Option<&String>> = members
        .iter()
        .map(|d| projections.get(d.url_path.as_str()))
        .collect();

    // The member-derived half of the listing plan (ADR-044 rule 1, second
    // bullet). The host's OWN overrides (`children_style`, `children_group`,
    // `sort:`) are fields of the host document, so they already move the
    // host's facade; what the host cannot see in its own fingerprint is how
    // its children's `weight:`/`date:`/richness resolve the plan for it.
    //
    // `resolve_children_config` reads `resolve_page_description(.., content)`
    // to decide "rich" — a body-derived input, and the reason the plan half
    // exists at all rather than falling out of the projection.
    let (style, group) = resolve_children_config(members, None, None, math);
    // A default folder declares no `sort:`, so `resolve_folder_sort` reduces
    // to `infer_axis` over the members — exactly the child-derived half.
    let axis = moss_core::sort::resolve_folder_sort(&ParsedDocument::default(), members).axis;
    let plan = (
        axis,
        style.value.clone(),
        effective_group_for_axis(&group, axis),
    );

    GroupDigest {
        membership: debug_hash(&membership),
        contents: debug_hash(&contents),
        plan: debug_hash(&plan),
    }
}

/// Everything a listing card can observe about one child.
///
/// **Blank-out on a clone** (ADR-044 rule 2). The five body fields are the
/// ones a card provably cannot read — a card renders no HTML of the child's
/// body, no links out of it and no transclusion — and each is replaced, where
/// the card *does* observe a derived form of it, by that resolved value.
/// Everything else on `ParsedDocument` stays in by default.
///
/// Every field exists to be *hashed*, never read — `debug_hash` fingerprints
/// the `Debug` rendering, the same convention `facade.rs` uses so a field that
/// stops implementing `Debug` fails to compile rather than silently leaving
/// the fingerprint.
#[allow(dead_code)]
#[derive(Debug)]
struct ChildProjection<'a> {
    /// The clone with the body blanked.
    stripped: ParsedDocument,
    /// `resolve_page_description` short-circuits on frontmatter
    /// `description:` and otherwise reads only the first paragraph, truncated
    /// — which is why a body edit below paragraph one prunes for free, and a
    /// child with a `description:` prunes on *every* body edit.
    resolved_excerpt: Option<String>,
    /// A folder card shows "N articles" and the folder's latest child date.
    /// Both are corpus-derived: they move when a file is added under the
    /// folder even though the folder's own document did not change. This is
    /// what makes the root home re-render when `writings/` gains a post.
    child_count: Option<usize>,
    folder_latest_date: Option<String>,
    /// `extract_date_from_doc` reaches past frontmatter into filename and
    /// filesystem fallbacks, so `date_raw` can be `Some` where `doc.date` is
    /// `None`.
    date_display: String,
    date_raw: Option<String>,
    date_explicit: bool,
    /// Kept as a borrow so the hash covers the identity the group orders by.
    url_path: &'a str,
}

fn project_child(
    doc: &ParsedDocument,
    all_refs: &[&ParsedDocument],
    project: &ProjectStructure,
    math: bool,
) -> String {
    let mut stripped = doc.clone();
    stripped.content = String::new();
    stripped.html_content = String::new();
    stripped.body_plan = None;
    stripped.outgoing_links = Vec::new();
    stripped.embed_deps = Vec::new();
    // moss#1041: a card never displays a CHILD's own `lang` — the only lang a
    // listing reads is the HOST's own (`folder_embed.rs`'s `let lang =
    // documents[i].lang`, for localized "N articles" strings), which already
    // moves the host's own facade directly. Left in, a translation flipping
    // (a page's `lang` re-decided, no group-membership change) moved this
    // digest and swept in every host of the child's listing group for a value
    // no card renders — narrower than a site-wide render, but still a false
    // positive of the exact same shape.
    stripped.lang = crate::i18n::Language::default();
    // Minted fresh on every parse for a page with no frontmatter block; left
    // in, it would move every digest on every build. Same normalization the
    // facade applies, and for the same reason (`facade::normalized`).
    stripped.uid = None;

    // Mirrors `child_list::props_for_document`: the card's count is always
    // recursive (subfolders included), independent of `children_depth`, so
    // the digest must move on the same trigger the card does — a page added
    // two levels down still has to re-render every host of this card.
    let article_count = (doc.kind == PageKind::Folder)
        .then(|| {
            crate::build::page::page::count_listable_articles(
                all_refs,
                doc.url_path.trim_end_matches("index.html"),
                &doc.url_path,
            )
        })
        .filter(|n| *n > 0);
    let folder_latest_date = article_count
        .and_then(|_| crate::build::folder_embed::folder_latest_date(doc, all_refs, &project.root_path));
    let (date_display, date_raw, date_explicit) =
        crate::build::components::extract_date_from_doc(doc, &project.root_path);

    debug_hash(&ChildProjection {
        stripped,
        resolved_excerpt: crate::build::page::meta::resolve_page_description(
            doc.description.as_deref(),
            &doc.content,
            math,
        ),
        child_count: article_count,
        folder_latest_date,
        date_display,
        date_raw,
        date_explicit,
        url_path: &doc.url_path,
    })
}

/// The demoted predicate. It used to answer "render it"; it now answers
/// "which groups does it read" (ADR-044).
///
/// NOT a `url_path.ends_with("/index.html")` test: pretty URLs give EVERY page
/// that shape, which would make every page a host.
pub fn hosts_listing(doc: &ParsedDocument) -> bool {
    doc.kind == PageKind::Folder
        || doc.url_path == "index.html"
        || doc.is_home_override
        || doc.children_source.is_some()
        || doc.sidebar.is_some()
        || doc.term_listing.is_some()
}

/// The groups a host reads, or `None` when its shape is not modelled.
///
/// `None` means **render**. Over-approximation is the only safe default —
/// rustc keeps `eval_always` for the same reason, and it is what all host
/// shapes did before moss#968.
pub fn groups_read_by(doc: &ParsedDocument, documents: &[ParsedDocument]) -> Option<Vec<GroupKey>> {
    let mut keys: Vec<GroupKey> = Vec::new();

    // (a) The root homepage. `synthesize_children_marker(.., is_homepage: true)`
    //     defaults depth to "all" and — in default mode only, i.e. with no
    //     `children_source` redirecting the listing — turns on both homepage
    //     filters.
    if doc.url_path == "index.html" {
        let default_mode = doc.children_source.is_none();
        let folder_slug = match doc.children_source.as_deref() {
            None => String::new(),
            Some(reference) => resolve_children_source_slug(reference, documents)?,
        };
        keys.push(GroupKey {
            folder_slug,
            depth: Depth::from_frontmatter(doc.children_depth.as_deref(), true),
            scope_default_tree: default_mode,
            exclude_nav: default_mode,
        });
    }

    // (b) A folder index page lists its OWN folder — `render/html.rs` passes
    //     the page's own url-derived folder path, ignoring `children_source`
    //     on this branch — and defaults depth to "direct".
    if doc.kind == PageKind::Folder
        && doc.url_path != "index.html"
        && doc.url_path.ends_with("/index.html")
    {
        keys.push(GroupKey {
            folder_slug: doc.url_path.trim_end_matches("/index.html").to_string(),
            depth: Depth::from_frontmatter(doc.children_depth.as_deref(), false),
            scope_default_tree: false,
            exclude_nav: false,
        });
    }

    // (c) The right rail. Its own membership rule is narrower than the
    //     selector's (physical children only, dated only), so the group is an
    //     over-approximation of what the sidebar reads — the safe direction.
    //     Both resolution paths are keyed, because `render/html.rs` resolves a
    //     sidebar reference by last-segment-or-clean_stem while a
    //     `children_source` resolves by folder-stem; a host that hits the
    //     other one must not be pruned by this one's answer.
    if let Some(reference) = sidebar_reference(doc) {
        keys.push(GroupKey {
            folder_slug: resolve_sidebar_slug(reference, documents)?,
            depth: Depth::Direct,
            scope_default_tree: false,
            exclude_nav: false,
        });
        keys.push(GroupKey {
            folder_slug: resolve_children_source_slug(reference, documents)?,
            depth: Depth::Direct,
            scope_default_tree: false,
            exclude_nav: false,
        });
    }

    // (d) A term-claim page hosts the claimed term's member listing
    //     (`render/html.rs` passes `term_listing` as the folder path, so the
    //     key IS the slug — no stem resolution). For a folder index that also
    //     claims a term, (b) stays as an over-approximation: the term listing
    //     replaces the own-folder one, but reading one group too many only
    //     costs a render, never staleness.
    if let Some(key) = doc.term_listing.as_deref() {
        keys.push(GroupKey {
            folder_slug: key.to_string(),
            depth: Depth::from_frontmatter(doc.children_depth.as_deref(), false),
            scope_default_tree: false,
            exclude_nav: false,
        });
    }

    // A host that matched the predicate but no arm above (e.g. a home-override
    // page whose url is not a folder index) is an unmodelled shape.
    if keys.is_empty() {
        None
    } else {
        keys.sort();
        keys.dedup();
        Some(keys)
    }
}

/// Mirrors the sidebar source resolution in `render/html.rs`: the provenance
/// flag, not `sidebar.is_some()`, decides which field drives the rail.
fn sidebar_reference(doc: &ParsedDocument) -> Option<&str> {
    if doc.from_sidebar_alias.unwrap_or(false) {
        doc.sidebar.as_deref()
    } else if doc.children_in.as_deref() == Some("sidebar") {
        doc.children_source.as_deref()
    } else {
        None
    }
}

/// Mirrors `render/html.rs`'s `resolve_children_source_folder_path` lookup
/// for `children_source:`.
fn resolve_children_source_slug(reference: &str, documents: &[ParsedDocument]) -> Option<String> {
    let stem = crate::build::markdown::frontmatter_ref_to_stem(reference);
    let found = documents.iter().find(|d| {
        let d_stem = d.url_path.trim_end_matches("/index.html").trim_end_matches('/');
        d_stem.eq_ignore_ascii_case(&stem) && d.kind == PageKind::Folder
    });
    match found {
        Some(d) => Some(d.url_path.trim_end_matches("index.html").trim_end_matches('/').to_string()),
        // html.rs falls back to `{stem}/` here. Keep the fallback only when it
        // names a folder that exists in URL space; otherwise the host's shape
        // is not modelled and it must render.
        None => {
            let guess = stem.to_lowercase();
            let target = format!("{}/index.html", guess);
            documents.iter().any(|d| d.url_path == target).then_some(guess)
        }
    }
}

/// Mirrors `render/html.rs`'s sidebar target lookup (last url segment OR
/// `clean_stem`, case-insensitive).
fn resolve_sidebar_slug(reference: &str, documents: &[ParsedDocument]) -> Option<String> {
    let target_name = crate::build::markdown::frontmatter_ref_to_stem(reference);
    documents
        .iter()
        .find(|d| {
            if !d.url_path.ends_with("/index.html") {
                return false;
            }
            let slug = d.url_path.trim_end_matches("/index.html");
            let last_segment = slug.rsplit('/').next().unwrap_or(slug);
            last_segment.eq_ignore_ascii_case(&target_name)
                || d.clean_stem.eq_ignore_ascii_case(&target_name)
        })
        .map(|d| d.url_path.trim_end_matches("/index.html").to_string())
}

/// The build-global inputs to card rendering that no per-child projection
/// covers (moss#968 FM-4).
///
/// A mismatch is a full-render bypass alongside `asset_versions`, not a
/// per-group diff: these move the rendered listing of *every* group at once,
/// so there is nothing a group-scoped digest would buy.
///
/// `has_language_trees` / `has_content_folders` are in here because they feed
/// `select_children_by_slug` **directly** — they change membership without any
/// document changing.
pub fn listing_globals(
    project: &ProjectStructure,
    dir_overrides: &HashMap<String, String>,
    site_lang: crate::i18n::Language,
    typesetting: Option<&str>,
    math: bool,
) -> BTreeMap<String, String> {
    let overrides: BTreeMap<&str, &str> = dir_overrides
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    // One digest PER INPUT rather than one over the tuple. Same verdict —
    // any entry differing is the same bypass the combined hash triggered —
    // but a verdict that can say which of the eight moved. `image_files` is
    // the one that matters: a cover arriving or being enriched full-renders
    // the site through this branch, and the combined hash could only report
    // that as "listing globals moved".
    [
        ("math", debug_hash(&math)),
        ("typesetting", debug_hash(&typesetting)),
        ("site_lang", debug_hash(&site_lang)),
        ("dir_overrides", debug_hash(&overrides)),
        ("has_language_trees", debug_hash(&project.has_language_trees)),
        ("has_content_folders", debug_hash(&project.has_content_folders)),
        // Media dimensions reach cards through `MediaDimensionLookup`
        // (width/height/LQIP/dominant colour on every cover `<img>`). The
        // lookup itself is built inside the render loop, so hash the table it
        // is built from.
        ("image_files", debug_hash(&project.image_files)),
        ("video_files", debug_hash(&project.video_files)),
    ]
    .into_iter()
    .map(|(name, digest)| (name.to_string(), digest))
    .collect()
}

#[cfg(test)]
#[path = "listing_tests.rs"]
mod tests;
