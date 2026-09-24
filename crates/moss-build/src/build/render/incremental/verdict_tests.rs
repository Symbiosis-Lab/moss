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
    project: ProjectStructure,
    overrides: std::collections::HashMap<String, String>,
    skip: bool,
    globals_salt: bool,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("dep-cache.json");
        Self {
            _dir: dir,
            cache,
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

/// The Stage-0 baseline on `harbor/潮汐`: a no-op save re-renders 114 of
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

    // SurfaceChanged — a cross-page-visible field moved.
    let h = Harness::new();
    h.run(&vault());
    let mut relabelled = vault();
    relabelled[2].label = "Renamed".into();
    assert_full(&h.run(&relabelled), FullCause::SurfaceChanged);

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
/// editing the homepage was a full render — 223 pages on harbor. The only
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
