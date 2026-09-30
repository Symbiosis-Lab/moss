//! Verdict tests.
//!
//! The escape hatches are asserted by NAME here — the whole point of
//! `FullCause` is that a site permanently taking the full path stops looking
//! identical to one that never needs it.

use super::*;
use crate::build::render::incremental::listing::{Depth, GroupKey};
use moss_core::PageKind;

fn project() -> ProjectStructure {
    ProjectStructure {
        root_path: "/tmp/moss-verdict-tests".into(),
        markdown_files: vec![],
        html_files: vec![],
        image_files: vec![],
        video_files: vec![],
        notebook_files: vec![],
        other_files: vec![],
        total_files: 0,
        homepage_file: None,
        ffmpeg_bin_path: None,
        evicted_count: 0,
        evicted_paths: Vec::new(),
        has_content_folders: true,
        has_language_trees: false,
        passthrough_roots: std::collections::HashSet::new(),
        dirs: Vec::new(),
    }
}

fn doc(url: &str, kind: PageKind, body: &str, description: Option<&str>) -> ParsedDocument {
    let stem = url.trim_end_matches("/index.html").rsplit('/').next().unwrap_or(url);
    ParsedDocument {
        url_path: url.to_string(),
        source_path: Some(if url == "index.html" {
            "index.md".to_string()
        } else {
            url.replace("index.html", "index.md")
        }),
        title: stem.to_string(),
        label: stem.to_string(),
        clean_stem: stem.to_string(),
        kind,
        content: body.to_string(),
        description: description.map(str::to_string),
        ..Default::default()
    }
}

/// One folder index (a listing host) over two described articles, plus the
/// root home. The two hosts are what re-rendered unconditionally before
/// this model.
fn vault() -> Vec<ParsedDocument> {
    vec![
        doc("index.html", PageKind::Folder, "home body", Some("Home")),
        doc("writings/index.html", PageKind::Folder, "folder body", Some("Writings")),
        doc("writings/alpha/index.html", PageKind::Article, "Alpha lede.\n\nAlpha tail.", Some("A")),
        doc("writings/beta/index.html", PageKind::Article, "Beta lede.\n\nBeta tail.", Some("B")),
    ]
}

struct Harness {
    _dir: tempfile::TempDir,
    cache: std::path::PathBuf,
    output: std::path::PathBuf,
    project: ProjectStructure,
    overrides: std::collections::HashMap<String, String>,
    skip: bool,
    globals_salt: bool,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("dep-cache.json");
        let output = dir.path().join("out");
        Self {
            _dir: dir,
            cache,
            output,
            project: project(),
            overrides: std::collections::HashMap::new(),
            skip: true,
            globals_salt: false,
        }
    }

    fn run(&self, documents: &[ParsedDocument]) -> RenderVerdict {
        let mut project = self.project.clone();
        // Any listing global will do to prove the bypass; this one is a
        // straight boolean on the struct.
        project.has_language_trees = self.globals_salt;
        compute(
            documents,
            &VerdictInputs {
                policy: IncrementalPolicy { skip_unchanged_renders: self.skip },
                project: &project,
                cache_path: &self.cache,
                output_dir: &self.output,
                asset_versions: "v1",
                dir_overrides: &self.overrides,
                site_lang: crate::i18n::Language::En,
                typesetting: None,
                math: false,
            },
        )
    }
}

#[track_caller]
fn assert_full(verdict: &RenderVerdict, cause: FullCause) {
    assert!(
        matches!(verdict.basis(), VerdictBasis::Full(c, _) if *c == cause),
        "expected Full({cause:?}), got {:?}",
        verdict.basis()
    );
}

// ---- the number this whole design exists to move -------------------------

/// The Stage-0 baseline on `riverbend/河灣`: a no-op save re-renders 114 of
/// 214 pages, of which every listing host was rendered *by the predicate*, not
/// by the diff. After Stage 2 the same save must skip every page — hosts
/// included.
#[test]
fn a_no_op_rebuild_skips_every_page_including_every_listing_host() {
    let h = Harness::new();
    let docs = vault();

    assert_full(&h.run(&docs), FullCause::ColdCache);

    let verdict = h.run(&docs);
    for d in &docs {
        let src = d.source_path.as_deref().unwrap();
        assert!(verdict.may_skip(src), "{src} must be carried on a no-op save");
    }
    match verdict.basis() {
        VerdictBasis::Incremental { tracked, changed, by_listing_group, .. } => {
            assert_eq!((*tracked, *changed, *by_listing_group), (4, 0, 0));
        }
        other => panic!("expected an incremental verdict, got {other:?}"),
    }
}

/// A body edit to a child that carries a frontmatter `description:` renders
/// that page and nothing else. Before this model it rendered the page *and*
/// both listing hosts.
#[test]
fn a_body_edit_below_the_lede_renders_one_page_and_no_host() {
    let h = Harness::new();
    h.run(&vault());

    let mut edited = vault();
    edited[2].content = "Alpha lede.\n\nA COMPLETELY DIFFERENT tail.".into();
    let verdict = h.run(&edited);

    assert!(!verdict.may_skip("writings/alpha/index.md"), "the edited page renders");
    assert!(verdict.may_skip("writings/index.md"), "its folder index must be carried");
    assert!(verdict.may_skip("index.md"), "the root home must be carried");
    match verdict.basis() {
        VerdictBasis::Incremental { changed, by_listing_group, .. } => {
            assert_eq!((*changed, *by_listing_group), (1, 0));
        }
        other => panic!("expected an incremental verdict, got {other:?}"),
    }
}

/// The other direction: the same child WITHOUT a `description:`, edited in its
/// first paragraph, moves the group's `contents` and the host must render.
/// This is the design's core claim and the test that would catch a stale card.
#[test]
fn a_lede_edit_on_an_undescribed_child_re_renders_its_listing_host() {
    let h = Harness::new();
    let mut docs = vault();
    docs[2].description = None;
    h.run(&docs);

    docs[2].content = "A rewritten lede.\n\nAlpha tail.".into();
    let verdict = h.run(&docs);

    assert!(!verdict.may_skip("writings/alpha/index.md"), "the edited page renders");
    assert!(
        !verdict.may_skip("writings/index.md"),
        "its folder index shows the excerpt, so it must re-render"
    );
    match verdict.basis() {
        VerdictBasis::Incremental { by_listing_group, .. } => assert!(*by_listing_group >= 1),
        other => panic!("expected an incremental verdict, got {other:?}"),
    }
}

// ---- the named escape hatches -------------------------------------------

#[test]
fn each_full_render_says_why() {
    // SkipDisabled — the total off switch.
    let mut h = Harness::new();
    h.run(&vault());
    h.skip = false;
    assert_full(&h.run(&vault()), FullCause::SkipDisabled);

    // PathSetMoved — a page appeared.
    let h = Harness::new();
    h.run(&vault());
    let mut grown = vault();
    grown.push(doc("writings/gamma/index.html", PageKind::Article, "g", Some("G")));
    assert_full(&h.run(&grown), FullCause::PathSetMoved);

    // SurfaceChanged — a cross-page-visible field moved by an UNCLASSIFIED
    // field (`children_source`, which `field_is_classified` does not name).
    // A classified field (e.g. `label` on a listed child) instead
    // narrows; see `a_classified_label_edit_narrows_to_its_listing_host`.
    let h = Harness::new();
    h.run(&vault());
    let mut redirected = vault();
    redirected[2].children_source = Some("writings".to_string());
    assert_full(&h.run(&redirected), FullCause::SurfaceChanged);

    // GlobalInvalidator — the root homepage's EXCERPT feeds every page's
    // <meta name="description">, so moving it is site-wide.
    let h = Harness::new();
    h.run(&vault());
    let mut home_edit = vault();
    home_edit[0].content = "a new home body".into();
    assert_full(&h.run(&home_edit), FullCause::GlobalInvalidator);

    // ListingGlobalsMoved — a build-global input to card rendering (FM-4).
    let mut h = Harness::new();
    h.run(&vault());
    h.globals_salt = true;
    assert_full(&h.run(&vault()), FullCause::ListingGlobalsMoved);

    // LangGlobalsMoved — a FOOTER-eligible page's own `lang` changes which
    // OTHER pages' auto-generated footer link list it appears in
    // (`components/nav.rs::generate_footer` filters the whole corpus by
    // `d.footer == Some(true) && d.lang == self.current_lang`, on every
    // render). Found in review: the first cut of this fix only hashed the
    // switcher + subscribe-form globals and missed this nav/footer channel.
    let h = Harness::new();
    let mut docs = vault();
    docs.push(doc("colophon/index.html", PageKind::Article, "Colophon.", Some("C")));
    let last = docs.len() - 1;
    docs[last].footer = Some(true);
    docs[last].lang = crate::i18n::Language::En;
    h.run(&docs);
    docs[last].lang = crate::i18n::Language::ZhHans;
    assert_full(&h.run(&docs), FullCause::LangGlobalsMoved);
}

/// The actual false positive: an ordinary leaf page with
/// NO translation-group siblings has its `lang` re-decided (frontmatter edit,
/// or content-detection flipping on a rewritten paragraph). Nothing else in
/// the corpus reads this page's raw `.lang` — `translations` on every OTHER
/// doc is untouched, since this page isn't listed in anyone's translation
/// group — so the verdict must stay incremental, not full-render 223 pages
/// for a field only this page's own facade cares about.
#[test]
fn a_lang_change_with_no_translation_group_stays_incremental() {
    use crate::i18n::Language;

    let h = Harness::new();
    let mut docs = vault();
    docs.push(doc("writings/gamma/index.html", PageKind::Article, "Gamma lede.\n\nGamma tail.", Some("G")));
    let idx = docs.len() - 1;
    docs[idx].lang = Language::ZhHans;
    h.run(&docs);

    docs[idx].lang = Language::ZhHant;
    let verdict = h.run(&docs);
    assert!(!verdict.may_skip("writings/gamma/index.md"), "the edited page renders");
    for src in ["index.md", "writings/index.md", "writings/alpha/index.md", "writings/beta/index.md"] {
        assert!(verdict.may_skip(src), "{src} reads nothing that moved");
    }
    match verdict.basis() {
        VerdictBasis::Incremental { .. } => {}
        other => panic!("a page's own lang, with no translation siblings, must not full-render the site, got {other:?}"),
    }
}

/// A genuine translation PAIR is a different case, and this test's original
/// form wrongly expected it to also stay
/// incremental. It can't: `translations` has a real cross-page reader the
/// dependency graph cannot see — `render/blocking.rs`'s auto-index folder
/// filter reads an arbitrary OTHER document's `.translations` (via
/// `documents.iter().find(|d| d.url_path == top_index)`) to decide whether a
/// whole folder is a translation-root and should be excluded from
/// auto-generated indexes, which shapes what OTHER pages get synthesized.
/// `translations` therefore correctly stays in the surface, and a sibling's
/// entry moving correctly still trips `FullCause::SurfaceChanged` — that is
/// pre-existing, unrelated-to-`lang` behavior, not the bug this task fixes.
#[test]
fn a_translation_siblings_lang_change_still_full_renders_via_surface_changed() {
    use crate::i18n::link::TranslationLink;
    use crate::i18n::Language;

    let h = Harness::new();
    let mut docs = vault();
    let mut a = doc("writings/gamma/index.html", PageKind::Article, "Gamma lede.\n\nGamma tail.", Some("G"));
    let mut b = doc("en/gamma/index.html", PageKind::Article, "Gamma EN lede.\n\nEN tail.", Some("G EN"));
    a.lang = Language::ZhHans;
    b.lang = Language::En;
    a.translations = vec![TranslationLink { lang_tag: b.lang.as_bcp47_attr().to_string(), url_path: b.url_path.clone(), display_name: "EN" }];
    b.translations = vec![TranslationLink { lang_tag: a.lang.as_bcp47_attr().to_string(), url_path: a.url_path.clone(), display_name: "ZH" }];
    docs.push(a);
    docs.push(b);
    h.run(&docs);

    let b_idx = docs.len() - 1;
    let a_idx = docs.len() - 2;
    docs[b_idx].lang = Language::ZhHant;
    docs[a_idx].translations =
        vec![TranslationLink { lang_tag: Language::ZhHant.as_bcp47_attr().to_string(), url_path: docs[b_idx].url_path.clone(), display_name: "繁" }];

    assert_full(&h.run(&docs), FullCause::SurfaceChanged);
}

/// The narrowing that pays for the bypass: a homepage edit BELOW the excerpt
/// reaches no other page, so it must not re-render the site.
///
/// The old rule tested the home page's whole facade, so every keystroke while
/// editing the homepage was a full render — 223 pages on riverbend. The only
/// body-derived value another page reads out of a home is rung 6 of the
/// description chain, and this edit does not move it.
#[test]
fn a_homepage_edit_below_the_excerpt_is_not_a_global_invalidator() {
    let h = Harness::new();
    let mut docs = vault();
    docs[0].content = "The home lede.\n\nA tail nobody else reads.".into();
    h.run(&docs);

    docs[0].content = "The home lede.\n\nA COMPLETELY REWRITTEN tail.".into();
    let verdict = h.run(&docs);

    assert!(!verdict.may_skip("index.md"), "the homepage itself still renders");
    for src in ["writings/index.md", "writings/alpha/index.md", "writings/beta/index.md"] {
        assert!(verdict.may_skip(src), "{src} reads nothing that moved");
    }
}

/// The other direction, and the one that would serve a stale
/// `<meta name="description">` site-wide if the narrowing were wrong: the same
/// homepage edited IN its lede moves rung 6 and must still be site-wide.
#[test]
fn a_homepage_lede_edit_is_still_a_global_invalidator() {
    let h = Harness::new();
    let mut docs = vault();
    docs[0].content = "The home lede.\n\nA tail nobody else reads.".into();
    h.run(&docs);

    docs[0].content = "A REWRITTEN home lede.\n\nA tail nobody else reads.".into();
    assert_full(&h.run(&docs), FullCause::GlobalInvalidator);
}

/// A slot page gets NO narrowing: `build/footer.rs` splices its rendered body
/// into every page verbatim, so any part of its body is a global contribution.
#[test]
fn a_slot_page_body_edit_is_still_a_global_invalidator() {
    let h = Harness::new();
    let mut docs = vault();
    let mut footer = doc("footer.html", PageKind::Article, "Lede.\n\nTail.", None);
    footer.source_path = Some("footer.md".into());
    footer.slot_only = true;
    docs.push(footer);
    h.run(&docs);

    // Edited below the lede — narrowed for a home, never for a slot.
    docs[4].content = "Lede.\n\nA REWRITTEN tail.".into();
    assert_full(&h.run(&docs), FullCause::GlobalInvalidator);
}

/// A cache written before the listing groups existed has no digests, so every
/// group reads as moved: every host renders exactly once and the next build
/// settles. Same fail-safe direction as `asset_versions`.
#[test]
fn a_cache_without_listing_digests_renders_every_host_exactly_once() {
    let h = Harness::new();
    h.run(&vault());

    // Strip the two listing-digest fields, leaving an older-era cache.
    let raw = std::fs::read_to_string(&h.cache).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let obj = value.as_object_mut().unwrap();
    obj.remove("listing_groups");
    obj.remove("listing_globals");
    std::fs::write(&h.cache, serde_json::to_string(&value).unwrap()).unwrap();

    // The globals mismatch is checked first and is itself a full render...
    assert_full(&h.run(&vault()), FullCause::ListingGlobalsMoved);
    // ...and the build after it settles.
    let settled = h.run(&vault());
    assert!(matches!(settled.basis(), VerdictBasis::Incremental { .. }));
    assert!(settled.may_skip("writings/index.md"));
}

/// The verdict is a value with one owner: nothing outside `compute` can put a
/// path into it. This is the same unforgeability discipline
/// `BuildStopped::Deferred` uses — asserted here as a behavioural fact
/// (a page the diff never cleared is never skippable) rather than left to a
/// code reviewer noticing the missing constructor.
#[test]
fn a_page_absent_from_the_corpus_is_never_skippable() {
    let h = Harness::new();
    h.run(&vault());
    let verdict = h.run(&vault());
    assert!(!verdict.may_skip("not-a-page.md"));
}

/// Groups are keyed by shape, so two hosts reading the same folder at the same
/// depth share one digest rather than each carrying their own copy.
#[test]
fn the_group_set_is_keyed_by_shape_not_by_host() {
    let docs = vault();
    let groups = listing::ListingGroups::build(&docs, &project(), false);
    assert!(groups
        .digest(&GroupKey {
            folder_slug: "writings".into(),
            depth: Depth::Direct,
            scope_default_tree: false,
            exclude_nav: false,
        })
        .is_some());
    assert_eq!(groups.len(), 2, "the root home's group and writings/'s group");
}

/// A `.moss/places.toml`-only edit (no document frontmatter touched) has no
/// dedicated `listing_globals` entry — `listing_globals` is built in exactly
/// one place (`render/incremental/verdict.rs`'s own call, passing only
/// `project`/`dir_overrides`/`site_lang`/`typesetting`/`math`, no
/// `SiteConfig`, no `term_kinds`), so a places-only edit has to move a
/// document's own `also_in` for the verdict to see it at all. It does:
/// `derive_terms`'s roll-up (task A5) writes the parent chain into
/// `also_in`, which is inside the whole-document `Debug` surface
/// `facade.rs::surface_debug` hashes (it strips only a short, explicitly
/// named list of body-only fields, and `also_in` is not one of them), so
/// `FullCause::SurfaceChanged` already catches this with nothing new to
/// wire — no `listing_globals` extension is added here, deliberately.
///
/// Ablated by disabling the ancestor-push loop in `derive_terms` pass 2:
/// `also_in` never moves at all with the loop gone, so there is nothing
/// for the surface diff to see and the verdict stays `Incremental` rather
/// than firing with the wrong membership. That reddens the FIRST
/// assertion below (`assert_full`, verdict stays `Incremental` because
/// `also_in` does not move), not the membership assertion at the end —
/// with the loop gone there is no wrong page left to list the document on.
#[test]
fn a_places_toml_only_edit_full_renders_via_surface_changed_and_moves_membership() {
    let h = Harness::new();

    let places_kind = |parent_of_kyoto: &str| crate::build::terms::TermKind {
        key: "places".to_string(),
        fields: vec!["location".to_string()],
        title: "Places".to_string(),
        is_place: true,
        parents: [("places/kyoto".to_string(), parent_of_kyoto.to_string())].into_iter().collect(),
    };

    let mut before_docs = vec![doc("posts/a/index.html", PageKind::Article, "A body", Some("A"))];
    before_docs[0].location = vec!["Kyoto".to_string()];
    crate::build::terms::derive_terms(&mut before_docs, vec![places_kind("Japan")]);
    h.run(&before_docs);

    // The only change: Kyoto's `parent` in `.moss/places.toml` moves from
    // Japan to Kansai — re-run `derive_terms` the same way a real rebuild
    // would, with no document frontmatter touched.
    let mut after_docs = vec![doc("posts/a/index.html", PageKind::Article, "A body", Some("A"))];
    after_docs[0].location = vec!["Kyoto".to_string()];
    crate::build::terms::derive_terms(&mut after_docs, vec![places_kind("Kansai")]);

    let verdict = h.run(&after_docs);
    assert_full(&verdict, FullCause::SurfaceChanged);
    match verdict.basis() {
        VerdictBasis::Full(_, Some(witness)) => {
            assert!(witness.contains("also_in"), "witness must name also_in: {witness}");
        }
        other => panic!("expected a witness naming the moved field, got {other:?}"),
    }

    // The new ancestor lists the document; the old one no longer does — not
    // merely "a full render happened," but that membership actually moved
    // to the right page.
    let also_in = after_docs[0].also_in.as_ref().expect("Kyoto and Kansai both recorded");
    assert!(also_in.contains(&"places/kansai".to_string()), "got {also_in:?}");
    assert!(!also_in.contains(&"places/japan".to_string()), "got {also_in:?}");
}

// ---- the dependents narrowing: classified surface moves no longer force Full ----

#[track_caller]
fn assert_incremental(verdict: &RenderVerdict) {
    match verdict.basis() {
        VerdictBasis::Incremental { .. } => {}
        other => panic!("expected an incremental (narrowed) verdict, got {other:?}"),
    }
}

/// The narrowest case: a title edit on a leaf article that no listing host,
/// nav bar or breadcrumb trail reads renders only itself.
/// `listed: Some(false)` is what removes it from every listing group's
/// membership (`folder_embed::select_children_by_slug`'s `is_listable`
/// gate) — without it, the root homepage's own default listing would read
/// this leaf too, which is correct behavior but would defeat the point of a
/// SINGLETON assertion here.
#[test]
fn a_leaf_title_edit_render_set_is_bounded_to_the_page_itself() {
    let h = Harness::new();
    let mut docs = vault();
    let mut isolated = doc("writings/gamma/index.html", PageKind::Article, "Gamma lede.\n\nGamma tail.", Some("G"));
    isolated.listed = Some(false);
    docs.push(isolated);
    h.run(&docs);

    let idx = docs.iter().position(|d| d.url_path == "writings/gamma/index.html").unwrap();
    docs[idx].title = "Renamed Gamma".into();
    let verdict = h.run(&docs);
    assert_incremental(&verdict);
    for src in ["index.md", "writings/index.md", "writings/alpha/index.md", "writings/beta/index.md"] {
        assert!(verdict.may_skip(src), "{src} reads nothing that moved");
    }
    assert!(!verdict.may_skip("writings/gamma/index.md"));
}

/// A classified field move (here `label`) on a LISTED child narrows to that
/// child's listing host instead of forcing `FullCause::SurfaceChanged` — the
/// pre-existing group-digest loop (`listing::groups_read_by`) already covers
/// this once the early `Full` return stops short-circuiting it. Confirmed
/// against the real verdict rather than assumed: `writings/beta` — an
/// unrelated sibling — must stay skippable.
#[test]
fn a_classified_label_edit_narrows_to_its_listing_host() {
    let h = Harness::new();
    let docs = vault();
    h.run(&docs);

    let mut relabelled = docs.clone();
    let idx = relabelled.iter().position(|d| d.url_path == "writings/alpha/index.html").unwrap();
    relabelled[idx].label = "Renamed".into();
    let verdict = h.run(&relabelled);
    assert_incremental(&verdict);
    assert!(!verdict.may_skip("writings/alpha/index.md"));
    assert!(!verdict.may_skip("writings/index.md"), "the listing host must see the moved child projection");
    assert!(verdict.may_skip("writings/beta/index.md"), "an unrelated sibling must not be swept in");
}

/// `lang` is blanked out of the surface entirely (`facade::surface_debug`),
/// so a reassigned nav item's own page never even enters `surface_changed` —
/// which would under-render if nothing else caught it. `nav_globals` reads
/// `doc.lang` unblanked and fresh every build for exactly this reason,
/// bucketing a nav-eligible doc by its CURRENT language rather than trusting
/// the surface to notice.
///
/// This specific scenario, though, is ALSO caught by a pre-existing, coarser
/// gate one step earlier in the same `if`/`else if` chain:
/// `render::lang_roots::lang_switcher_globals`'s `nav_footer_lang_membership`
/// hashes `(url_path, lang)` for every nav/footer-eligible doc and forces
/// `FullCause::LangGlobalsMoved` — a whole-site render, same safe direction,
/// independent of the classification this file otherwise tests — whenever a
/// nav-eligible doc's `lang` moves. So
/// the OUTCOME here is `Full`, not a narrowed per-language render; what this
/// test pins is that it is `Full` for a REASON now covered twice, not zero
/// times. `a_nav_items_weight_edit_widens_to_its_language_not_the_site`
/// (below) is the scenario where `lang_switcher_globals` stays silent
/// (weight is not part of its membership tuple) and `nav_globals` is the
/// only thing standing between this edit and the old blanket `Full`.
#[test]
fn a_nav_items_lang_reassignment_is_still_safe_via_the_coarser_lang_globals_gate() {
    use crate::i18n::Language;

    let h = Harness::new();
    let mut docs = vault();
    let mut about = doc("about/index.html", PageKind::Article, "About body", Some("About"));
    about.nav = Some(true);
    about.lang = Language::En;
    docs.push(about);
    h.run(&docs);

    let about_idx = docs.iter().position(|d| d.url_path == "about/index.html").unwrap();
    docs[about_idx].lang = Language::ZhHans;
    assert_full(&h.run(&docs), FullCause::LangGlobalsMoved);
}

/// The same channel, the field that names it: a nav item's `weight` moves the
/// language's whole nav digest (order changed), not just the item's own page.
#[test]
fn a_nav_items_weight_edit_widens_to_its_language_not_the_site() {
    use crate::i18n::Language;

    let h = Harness::new();
    let mut docs = vault();
    let mut about = doc("about/index.html", PageKind::Article, "About body", Some("About"));
    about.nav = Some(true);
    about.weight = Some(1);
    about.lang = Language::En;
    let mut zh_leaf = doc("zh-hans/other/index.html", PageKind::Article, "zh leaf", Some("O"));
    zh_leaf.lang = Language::ZhHans;
    docs.push(about);
    docs.push(zh_leaf);
    h.run(&docs);

    let about_idx = docs.iter().position(|d| d.url_path == "about/index.html").unwrap();
    docs[about_idx].weight = Some(2);
    let verdict = h.run(&docs);
    assert_incremental(&verdict);
    assert!(!verdict.may_skip("about/index.md"));
    assert!(!verdict.may_skip("index.md"), "the en homepage must refresh its nav order");
    assert!(verdict.may_skip("zh-hans/other/index.md"), "a different language must not be swept in");
}

/// The homepage-title channel: editing a language's home page's `title`
/// widens to every page of THAT language, proven by a second language
/// staying untouched.
#[test]
fn a_homepage_title_edit_widens_to_its_language_not_the_site() {
    use crate::i18n::Language;

    let h = Harness::new();
    let mut docs = vault();
    let mut zh_home = doc("zh-hans/index.html", PageKind::Folder, "zh home body", Some("ZH Home"));
    zh_home.lang = Language::ZhHans;
    docs.push(zh_home);
    h.run(&docs);

    docs[0].title = "New Home Title".into();
    let verdict = h.run(&docs);
    assert_incremental(&verdict);
    assert!(!verdict.may_skip("index.md"));
    assert!(!verdict.may_skip("writings/index.md"), "an en page must refresh the site title it shows");
    assert!(verdict.may_skip("zh-hans/index.md"), "a different language's home must not be swept in");
}

/// The breadcrumb-ancestor channel: a folder index's `label` (the ancestor
/// segment text every descendant's trail shows) widens to that folder's
/// descendants — and ONLY that folder's, proven by an unrelated sibling
/// folder staying skippable.
#[test]
fn a_folder_indexs_label_edit_widens_to_its_breadcrumb_descendants_only() {
    let h = Harness::new();
    let mut docs = vault();
    docs[0].breadcrumb = Some(true);
    docs.push(doc("other/index.html", PageKind::Folder, "other body", Some("Other")));
    docs.push(doc("other/leaf/index.html", PageKind::Article, "leaf body", Some("Leaf")));
    h.run(&docs);

    let writings_idx = docs.iter().position(|d| d.url_path == "writings/index.html").unwrap();
    docs[writings_idx].label = "Renamed Writings".into();
    let verdict = h.run(&docs);
    assert_incremental(&verdict);
    assert!(!verdict.may_skip("writings/index.md"));
    assert!(!verdict.may_skip("writings/alpha/index.md"), "a breadcrumb descendant must refresh its trail");
    assert!(!verdict.may_skip("writings/beta/index.md"), "a breadcrumb descendant must refresh its trail");
    assert!(verdict.may_skip("other/index.md"), "an unrelated folder must not be swept in");
    assert!(verdict.may_skip("other/leaf/index.md"), "an unrelated folder's descendant must not be swept in");
}

/// The site-wide (not per-language) shape of the breadcrumb-ENABLE toggle:
/// flipping the homepage's `breadcrumb:` flag reaches every page's trail,
/// regardless of language, because `compute_breadcrumb_segments` always
/// reads the literal root `index.html` for this — see
/// `dependents::home_breadcrumb_globals`'s doc comment.
#[test]
fn a_homepage_breadcrumb_toggle_widens_the_whole_site() {
    let h = Harness::new();
    let docs = vault();
    h.run(&docs);

    let mut flipped = docs.clone();
    flipped[0].breadcrumb = Some(true);
    let verdict = h.run(&flipped);
    assert_incremental(&verdict);
    for src in ["index.md", "writings/index.md", "writings/alpha/index.md", "writings/beta/index.md"] {
        assert!(!verdict.may_skip(src), "{src} must refresh once breadcrumbs turn on site-wide");
    }
}

/// The series-siblings channel: a step's `weight` (reading-order position)
/// moves the whole chain's prev/next labels — every sibling in the SAME
/// series folder narrows in, an unrelated leaf elsewhere does not, and the
/// verdict stays narrowed rather than forcing `Full`.
#[test]
fn a_series_members_weight_edit_narrows_to_its_siblings() {
    let h = Harness::new();
    let mut docs = vault();
    let mut parent = doc("series/index.html", PageKind::Folder, "series body", Some("Series"));
    parent.series = Some(crate::build::types::SeriesField::Flag(true));
    let mut part1 = doc("series/part-1/index.html", PageKind::Article, "Part one.", Some("Part 1"));
    part1.weight = Some(1);
    let mut part2 = doc("series/part-2/index.html", PageKind::Article, "Part two.", Some("Part 2"));
    part2.weight = Some(2);
    docs.push(parent);
    docs.push(part1);
    docs.push(part2);
    h.run(&docs);

    let part1_idx = docs.iter().position(|d| d.url_path == "series/part-1/index.html").unwrap();
    docs[part1_idx].weight = Some(3);
    let verdict = h.run(&docs);
    assert_incremental(&verdict);
    assert!(!verdict.may_skip("series/part-1/index.md"));
    assert!(!verdict.may_skip("series/part-2/index.md"), "a series sibling must refresh its prev/next chrome");
    assert!(verdict.may_skip("writings/beta/index.md"), "a page outside the series must not be swept in");
}

/// The fail-safe boundary itself: a field this module does not model
/// (`sidebar`, alongside `translations`/`children_source`/`is_home_override`)
/// still forces `FullCause::SurfaceChanged` for the whole build, unchanged
/// from before this narrowing existed.
#[test]
fn an_unclassified_field_edit_still_forces_full() {
    let h = Harness::new();
    let docs = vault();
    h.run(&docs);

    let mut edited = docs.clone();
    edited[1].sidebar = Some("elsewhere".to_string());
    assert_full(&h.run(&edited), FullCause::SurfaceChanged);
}

/// The first edit after a cold start. A preview scan leaves an image's
/// dominant colour and LQIP empty and the background encoder fills them in, so
/// the next build sees them move with nothing about the site edited. That
/// used to be a `listing globals moved` full render — every page, on a real
/// site 244 of them, for the handful whose output actually changed. It must
/// now render exactly the pages whose previous output shows the image.
#[test]
fn an_image_gaining_its_placeholder_renders_only_the_pages_that_show_it() {
    let mut h = Harness::new();
    let cover = |color: Option<&str>| crate::types::content::MediaMetadata {
        path: "writings/alpha/cover.png".into(),
        file_type: "png".into(),
        dimensions: Some((1200, 800)),
        dominant_color: color.map(str::to_string),
        lqip_data_uri: color.map(|_| "data:image/jpeg;base64,AAAA".to_string()),
        ..Default::default()
    };
    let docs = vault();
    for d in &docs {
        let page = h.output.join(&d.url_path);
        std::fs::create_dir_all(page.parent().unwrap()).unwrap();
        let body = if d.url_path == "writings/index.html" {
            r#"<div class="moss-grid-card"><img src="/writings/alpha/cover.png" width="1200" height="800"></div>"#
        } else {
            "<p>no images</p>"
        };
        std::fs::write(page, body).unwrap();
    }
    h.project.image_files = vec![cover(None)];
    h.run(&docs);

    h.project.image_files = vec![cover(Some("#336699"))];
    let verdict = h.run(&docs);

    assert_incremental(&verdict);
    assert!(!verdict.may_skip("writings/index.md"), "the listing that shows the cover must pick up its placeholder");
    for carried in ["index.md", "writings/alpha/index.md", "writings/beta/index.md"] {
        assert!(verdict.may_skip(carried), "{carried} does not show the image");
    }
}
