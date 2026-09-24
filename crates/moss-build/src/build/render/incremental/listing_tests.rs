//! Listing-group tests.
//!
//! Three of the four correctness gates the design requires with Stage 2 live
//! here (the fourth, shadow verification, is an end-to-end switch), plus the
//! Stage-1b selector fixture. They are unit tests rather than end-to-end
//! builds because the claim under test is about the *digest* — whether a given
//! edit moves it — and a build can only observe the consequence one layer
//! later, where a dozen other gates could explain the same outcome.

use super::*;

fn project() -> ProjectStructure {
    ProjectStructure {
        root_path: "/tmp/moss-listing-tests".into(),
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

fn article(url: &str, body: &str) -> ParsedDocument {
    let stem = url.trim_end_matches("/index.html").rsplit('/').next().unwrap_or(url);
    ParsedDocument {
        url_path: url.to_string(),
        source_path: Some(format!("{}.md", url.trim_end_matches("/index.html"))),
        title: stem.to_string(),
        label: stem.to_string(),
        clean_stem: stem.to_string(),
        kind: PageKind::Article,
        content: body.to_string(),
        ..Default::default()
    }
}

fn folder(url: &str) -> ParsedDocument {
    let mut d = article(url, "");
    d.kind = PageKind::Folder;
    d
}

/// `writings/` with two articles, a subfolder that has its own child, a draft,
/// an `also_in` outsider from `notes/`, an `en/` language tree, and the root
/// home. Every rule the old inlined selector copy reimplemented is reachable
/// from this one fixture.
fn vault() -> Vec<ParsedDocument> {
    let mut draft = article("writings/wip/index.html", "unfinished");
    draft.draft = Some(true);

    let mut outsider = article("notes/guest/index.html", "a guest post");
    outsider.also_in = Some(vec!["writings".to_string()]);

    vec![
        folder("index.html"),
        folder("writings/index.html"),
        article("writings/alpha/index.html", "First para of alpha.\n\nNinth para."),
        article("writings/beta/index.html", "Beta lede.\n\nBeta tail."),
        draft,
        folder("writings/deep/index.html"),
        article("writings/deep/nested/index.html", "a grandchild"),
        outsider,
        article("en/hello/index.html", "hello"),
    ]
}

const ALPHA: usize = 2;
const DRAFT: usize = 4;

fn members(docs: &[ParsedDocument], slug: &str, flatten: bool) -> Vec<String> {
    crate::build::folder_embed::select_children_by_slug(slug, flatten, false, false, docs, &project())
        .iter()
        .map(|d| d.url_path.clone())
        .collect()
}

// ---- Stage 1b: one selector, no drift ------------------------------------

/// The synthetic folder-index loop in `render/blocking.rs` used to carry an
/// inlined second copy of this filter, which is why "one selector, therefore
/// no drift" was false and why the digest could not honour a rule the renderer
/// honours. It now calls the selector; this fixture pins every rule the copy
/// had to reimplement.
#[test]
fn the_selector_is_the_single_source_of_truth_for_membership() {
    let docs = vault();

    assert_eq!(
        members(&docs, "writings", false),
        vec![
            "writings/alpha/index.html",
            "writings/beta/index.html",
            "writings/deep/index.html",
            // `also_in` — a page declaring membership in a folder it does not
            // live under. This is the case that breaks any prefix-only model.
            "notes/guest/index.html",
        ],
        "direct depth: direct children and `also_in` outsiders; never the \
         draft, never a grandchild, never the folder's own index"
    );

    assert_eq!(
        members(&docs, "writings", true),
        vec![
            "writings/alpha/index.html",
            "writings/beta/index.html",
            // Folders are not listable at depth `all`; their descendants are.
            "writings/deep/nested/index.html",
            "notes/guest/index.html",
        ],
        "children_depth: all flattens descendants and still excludes the draft"
    );

    // The language-tree filter fires only when the homepage asks for it.
    let multilingual = ProjectStructure { has_language_trees: true, ..project() };
    let scoped = crate::build::folder_embed::select_children_by_slug(
        "", true, true, false, &docs, &multilingual,
    );
    assert!(
        !scoped.iter().any(|d| d.url_path.starts_with("en/")),
        "scope_default_tree drops language subtrees on a multilingual site"
    );
    let unscoped = crate::build::folder_embed::select_children_by_slug(
        "", true, false, false, &docs, &multilingual,
    );
    assert!(
        unscoped.iter().any(|d| d.url_path.starts_with("en/")),
        "...and only then"
    );
}

// ---- digest helpers ------------------------------------------------------

fn key(slug: &str) -> GroupKey {
    GroupKey {
        folder_slug: slug.to_string(),
        depth: Depth::Direct,
        scope_default_tree: false,
        exclude_nav: false,
    }
}

/// The key the root homepage actually reads: depth `all`, both homepage
/// filters on.
fn root_key() -> GroupKey {
    GroupKey {
        folder_slug: String::new(),
        depth: Depth::All,
        scope_default_tree: true,
        exclude_nav: true,
    }
}

fn digest(docs: &[ParsedDocument], key: &GroupKey) -> GroupDigest {
    ListingGroups::build(docs, &project(), false)
        .digest(key)
        .cloned()
        .unwrap_or_else(|| panic!("group {} was not built", key.id()))
}

// ---- Gate 1: excerpt liveness (FM-1), both directions --------------------

#[test]
fn a_first_paragraph_edit_moves_the_contents_digest_and_a_later_one_does_not() {
    let before = digest(&vault(), &key("writings"));

    let mut late_edit = vault();
    late_edit[ALPHA].content = "First para of alpha.\n\nA COMPLETELY DIFFERENT ninth para.".into();
    assert_eq!(
        digest(&late_edit, &key("writings")).contents,
        before.contents,
        "an edit below paragraph one cannot reach a card, so it must prune"
    );

    let mut lede_edit = vault();
    lede_edit[ALPHA].content = "A rewritten lede.\n\nNinth para.".into();
    assert_ne!(
        digest(&lede_edit, &key("writings")).contents,
        before.contents,
        "the card excerpt IS the first paragraph — this must re-render the host"
    );
}

#[test]
fn a_child_with_a_frontmatter_description_prunes_on_every_body_edit() {
    let mut described = vault();
    described[ALPHA].description = Some("Fixed blurb.".into());
    let before = digest(&described, &key("writings"));

    described[ALPHA].content = "Every single word of this body is new.".into();
    assert_eq!(
        digest(&described, &key("writings")).contents,
        before.contents,
        "resolve_page_description short-circuits on frontmatter, so the body is \
         invisible to the card — true of 136 of 223 files on the reference vault"
    );
}

// ---- Gate 2: the card-only-projection trap (§4a-ii) ----------------------

/// Every card field is byte-identical and the listing still re-orders and
/// loses its year headings. A projection built only from `ChildItemProps`
/// gets this wrong, and the symptom is a stale order that renders perfectly.
#[test]
fn a_child_gaining_a_weight_moves_the_plan_while_every_card_stays_identical() {
    let mut dated = vault();
    for d in dated.iter_mut() {
        d.date = Some("2025-01-01".into());
    }
    let before = digest(&dated, &key("writings"));

    let mut weighted = dated.clone();
    weighted[ALPHA].weight = Some(3);
    let after = digest(&weighted, &key("writings"));

    assert_ne!(
        after.plan, before.plan,
        "infer_axis flips Date -> Weight, which suppresses auto year grouping"
    );
}

/// The style half of the plan is body-derived: `resolve_children_config` calls
/// `resolve_page_description` to decide whether a listing is "rich". So the
/// plan moves on a body edit even where the resolved excerpt is not what the
/// chosen style renders.
#[test]
fn richness_is_a_plan_input_not_only_a_card_input() {
    let mut bare = vault();
    for d in bare.iter_mut() {
        d.content = String::new();
        d.date = Some("2025-01-01".into());
    }
    let before = digest(&bare, &key("writings"));

    bare[ALPHA].content = "Now there is a lede.".into();
    assert_ne!(
        digest(&bare, &key("writings")).plan,
        before.plan,
        "a listing that gains an excerpt switches auto style list -> summary"
    );
}

// ---- Gate 3: membership liveness ----------------------------------------

#[test]
fn add_delete_rename_and_undraft_all_move_the_membership_digest() {
    let base = digest(&vault(), &key("writings"));

    let mut added = vault();
    added.push(article("writings/gamma/index.html", "new"));
    assert_ne!(digest(&added, &key("writings")).membership, base.membership, "add");

    let mut deleted = vault();
    deleted.remove(ALPHA);
    assert_ne!(digest(&deleted, &key("writings")).membership, base.membership, "delete");

    let mut renamed = vault();
    renamed[ALPHA] = article("writings/alpha-renamed/index.html", "First para of alpha.");
    assert_ne!(digest(&renamed, &key("writings")).membership, base.membership, "rename");

    let mut undrafted = vault();
    undrafted[DRAFT].draft = Some(false);
    assert_ne!(digest(&undrafted, &key("writings")).membership, base.membership, "un-draft");
}

/// The root home lists the whole default tree (`children_depth: all`), so a
/// file added anywhere under it moves the ROOT group too — §4d's "the root
/// renders when a folder's contribution moves", reached without any
/// structural-change fallback.
#[test]
fn a_new_article_moves_the_root_groups_membership_as_well_as_its_folders() {
    let before = digest(&vault(), &root_key());
    let mut added = vault();
    added.push(article("writings/gamma/index.html", "new"));
    assert_ne!(digest(&added, &root_key()).membership, before.membership);
}

/// A folder card shows "N articles" and the folder's latest child date — both
/// corpus-derived. So a file added two levels down moves the *contents* of a
/// group whose *membership* did not move at all. Nothing in the subfolder's
/// own document changed; a projection that hashed only `ParsedDocument` fields
/// would miss this entirely.
#[test]
fn a_new_grandchild_moves_contents_through_the_subfolder_card_alone() {
    let before = digest(&vault(), &key("writings"));

    let mut added = vault();
    added.push(article("writings/deep/second/index.html", "another"));
    let after = digest(&added, &key("writings"));

    assert_eq!(
        before.membership, after.membership,
        "writings/'s own direct members did not change"
    );
    assert_ne!(
        before.contents, after.contents,
        "writings/deep's card count moved, and that card is part of the contents"
    );
}

// ---- FM-3: a non-deterministic Debug is silent in the expensive direction -

#[test]
fn digests_are_stable_across_two_identical_builds() {
    let docs = vault();
    let a = ListingGroups::build(&docs, &project(), false);
    let b = ListingGroups::build(&docs, &project(), false);
    assert!(a.len() > 1, "the fixture must produce more than one group");
    for k in [root_key(), key("writings"), key("deep")] {
        assert_eq!(a.digest(&k), b.digest(&k), "{}", k.id());
    }
}

// ---- FM-4: the globals bypass -------------------------------------------

#[test]
fn the_two_project_flags_that_feed_the_selector_are_listing_globals() {
    let empty = std::collections::HashMap::new();
    let base = listing_globals(&project(), &empty, crate::i18n::Language::En, None, false);
    let langs = listing_globals(
        &ProjectStructure { has_language_trees: true, ..project() },
        &empty,
        crate::i18n::Language::En,
        None,
        false,
    );
    let folders = listing_globals(
        &ProjectStructure { has_content_folders: false, ..project() },
        &empty,
        crate::i18n::Language::En,
        None,
        false,
    );
    assert_ne!(base, langs, "has_language_trees changes membership with no document changing");
    assert_ne!(base, folders, "has_content_folders feeds the exclude_nav filter");
}

// ---- The demoted predicate ----------------------------------------------

#[test]
fn hosts_are_mapped_to_the_groups_they_actually_read() {
    let docs = vault();

    let writings = docs.iter().find(|d| d.url_path == "writings/index.html").unwrap();
    let keys = groups_read_by(writings, &docs).expect("a folder index is a modelled shape");
    assert_eq!(keys, vec![key("writings")]);

    let home = docs.iter().find(|d| d.url_path == "index.html").unwrap();
    let keys = groups_read_by(home, &docs).expect("the root home is a modelled shape");
    assert_eq!(keys, vec![root_key()], "the homepage defaults depth to all and filters its own tree");

    assert!(!hosts_listing(&article("writings/alpha/index.html", "")), "a plain article hosts nothing");
}

/// An unrecognised host shape re-renders. Over-approximation is the only safe
/// default (rustc keeps `eval_always` for the same reason), and it is what
/// every host shape did before this model.
#[test]
fn an_unmodelled_host_shape_yields_no_groups_and_therefore_renders() {
    let mut override_home = article("en/mountain-home/index.html", "a language home override");
    override_home.is_home_override = true;
    assert!(hosts_listing(&override_home));
    assert!(
        groups_read_by(&override_home, &[override_home.clone()]).is_none(),
        "a home-override page whose url is not a folder index is not modelled"
    );

    let mut dangling = article("page/index.html", "");
    dangling.children_source = Some("[[NoSuchFolder]]".into());
    dangling.children_in = Some("sidebar".into());
    assert!(hosts_listing(&dangling));
    assert!(
        groups_read_by(&dangling, &[dangling.clone()]).is_none(),
        "an unresolvable sidebar target must render, never silently prune"
    );
}

// ---- Term claim pages (design: 2026-09-01-tags-and-authors-design.md) ----

#[test]
fn a_term_claim_page_hosts_the_terms_group_and_its_membership_moves_with_authorship() {
    let mut claim = article("about/ma/index.html", "bio");
    claim.term_listing = Some("authors/林小滿".to_string());
    assert!(hosts_listing(&claim));

    let mut docs = vault();
    let mut credited = article("writings/credited/index.html", "x");
    credited.also_in = Some(vec!["authors/林小滿".to_string()]);
    docs.push(credited);
    docs.push(claim.clone());

    let keys = groups_read_by(&claim, &docs).expect("term hosts are a modelled shape");
    assert!(keys.iter().any(|k| k.folder_slug == "authors/林小滿"));

    // Crediting one more article moves the group's membership digest — the
    // signal that re-renders the claim page.
    let before = digest(&docs, &key("authors/林小滿"));
    let mut more = docs.clone();
    let mut another = article("writings/another/index.html", "y");
    another.also_in = Some(vec!["authors/林小滿".to_string()]);
    more.push(another);
    assert_ne!(digest(&more, &key("authors/林小滿")).membership, before.membership);
}
