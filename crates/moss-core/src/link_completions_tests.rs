use super::*;
use LinkSyntax::*;

fn page(source: &str) -> Target {
    Target::Page { source: source.to_string(), title: String::new(), url: String::new() }
}

fn page_at(source: &str, title: &str, url: &str) -> Target {
    Target::Page { source: source.to_string(), title: title.to_string(), url: url.to_string() }
}

fn asset(source: &str) -> Target {
    Target::Asset { source: source.to_string() }
}

fn folder(source: &str) -> Target {
    Target::Folder { source: source.to_string() }
}

fn generated(url: &str, display: &str) -> Target {
    Target::Generated { url: url.to_string(), display: display.to_string() }
}

fn heading(text: &str) -> Target {
    Target::Heading { text: text.to_string(), slug: norm(text).replace(' ', "-"), level: 2 }
}

fn ctx<'a>(syntax: LinkSyntax, prefix: &'a str, from_rel: &'a str) -> InsertCtx<'a> {
    InsertCtx { syntax, prefix, from_rel }
}

fn rank<'a>(targets: &'a [Target], c: &InsertCtx<'_>) -> Vec<&'a Target> {
    rank_completions(targets, c).into_iter().map(|i| &targets[i]).collect()
}

#[test]
fn empty_prefix_returns_all_candidates() {
    let t = vec![page("about.md"), asset("photo.png")];
    let ranked = rank(&t, &ctx(Wikilink, "", ""));
    assert_eq!(ranked.len(), 2);
    // Link mode: page ranks before asset.
    assert_eq!(ranked[0], &t[0]);
    assert_eq!(ranked[1], &t[1]);
}

#[test]
fn prefix_filters_and_starts_with_ranks_first() {
    let t = vec![
        page("changelog.md"), // contains "ang" in middle
        page("angle.md"),     // starts with "ang"
        page("about.md"),     // no match
    ];
    let ranked = rank(&t, &ctx(Wikilink, "ang", ""));
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].label(), "angle");
    assert_eq!(ranked[1].label(), "changelog");
}

#[test]
fn case_insensitive_match() {
    let t = vec![page("README.md")];
    assert_eq!(rank(&t, &ctx(Wikilink, "read", "")).len(), 1);
}

#[test]
fn embed_ranks_assets_before_pages() {
    let t = vec![page("hero.md"), asset("hero.png")];
    let ranked = rank(&t, &ctx(Embed, "hero", ""));
    assert!(matches!(ranked[0], Target::Asset { .. }));
    let ranked2 = rank(&t, &ctx(Wikilink, "hero", ""));
    assert!(matches!(ranked2[0], Target::Page { .. }));
}

#[test]
fn cjk_prefix_matches() {
    let t = vec![page("刘果的笔记.md"), page("about.md")];
    let ranked = rank(&t, &ctx(Wikilink, "刘果", ""));
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].label(), "刘果的笔记");
}

#[test]
fn heading_candidates_rank_starts_with_before_contains() {
    let t = vec![
        heading("Background and context"),
        heading("Context"),
        heading("Conclusion"),
    ];
    let ranked = rank(&t, &ctx(Wikilink, "context", ""));
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].label(), "Context");
    assert_eq!(ranked[1].label(), "Background and context");
}

#[test]
fn embed_syntax_does_not_reorder_headings() {
    let t = vec![heading("bbbb"), heading("aaaa")];
    let with_embed = rank_completions(&t, &ctx(Embed, "", ""));
    let without = rank_completions(&t, &ctx(Wikilink, "", ""));
    assert_eq!(with_embed, without);
    assert_eq!(t[with_embed[0]].label(), "aaaa");
}

#[test]
fn nfc_and_nfd_forms_match_each_other() {
    let nfc = "caf\u{00e9}";
    let nfd = "cafe\u{0301}";
    assert_ne!(nfc, nfd, "precondition: the two byte-forms differ");
    let t = vec![page(&format!("{nfd}.md"))];
    assert_eq!(rank(&t, &ctx(Wikilink, nfc, "")).len(), 1);
    let t2 = vec![page(&format!("{nfc}.md"))];
    assert_eq!(rank(&t2, &ctx(Wikilink, nfd, "")).len(), 1);
}

// ── Language-tree + tree-proximity ranking (uses from_rel) ───────────

#[test]
fn same_language_tree_ranks_before_other_language() {
    let t = vec![page("en/guide.md"), page("zh-hans/guide.md")];
    let ranked = rank(&t, &ctx(Wikilink, "guide", "zh-hans/about.md"));
    assert_eq!(ranked[0], &t[1]);
}

#[test]
fn closer_in_tree_ranks_before_farther_in_same_language() {
    let t = vec![page("zh-hans/note.md"), page("zh-hans/游记/note.md")];
    let ranked = rank(&t, &ctx(Wikilink, "note", "zh-hans/游记/index.md"));
    assert_eq!(ranked[0], &t[1]);
}

#[test]
fn match_quality_outranks_language() {
    let t = vec![page("zh-hans/周报report.md"), page("en/report-en.md")];
    let ranked = rank(&t, &ctx(Wikilink, "report", "zh-hans/about.md"));
    assert_eq!(ranked[0], &t[1]);
}

#[test]
fn root_source_prefers_root_candidate_over_language_tree() {
    let t = vec![page("zh-hans/about.md"), page("about.md")];
    let ranked = rank(&t, &ctx(Wikilink, "about", "index.md"));
    assert_eq!(ranked[0], &t[1]);
}

#[test]
fn ties_break_by_path_deterministically() {
    let t = vec![page("zh-hans/b/guide.md"), page("zh-hans/a/guide.md")];
    let ranked = rank(&t, &ctx(Wikilink, "guide", "zh-hans/x.md"));
    assert_eq!(ranked[0], &t[1]);
}

// ── Path-qualified queries (a `/` in the prefix) ─────────────────────

#[test]
fn path_query_matches_across_an_omitted_directory() {
    // THE CORPUS BUG: `關於/頭像-李柏萱.png` written for a file that lives at
    // `關於/assets/頭像-李柏萱.png`. The filename-only matcher found nothing.
    let t = vec![asset("關於/assets/頭像-李柏萱.png")];
    assert_eq!(rank(&t, &ctx(Embed, "關於/頭像-李", "在場.md")).len(), 1);
}

#[test]
fn path_query_rejects_a_different_directory() {
    let t = vec![asset("關於/assets/頭像-李柏萱.png")];
    assert!(rank(&t, &ctx(Embed, "獎項/頭像-李", "在場.md")).is_empty());
}

#[test]
fn partial_directory_segment_still_lists_the_subtree() {
    let t = vec![asset("關於/assets/f99cc68b.png"), asset("關於/assembly.png")];
    let ranked = rank(&t, &ctx(Embed, "關於/ass", "在場.md"));
    assert_eq!(ranked.len(), 2);
    // `assembly.png` matches on the FILENAME; the subtree listing under
    // `assets/` matched only a directory.
    assert_eq!(ranked[0].label(), "assembly.png");
}

#[test]
fn bare_query_ordering_is_unchanged_by_path_keys() {
    // The neutrality proof for `seg_hit`/`dir_tight`/the last-segment `starts`.
    // MUST NOT BE DELETED — nothing else holds this.
    let t = vec![
        page("en/notes/changelog.md"),
        page("en/angle.md"),
        page("en/about.md"),
    ];
    let ranked = rank(&t, &ctx(Wikilink, "ang", "en/index.md"));
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].label(), "angle");
    assert_eq!(ranked[1].label(), "changelog");
}

#[test]
fn contiguous_suffix_dir_match_outranks_a_gapped_one() {
    let t = vec![asset("關於/deep/assets/x.png"), asset("assets/x.png")];
    let ranked = rank(&t, &ctx(Embed, "assets/x", "在場.md"));
    assert_eq!(ranked[0], &t[1]);
}

#[test]
fn starts_with_uses_the_final_segment_for_a_path_query() {
    let t = vec![page("dir/changelog.md"), page("dir/angle.md")];
    let ranked = rank(&t, &ctx(Wikilink, "dir/ang", "index.md"));
    assert_eq!(ranked[0].label(), "angle");
}

#[test]
fn heading_candidates_ignore_slashes_in_the_query() {
    // Heading TEXT may contain `/`, so path logic must not apply, and the
    // wikilink form inserts the text itself.
    let t = vec![heading("Intro/Setup")];
    let c = ctx(Wikilink, "Intro/Set", "notes.md");
    assert_eq!(rank(&t, &c).len(), 1);
    assert_eq!(insert_for(&t[0], &c), "Intro/Setup");
}

#[test]
fn trailing_slash_lists_only_that_directory() {
    let t = vec![asset("關於/assets/a.png"), asset("獎項/b.png")];
    let ranked = rank(&t, &ctx(Embed, "關於/", "在場.md"));
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0], &t[0]);
}

// ── Folders: an offer to descend ─────────────────────────────────────

#[test]
fn a_folder_lists_under_a_trailing_slash_but_never_itself() {
    let t = vec![folder("關於"), folder("關於/assets"), asset("關於/near.png")];
    let ranked = rank(&t, &ctx(Embed, "關於/", "在場.md"));
    assert_eq!(ranked.len(), 2, "the folder the author is already inside is not offered");
    // The file matched the same segment as the subfolder; files first.
    assert_eq!(ranked[0], &t[2]);
    assert_eq!(ranked[1], &t[1]);
}

#[test]
fn a_folder_inserts_its_path_with_a_trailing_slash() {
    let c = ctx(Inline, "ab", "index.md");
    assert_eq!(insert_for(&folder("about"), &c), "about/");
    assert_eq!(folder("about").label(), "about/");
}

#[test]
fn folders_are_never_offered_in_url_space() {
    let t = vec![folder("about"), page_at("about/index.md", "About", "/about/")];
    let ranked = rank(&t, &ctx(Inline, "/ab", "index.md"));
    assert_eq!(ranked.len(), 1);
    assert!(matches!(ranked[0], Target::Page { .. }));
}

// ── Names: title and url slug are searchable ─────────────────────────

#[test]
fn a_page_is_found_by_its_title_and_its_url_slug() {
    let t = vec![page_at("隐私.md", "隐私政策", "/privacy/")];
    assert_eq!(rank(&t, &ctx(Wikilink, "privacy", "index.md")).len(), 1);
    assert_eq!(rank(&t, &ctx(Wikilink, "政策", "index.md")).len(), 1);
    assert_eq!(rank(&t, &ctx(Wikilink, "隐私", "index.md")).len(), 1);
    assert!(rank(&t, &ctx(Wikilink, "cookies", "index.md")).is_empty());
}

#[test]
fn the_label_is_the_title_when_there_is_one() {
    assert_eq!(page_at("隐私.md", "隐私政策", "/privacy/").label(), "隐私政策");
    assert_eq!(page("隐私.md").label(), "隐私");
}

// ── The two address spaces ───────────────────────────────────────────

#[test]
fn a_leading_slash_in_an_inline_link_is_url_space_and_nothing_else_is() {
    assert!(ctx(Inline, "/ab", "").url_space());
    assert!(!ctx(Inline, "ab", "").url_space());
    assert!(!ctx(Wikilink, "/ab", "").url_space());
    assert!(!ctx(Embed, "/ab", "").url_space());
    assert!(!ctx(AssetPath, "/ab", "").url_space());
}

#[test]
fn url_space_writes_the_published_url_and_matches_its_segments() {
    let t = vec![page_at("隐私.md", "隐私政策", "/privacy/"), page_at("docs/guide.md", "Guide", "/docs/guide/")];
    let c = ctx(Inline, "/priv", "index.md");
    let ranked = rank(&t, &c);
    assert_eq!(ranked.len(), 1);
    assert_eq!(insert_for(ranked[0], &c), "/privacy/");
    // Path-qualified in URL space matches URL components, not source ones.
    let c2 = ctx(Inline, "/docs/gu", "index.md");
    let ranked2 = rank(&t, &c2);
    assert_eq!(ranked2.len(), 1);
    assert_eq!(insert_for(ranked2[0], &c2), "/docs/guide/");
}

#[test]
fn a_page_without_a_recorded_url_is_not_offered_in_url_space() {
    let t = vec![page("draft.md")];
    assert!(rank(&t, &ctx(Inline, "/dr", "index.md")).is_empty());
    assert_eq!(rank(&t, &ctx(Inline, "dr", "index.md")).len(), 1);
}

#[test]
fn generated_pages_exist_only_in_url_space() {
    let t = vec![generated("/tags/design/", "design"), page_at("design.md", "Design", "/design/")];
    let url = rank(&t, &ctx(Inline, "/des", "index.md"));
    assert_eq!(url.len(), 2);
    assert!(matches!(url[0], Target::Page { .. }), "authored pages rank before generated ones");
    assert_eq!(insert_for(url[1], &ctx(Inline, "/des", "index.md")), "/tags/design/");
    for c in [ctx(Inline, "des", "index.md"), ctx(Wikilink, "des", "index.md"), ctx(Embed, "des", "index.md")] {
        let r = rank(&t, &c);
        assert!(r.iter().all(|x| !matches!(x, Target::Generated { .. })), "{c:?}");
    }
}

#[test]
fn url_space_roots_an_asset_at_the_project_root() {
    let c = ctx(Inline, "/assets/h", "關於/x.md");
    assert_eq!(insert_for(&asset("assets/hero.png"), &c), "/assets/hero.png");
}

#[test]
fn asset_path_syntax_never_offers_pages() {
    let t = vec![page("hero.md"), asset("hero.png")];
    let ranked = rank(&t, &ctx(AssetPath, "hero", "index.md"));
    assert_eq!(ranked.len(), 1);
    assert!(matches!(ranked[0], Target::Asset { .. }));
}

// ── The insert_for invariant, enforced through the REAL resolvers ────

#[test]
fn insert_for_always_round_trips_through_the_resolver() {
    use crate::resolve::asset_class::{
        resolve_asset_ref, AssetProvenance, AssetResolution, FakeAssetIndex,
    };

    // The corpus shape: a root-level source and a nested one, against a sibling
    // asset, a subtree asset, a cross-tree asset and a root-level asset. The
    // cross-tree row is the collision case — `assets/首頁hero.png` seen from
    // `關於/x.md`, where the bare root-relative form would resolve to the
    // DIFFERENT, also-existing `關於/assets/首頁hero.png`.
    let paths = [
        "關於/assets/頭像-李柏萱.png",
        "關於/歷季得獎者.md",
        "關於/近照.png",
        "assets/首頁hero.png",
        "關於/assets/首頁hero.png",
        "首頁.png",
    ];
    let idx = FakeAssetIndex::new(&paths);

    for from_rel in ["在場.md", "關於/歷季得獎者.md"] {
        for rel in [
            "關於/assets/頭像-李柏萱.png",
            "關於/近照.png",
            "assets/首頁hero.png",
            "首頁.png",
        ] {
            // Path-qualified embed AND source-space inline: both exact forms.
            for c in [ctx(Embed, "關於/x", from_rel), ctx(Inline, "x", from_rel)] {
                let emitted = insert_for(&asset(rel), &c);
                assert_eq!(
                    resolve_asset_ref(&emitted, from_rel, &idx),
                    AssetResolution::Resolved {
                        root_rel: rel.to_string(),
                        provenance: AssetProvenance::Literal,
                    },
                    "from {from_rel}, candidate {rel}, emitted {emitted}"
                );
            }
        }
    }

    // The page leg, through the resolver pages actually use.
    let mut b = crate::content_graph::ContentGraphBuilder::new();
    b.add_file("notes/ideas.md", "notes/ideas");
    b.add_file("關於/歷季得獎者.md", "關於/歷季得獎者");
    let graph = b.build();
    for from_rel in ["在場.md", "關於/歷季得獎者.md"] {
        for c in [ctx(Wikilink, "notes/id", from_rel), ctx(Inline, "id", from_rel)] {
            let emitted = insert_for(&page("notes/ideas.md"), &c);
            assert_eq!(emitted, "notes/ideas");
            assert_eq!(graph.resolve_path(&emitted, from_rel).as_deref(), Some("notes/ideas.md"));
        }
    }
}

#[test]
fn bare_wikilink_queries_keep_the_obsidian_forms() {
    // The chip-bar contract: a bare query in a wikilink/embed/asset-path
    // context writes the bare filename, so the frontmatter cover picker keeps
    // writing `[[x.png]]`.
    let a = asset("關於/assets/x.png");
    assert_eq!(insert_for(&a, &ctx(Embed, "x", "關於/y.md")), "x.png");
    assert_eq!(insert_for(&a, &ctx(AssetPath, "x", "關於/y.md")), "x.png");
    assert_eq!(insert_for(&page("notes/ideas.md"), &ctx(Wikilink, "id", "index.md")), "ideas");
}

#[test]
fn a_heading_inserts_its_text_in_a_wikilink_and_its_slug_in_an_inline_link() {
    let h = heading("Big Idea");
    assert_eq!(insert_for(&h, &ctx(Wikilink, "", "a.md")), "Big Idea");
    assert_eq!(insert_for(&h, &ctx(Inline, "", "a.md")), "big-idea");
}

// ── asset_ref_relative: the reference form for a PICKED file ──────────

#[test]
fn asset_ref_relative_prefers_the_source_relative_form() {
    assert_eq!(asset_ref_relative("關於/x.md", "關於/img/cover.png"), "img/cover.png");
}

#[test]
fn asset_ref_relative_roots_a_path_outside_the_page_subtree() {
    assert_eq!(asset_ref_relative("關於/x.md", "photos/cover.png"), "/photos/cover.png");
}

#[test]
fn asset_ref_relative_never_reads_a_name_prefix_as_a_parent() {
    assert_eq!(asset_ref_relative("關於/x.md", "關於2/cover.png"), "/關於2/cover.png");
}

#[test]
fn asset_ref_relative_from_a_root_page_is_root_relative() {
    assert_eq!(asset_ref_relative("index.md", "img/cover.png"), "img/cover.png");
}

#[test]
fn asset_ref_relative_normalizes_windows_separators() {
    assert_eq!(asset_ref_relative("關於\\x.md", "關於\\img\\cover.png"), "img/cover.png");
}
