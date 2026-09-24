//! `RenderVerdict` — the one authority on "which pages must this build
//! re-render".
//!
//! Moved here verbatim from `render/blocking.rs`, where the computation was
//! produced, logged, and then consumed by exactly one `partition` fifty lines
//! below. Nothing else in the build could learn it, which is why three
//! separate deltas exist in the pipeline and none is authoritative.
//!
//! The verdict is **unforgeable**: `RenderVerdict` has no public constructor
//! from a raw set, so a downstream pass can ask it but cannot invent one — the
//! same discipline `BuildStopped::Deferred` uses.
//!
//! Produces the set of SOURCE paths whose page the render loop may leave
//! alone because nothing about this build can have changed their HTML.
//!
//! Escape hatches, checked before any page is spared, each falling back to
//! "render everything" (Bazel/Make's over-approximate-never-under-approximate
//! discipline) and each **named** rather than folded into one boolean — a site
//! permanently taking the full path used to look identical to one that never
//! needs it:
//!
//!   * [`FullCause::SkipDisabled`] — `IncrementalPolicy::skip_unchanged_renders`
//!     false: every entry point except a markdown-only watch rebuild, plus the
//!     `MOSS_NO_INCREMENTAL` kill switch.
//!   * [`FullCause::ColdCache`] — a cold or unreadable cache; nothing to diff.
//!   * [`FullCause::PathSetMoved`] — a page appeared or disappeared, which
//!     moves listings, nav and folder indexes no per-page diff can model.
//!   * [`FullCause::SurfaceChanged`] — ANY page's cross-page-visible surface
//!     moved. The design's six non-graph render dependencies (site nav,
//!     breadcrumbs, series siblings, homepage title, folder-embed listings,
//!     translation counterparts) all read surface fields.
//!   * [`FullCause::GlobalInvalidator`] — a page whose BODY feeds every other
//!     page's HTML changed.
//!   * [`FullCause::AssetVersionsMoved`] — a content-addressed asset moved, so
//!     the hashed filename every page references moved with it.
//!   * [`FullCause::ListingGlobalsMoved`] — a build-global input to card
//!     rendering moved.
//!   * [`FullCause::LangGlobalsMoved`] — a build-global input to the nav
//!     language switcher or the subscribe-form language sections moved
//!     (`render::lang_roots::lang_switcher_globals`).
//!
//! Past those, the facade-changed pages, their graph dependents, and the
//! listing hosts whose group digests moved render. Cache load/save is
//! best-effort: a failure logs and yields an empty skip set, i.e. today's
//! behavior.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::build::facade::{self, FacadeCache, PageFingerprints};
use crate::build::phase::PhaseTrace;
use crate::build::types::ParsedDocument;
use crate::types::content::ProjectStructure;

use super::listing::{self, ListingGroups};
use super::policy::IncrementalPolicy;

/// Why a build rendered everything. Named, not counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullCause {
    SkipDisabled,
    ColdCache,
    PathSetMoved,
    SurfaceChanged,
    GlobalInvalidator,
    AssetVersionsMoved,
    ListingGlobalsMoved,
    LangGlobalsMoved,
}

impl FullCause {
    fn as_str(self) -> &'static str {
        match self {
            FullCause::SkipDisabled => "skip disabled",
            FullCause::ColdCache => "cold cache",
            FullCause::PathSetMoved => "tracked path set moved",
            FullCause::SurfaceChanged => "a page surface moved",
            FullCause::GlobalInvalidator => "a global-invalidator page changed",
            FullCause::AssetVersionsMoved => "asset versions moved",
            FullCause::ListingGlobalsMoved => "listing globals moved",
            FullCause::LangGlobalsMoved => "language-switcher globals moved",
        }
    }
}

/// How the verdict was reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerdictBasis {
    /// Everything rendered, and the EVIDENCE for it.
    ///
    /// The cause alone says a page surface moved; it does not say which page,
    /// and on a 226-page vault that is the difference between a one-line log
    /// read and a bisect. `witness` names up to three of the paths (or inputs)
    /// that forced the full render — `None` for causes that are about no
    /// particular path (`SkipDisabled`, `ColdCache`).
    Full(FullCause, Option<String>),
    Incremental {
        tracked: usize,
        changed: usize,
        surface_changed: usize,
        by_backlink: usize,
        by_listing_group: usize,
        groups: usize,
    },
}

/// The one authority on "which pages must this build re-render".
///
/// Constructed only by [`compute`]; there is no public constructor from a raw
/// set.
#[derive(Debug, Clone)]
pub struct RenderVerdict {
    skip: HashSet<String>,
    basis: VerdictBasis,
}

impl RenderVerdict {
    /// True when this build may leave `source_path`'s page alone.
    ///
    /// Callers must still PROVE the skip is safe (the file is on disk, the
    /// previous manifest still carries the entry) — this answers only the
    /// diff's half of the question.
    pub fn may_skip(&self, source_path: &str) -> bool {
        self.skip.contains(source_path)
    }

    pub fn basis(&self) -> &VerdictBasis {
        &self.basis
    }

    /// A verdict that is not logged is a third copy of Finding 1 (FM-5).
    pub fn log(&self) {
        match &self.basis {
            VerdictBasis::Full(cause, witness) => log::info!(
                target: "incremental",
                "full render: {}{}",
                cause.as_str(),
                match witness {
                    Some(w) => format!(" — {w}"),
                    None => String::new(),
                },
            ),
            VerdictBasis::Incremental {
                tracked,
                changed,
                surface_changed,
                by_backlink,
                by_listing_group,
                groups,
            } => log::info!(
                target: "incremental",
                "{tracked} tracked pages, {changed} changed ({surface_changed} by surface), \
                 +{by_backlink} by backlink/embed, +{by_listing_group} by listing group \
                 ({groups} groups), skipping {}",
                self.skip.len(),
            ),
        }
    }
}

/// Everything the verdict needs that is not a document.
pub struct VerdictInputs<'a> {
    pub policy: IncrementalPolicy,
    pub project: &'a ProjectStructure,
    pub cache_path: &'a Path,
    pub asset_versions: &'a str,
    pub dir_overrides: &'a HashMap<String, String>,
    pub site_lang: crate::i18n::Language,
    pub typesetting: Option<&'a str>,
    pub math: bool,
}

/// Name up to three of the paths behind a full-render cause, sorted so the
/// line is stable across `HashMap` iteration orders and diffable between runs.
fn name_a_few(mut items: Vec<String>, noun: &str) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    items.sort();
    let total = items.len();
    let shown = items
        .iter()
        .take(3)
        .map(|p| format!("\"{p}\""))
        .collect::<Vec<_>>()
        .join(", ");
    Some(if total > 3 {
        format!("{total} {noun}(s), e.g. {shown}")
    } else {
        format!("{total} {noun}(s): {shown}")
    })
}

/// Name the pages whose surface moved AND the fields that moved on them.
///
/// The page alone was all this line used to say, and it was not enough: a
/// surface is a hash of ~60 fields, so "this page's surface moved" sends the
/// reader to guess which one. Three separate diagnoses of one 223-page full
/// render each guessed a different field, and two of them were wrong, before
/// `hero_html` turned out to be the answer. Now the verdict says it.
///
/// Fields are unioned across the named pages rather than listed per page —
/// what a reader needs first is *which field* is doing this, and one moved
/// field on many pages is the common shape.
fn name_surface_movers(
    changed: &[String],
    previous: &FacadeCache,
    current: &HashMap<String, PageFingerprints>,
) -> Option<String> {
    let pages = name_a_few(changed.to_vec(), "page")?;
    let names = facade::surface_field_names();
    let mut fields: Vec<String> = changed
        .iter()
        .filter_map(|path| current.get(path).map(|fp| (path, fp)))
        .flat_map(|(path, fp)| {
            facade::moved_surface_fields(previous.surface_fields_of(path), &fp.fields, &names)
        })
        .collect();
    fields.sort();
    fields.dedup();
    Some(if fields.is_empty() {
        pages
    } else {
        format!("{pages} — field(s): {}", fields.join(", "))
    })
}

/// Compute the verdict and persist this build's fingerprints for the next one.
pub fn compute(documents: &[ParsedDocument], inputs: &VerdictInputs<'_>) -> RenderVerdict {
    use rayon::prelude::*;
    let _phase_facade = PhaseTrace::start("incremental_facade_diff");

    // Each page's fingerprint is a pure function of that page alone (no
    // cross-page state), same independence the markdown-parse and html-render
    // loops already exploit with par_iter — this loop was the one left serial,
    // and at 216+ pages it dominated wall time on a single core (follow-up,
    // 2026-08-01).
    let current: HashMap<String, PageFingerprints> = documents
        .par_iter()
        .filter_map(|doc| {
            doc.source_path
                .as_ref()
                .map(|p| (p.clone(), PageFingerprints::of(doc)))
        })
        .collect();

    let previous = FacadeCache::load(inputs.cache_path);
    let changed = previous.changed_paths(&current);
    let surface_changed = previous.surface_changed_paths(&current);

    // A page whose BODY feeds every other page's HTML. Three kinds exist, all
    // found by auditing every cross-page read of another document's
    // `content`/`html_content`: the root homepage, a language home, and any
    // slot page (`footer.md` and friends, injected into every page by
    // `build/footer.rs`). None of these edges is a link, so no graph carries
    // them.
    //
    // What is compared is the CONTRIBUTION, not the page. A home page's whole
    // facade used to invalidate the site, but the only thing other pages read
    // out of its body is the extracted excerpt that lands in their
    // `<meta name="description">` (rung 6 of the chain in
    // `page/meta.rs::resolve_page_description_with_fallbacks`). Its other
    // cross-page reads — `cover`, `hero_image_url`, `description`,
    // `hero_overlay_text` — are frontmatter or hoisted fields, so they are
    // already in the SURFACE and already force a full render one branch up.
    // Editing a homepage paragraph below the excerpt therefore now costs the
    // pages that link to it, not the whole site (223 pages per keystroke on
    // harbor). A slot page has no such narrowing: its rendered body IS
    // spliced into every page, so its whole facade stays the contribution.
    //
    // SEE ALSO — there are TWO whole-build bypasses, not one, and they are
    // computed by entirely separate code paths. This one is POST-parse and
    // CONTENT-based; its pre-parse, input-SHAPE-based sibling is
    // `build::parse_cache::inputs_fingerprint`, which bypasses the Loop A
    // parse cache when `page_map`/`dir_overrides`/`external_url_map`/
    // `home_file_winners` move. If you are bisecting a stale-page report,
    // check both — neither one subsumes the other.
    let contributions = global_contributions(documents, inputs.math);
    let global_invalidator_changed = previous.global_contributions_changed(&contributions);

    // Third whole-build bypass (the SEE ALSO note above counted two): a
    // content-addressed asset moved, so the hashed filename every page
    // references moved with it. Unlike the other two this is not about any
    // page's content — a page can be byte-identical and still need
    // re-rendering, because the file it points at was replaced by one under a
    // different name. Causes include this build shipping a different set of
    // stylesheet partials, an edit to `site.css` or `tokens.json`, and a newer
    // moss binary.
    let asset_versions_changed = previous.asset_versions_changed(inputs.asset_versions);

    // Fourth: the build-global inputs to card rendering.
    let listing_globals = listing::listing_globals(
        inputs.project,
        inputs.dir_overrides,
        inputs.site_lang,
        inputs.typesetting,
        inputs.math,
    );
    let listing_globals_changed = previous.listing_globals_changed(&listing_globals);

    // Fifth: the build-global inputs to the nav language switcher, the
    // nav/footer link lists, and the subscribe-form language sections — see
    // `lang_switcher_globals`'s doc comment for why these are hashed whole
    // rather than attributed per page.
    let lang_globals = super::super::lang_roots::lang_switcher_globals(
        documents,
        inputs.site_lang,
        inputs.project.has_content_folders,
    );
    let lang_globals_changed = previous.lang_globals_changed(&lang_globals);

    // Listing groups: computed unconditionally, because this build's digests
    // must be persisted for the NEXT build even when this one renders
    // everything. Sub-millisecond on the reference vault.
    let groups = ListingGroups::build(documents, inputs.project, inputs.math);

    let full_cause = if !inputs.policy.skip_unchanged_renders {
        Some((FullCause::SkipDisabled, None))
    } else if previous.is_empty() {
        Some((FullCause::ColdCache, None))
    } else if !previous.covers_same_paths(&current) {
        Some((
            FullCause::PathSetMoved,
            name_a_few(previous.paths_symmetric_difference(&current), "page"),
        ))
    } else if !surface_changed.is_empty() {
        Some((
            FullCause::SurfaceChanged,
            name_surface_movers(&surface_changed, &previous, &current),
        ))
    } else if !global_invalidator_changed.is_empty() {
        Some((FullCause::GlobalInvalidator, name_a_few(global_invalidator_changed, "page")))
    } else if asset_versions_changed {
        Some((FullCause::AssetVersionsMoved, None))
    } else if !listing_globals_changed.is_empty() {
        Some((FullCause::ListingGlobalsMoved, name_a_few(listing_globals_changed, "input")))
    } else if !lang_globals_changed.is_empty() {
        Some((FullCause::LangGlobalsMoved, name_a_few(lang_globals_changed, "input")))
    } else {
        None
    };

    let (skip, basis) = match full_cause {
        Some((cause, witness)) => (HashSet::new(), VerdictBasis::Full(cause, witness)),
        None => {
            let path_and_links: Vec<(String, &[moss_core::resolve::OutgoingLink])> = documents
                .iter()
                .filter_map(|doc| {
                    doc.source_path
                        .as_ref()
                        .map(|p| (p.clone(), doc.outgoing_links.as_slice()))
                })
                .collect();
            // Transclusion edges. Until `embed_deps` was
            // threaded onto `ParsedDocument`, `back_embeds` was real machinery
            // over an empty relation for pages — `![[note.md]]` never produces
            // a `LinkType::Embed` outgoing link. Folding
            // them in here widens the render set to the pages a changed page's
            // bytes are spliced into, which is strictly more rendering, never
            // less.
            let embed_pairs: Vec<(&str, &str)> = documents
                .iter()
                .flat_map(|doc| {
                    doc.embed_deps
                        .iter()
                        .map(|(target, embedder)| (target.as_str(), embedder.as_str()))
                })
                .collect();
            let dep_graph = moss_core::dep_graph::DepGraph::build(
                path_and_links.iter().map(|(k, v)| (k.as_str(), *v)),
            )
            .with_embed_pairs(embed_pairs);
            let mut render_set: HashSet<String> = changed.iter().cloned().collect();
            for path in &changed {
                render_set.extend(dep_graph.backlinks(path).iter().cloned());
                render_set.extend(dep_graph.back_embeds(path).iter().cloned());
            }
            // `changed` are distinct map keys and all landed in the set first.
            let by_backlink = render_set.len() - changed.len();

            // Listing hosts read each listed child's RAW BODY at render time,
            // and folder membership is a URL prefix rather than a link, so the
            // dependency graph has no edge for it. Before this model every page
            // that COULD host a listing rendered unconditionally — 114 of 214
            // on the reference vault. It now renders iff a group it reads has
            // a moved digest.
            //
            // An inline `![[/dir/]]` folder embed does NOT need to be listed
            // here: `expand_markers_in_documents` splices its listing into the
            // embedding page's own body during the whole-corpus passes above,
            // i.e. BEFORE these fingerprints are taken, so a child's edit
            // already moves the embedding page's facade.
            let mut by_listing_group = 0usize;
            for doc in documents {
                if !listing::hosts_listing(doc) {
                    continue;
                }
                let Some(source) = doc.source_path.as_ref() else {
                    // No source path, hence no facade entry and nothing to
                    // skip. The synthetic folder-index loop renders these.
                    continue;
                };
                if render_set.contains(source) {
                    continue;
                }
                let dirty = match listing::groups_read_by(doc, documents) {
                    // Unknown host shape → render. rustc's `eval_always`:
                    // over-approximate is the only safe default, and it is
                    // what today's code does for all of them.
                    None => true,
                    Some(read) => read
                        .iter()
                        .any(|key| previous.listing_digest(key) != groups.digest(key)),
                };
                if dirty {
                    render_set.insert(source.clone());
                    by_listing_group += 1;
                }
            }

            let skip: HashSet<String> = current
                .keys()
                .filter(|p| !render_set.contains(*p))
                .cloned()
                .collect();
            let basis = VerdictBasis::Incremental {
                tracked: current.len(),
                changed: changed.len(),
                surface_changed: surface_changed.len(),
                by_backlink,
                by_listing_group,
                groups: groups.len(),
            };
            (skip, basis)
        }
    };

    let new_cache = FacadeCache::from_facades(current)
        .with_asset_versions(inputs.asset_versions.to_string())
        .with_global_contributions(contributions)
        .with_listing(listing_globals, groups.into_map())
        .with_lang_globals(lang_globals);
    if let Err(e) = new_cache.save(inputs.cache_path) {
        log::warn!(target: "incremental", "failed to save facade cache: {e}");
    }

    let verdict = RenderVerdict { skip, basis };
    verdict.log();
    verdict
}

/// Every body-derived value that reaches another page's HTML, folded in
/// source-path order so the digest is stable across a rebuild.
///
/// A home page contributes its extracted excerpt (`description_from_content`,
/// rung 6) — the one body-derived value another page reads. A slot page
/// contributes its whole facade, because `build/footer.rs` splices its
/// rendered body into every page verbatim.
///
/// Over-approximating is the safe direction here and this does it twice: the
/// excerpt is recomputed rather than read from a cache, and a page that is
/// BOTH a home and a slot contributes as a slot. Under-approximating would
/// serve a stale `<meta name="description">` site-wide.
fn global_contributions(
    documents: &[ParsedDocument],
    math: bool,
) -> std::collections::BTreeMap<String, String> {
    let mut parts: Vec<(String, String)> = Vec::new();
    for doc in documents {
        let Some(path) = doc.source_path.as_ref() else {
            continue;
        };
        let is_slot = doc.slot_only || doc.slot.is_some();
        let is_home = doc.url_path == "index.html" || doc.is_home_override;
        if is_slot {
            parts.push((path.clone(), crate::build::facade::compute_page_facade(doc)));
        } else if is_home {
            let excerpt =
                crate::build::page::meta::description_from_content(Some(&doc.content), math)
                    .unwrap_or_default();
            parts.push((path.clone(), excerpt));
        }
    }
    // Keyed by the contributing page. The parts were always computed here;
    // hashing them into one string was what left `FullCause::GlobalInvalidator`
    // unable to name the page it fired on.
    parts
        .into_iter()
        .map(|(path, value)| {
            (path, format!("{:016x}", xxhash_rust::xxh3::xxh3_64(value.as_bytes())))
        })
        .collect()
}

#[cfg(test)]
#[path = "verdict_tests.rs"]
mod tests;
