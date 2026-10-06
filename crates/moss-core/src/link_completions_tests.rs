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
    let t = vec![page("山居的笔记.md"), page("about.md")];
    let ranked = rank(&t, &ctx(Wikilink, "山居", ""));
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].label(), "山居的笔记");
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
    // THE CORPUS BUG: `關於/頭像-李知安.png` written for a file that lives at
    // `關於/assets/頭像-李知安.png`. The filename-only matcher found nothing.
    let t = vec![asset("關於/assets/頭像-李知安.png")];
    assert_eq!(rank(&t, &ctx(Embed, "關於/頭像-李", "河灣.md")).len(), 1);
}

#[test]
fn path_query_rejects_a_different_directory() {
    let t = vec![asset("關於/assets/頭像-李知安.png")];
    assert!(rank(&t, &ctx(Embed, "評選/頭像-李", "河灣.md")).is_empty());
}

#[test]
fn partial_directory_segment_still_lists_the_subtree() {
    let t = vec![asset("關於/assets/f99cc68b.png"), asset("關於/assembly.png")];
    let ranked = rank(&t, &ctx(Embed, "關於/ass", "河灣.md"));
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
    let ranked = rank(&t, &ctx(Embed, "assets/x", "河灣.md"));
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
    let t = vec![asset("關於/assets/a.png"), asset("評選/b.png")];
    let ranked = rank(&t, &ctx(Embed, "關於/", "河灣.md"));
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0], &t[0]);
}

// ── Folders: an offer to descend ─────────────────────────────────────

#[test]
fn a_folder_lists_under_a_trailing_slash_but_never_itself() {
    let t = vec![folder("關於"), folder("關於/assets"), asset("關於/near.png")];
    let ranked = rank(&t, &ctx(Embed, "關於/", "河灣.md"));
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
    use crate::resolve::asset_class::{resolve_file_target, AssetProvenance, AssetResolution};

    // The corpus shape: a root-level source and a nested one, against a sibling
    // asset, a subtree asset, a cross-tree asset and a root-level asset. The
    // cross-tree row is the collision case — `assets/首頁hero.png` seen from
    // `關於/x.md`, where the bare root-relative form would resolve to the
    // DIFFERENT, also-existing `關於/assets/首頁hero.png`.
    let paths = [
        "關於/assets/頭像-李知安.png",
        "關於/歷季得獎者.md",
        "關於/近照.png",
        "assets/首頁hero.png",
        "關於/assets/首頁hero.png",
        "首頁.png",
    ];
    let idx = crate::content_graph::ContentGraph::from_paths(&paths);

    for from_rel in ["河灣.md", "關於/歷季得獎者.md"] {
        for rel in [
            "關於/assets/頭像-李知安.png",
            "關於/近照.png",
            "assets/首頁hero.png",
            "首頁.png",
        ] {
            // Path-qualified embed AND source-space inline: both exact forms.
            for c in [ctx(Embed, "關於/x", from_rel), ctx(Inline, "x", from_rel)] {
                let emitted = insert_for(&asset(rel), &c);
                assert_eq!(
                    resolve_file_target(&emitted, from_rel, &idx),
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
    for from_rel in ["河灣.md", "關於/歷季得獎者.md"] {
        for (c, want) in [
            (ctx(Wikilink, "notes/id", from_rel), "notes/ideas"),
            (ctx(Inline, "id", from_rel), if from_rel == "河灣.md" { "notes/ideas.md" } else { "../notes/ideas.md" }),
        ] {
            let emitted = insert_for(&page("notes/ideas.md"), &c);
            assert_eq!(emitted, want);
            assert_eq!(graph.resolve_path(&emitted, from_rel).as_deref(), Some("notes/ideas.md"));
        }
    }
}

#[test]
fn a_picked_file_resolves_to_itself_although_a_copy_sits_beneath_the_page() {
    use crate::resolve::asset_class::{resolve_file_target, AssetProvenance, AssetResolution};

    let idx = crate::content_graph::ContentGraph::from_paths(&[
        "a/b/page.md", "a/c/p.png", "a/b/c/p.png", "a/c/my photo.jpg", "a/b/c/my photo.jpg",
    ]);
    for rel in ["a/c/p.png", "a/b/c/p.png", "a/c/my photo.jpg", "a/b/c/my photo.jpg"] {
        for c in [ctx(Inline, "p", "a/b/page.md"), ctx(Embed, "c/p", "a/b/page.md")] {
            let emitted = insert_for(&asset(rel), &c);
            match resolve_file_target(&emitted, "a/b/page.md", &idx) {
                AssetResolution::Resolved { root_rel, .. } => assert_eq!(root_rel, rel, "emitted {emitted}"),
                other => panic!("{emitted} from a/b/page.md gave {other:?}, wanted {rel}"),
            }
        }
    }
    assert_eq!(insert_for(&asset("a/c/p.png"), &ctx(Inline, "p", "a/b/page.md")), "../c/p.png");
    assert_eq!(
        resolve_file_target("../c/p.png", "a/b/page.md", &idx),
        AssetResolution::Resolved { root_rel: "a/c/p.png".into(), provenance: AssetProvenance::Literal },
    );
}

#[test]
fn a_folder_with_a_space_inserts_a_valid_destination_that_still_resolves() {
    let mut b = crate::content_graph::ContentGraphBuilder::new();
    b.add_file("my folder/index.md", "my-folder");
    b.add_file("a/b/page.md", "a/b/page");
    let graph = b.build();
    let f = folder("my folder");
    for from_rel in ["page.md", "a/b/page.md"] {
        let emitted = insert_for(&f, &ctx(Inline, "my", from_rel));
        assert_eq!(emitted, "my%20folder/");
        assert_eq!(
            crate::resolve::fuzzy_path::resolve_reference(&emitted, &graph, from_rel),
            crate::resolve::fuzzy_path::ResolvedRef::Found("my folder/index.md".into()),
            "from {from_rel}"
        );
    }
    // The wiki form is not a destination: the name stays as written.
    assert_eq!(insert_for(&f, &ctx(Wikilink, "my", "page.md")), "my folder/");
    // And the query the author edits afterwards still reopens inside it.
    assert_eq!(rank(&[asset("my folder/x.png")], &ctx(Inline, "my%20folder/", "page.md")).len(), 1);
}

#[test]
fn bare_wikilink_queries_keep_the_obsidian_forms() {
    // The chip-bar contract: a bare query in a wikilink/embed context writes
    // the bare filename, so the frontmatter cover picker keeps writing `[[x.png]]`.
    let a = asset("關於/assets/x.png");
    assert_eq!(insert_for(&a, &ctx(Embed, "x", "關於/y.md")), "x.png");
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

// ── Standard forms write a real relative path ────────────────────────

/// Every file of the fixture site. Same-named decoys sit in other folders so a
/// destination that only names the file, and leans on name search, picks the
/// wrong one or is ambiguous.
const SITE: &[&str] = &[
    "a/page.md",
    "a/photo.jpg",
    "a/my photo.jpg",
    "a/photos/photo.jpg",
    "b/photo.jpg",
    "b/my photo.jpg",
    "c/other.jpg",
    "d/other.jpg",
    "c/my photo.jpg",
    "c/頭像.png",
    "d/頭像.png",
    "c/x #1 (a).png",
    "d/x #1 (a).png",
    "a/v:1.png",
    "n/alpha.md",
    "m/alpha.md",
];

/// What the build resolves `dest` to when written as `[t](dest)` or `![t](dest)`
/// on the page at `from`: the real parser and `resolve_urls`.
fn build_resolves(image: bool, dest: &str, from: &str) -> Option<String> {
    let mut b = crate::content_graph::ContentGraphBuilder::new();
    for p in SITE {
        b.add_file(p, p);
    }
    let graph = b.build();
    let src = format!("{}[t]({dest})", if image { "!" } else { "" });
    let mut doc = crate::ast::parser::parse(&src);
    let out = crate::ast::resolve_urls::resolve_urls(&mut doc, &graph, from);
    match out.outgoing.as_slice() {
        [one] => Some(one.target_path.clone()),
        _ => None,
    }
}

const FROM: &str = "a/page.md";

/// (file, expected insert) for a file seen from `a/page.md`.
const CASES: &[(&str, &str)] = &[
    ("a/photo.jpg", "photo.jpg"),
    ("a/photos/photo.jpg", "photos/photo.jpg"),
    ("c/other.jpg", "../c/other.jpg"),
    ("a/my photo.jpg", "my%20photo.jpg"),
    ("c/my photo.jpg", "../c/my%20photo.jpg"),
    ("c/頭像.png", "../c/頭像.png"),
    ("c/x #1 (a).png", "../c/x%20%231%20(a).png"),
    ("a/v:1.png", "./v:1.png"),
];

#[test]
fn a_file_is_inserted_as_the_encoded_path_from_the_page_folder() {
    let mut wrong = Vec::new();
    for syntax in [Inline, AssetPath] {
        for (file, want) in CASES {
            let got = insert_for(&asset(file), &ctx(syntax, "", FROM));
            if got != *want {
                wrong.push(format!("{syntax:?} {file}: got {got:?}, want {want:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_typed_name_or_path_gives_the_same_relative_path() {
    for typed in ["ph", "photos/ph", "c/oth", "my%20ph"] {
        for syntax in [Inline, AssetPath] {
            let got = insert_for(&asset("c/other.jpg"), &ctx(syntax, typed, FROM));
            assert_eq!(got, "../c/other.jpg", "{syntax:?} typed {typed:?}");
        }
    }
}

#[test]
fn every_inserted_text_resolves_to_the_chosen_file_in_the_build() {
    let mut wrong = Vec::new();
    for (syntax, image) in [(Inline, false), (AssetPath, true)] {
        for (file, _) in CASES {
            let dest = insert_for(&asset(file), &ctx(syntax, "", FROM));
            let got = build_resolves(image, &dest, FROM);
            if got.as_deref() != Some(*file) {
                wrong.push(format!("{syntax:?} {file}: inserted {dest:?} resolves to {got:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_folder_named_like_a_language_round_trips_from_an_ordinary_page() {
    let mut b = crate::content_graph::ContentGraphBuilder::new();
    for p in ["trips/index.md", "trips/uk/day1.jpg", "trips/2023/uk/day1.jpg"] {
        b.add_file(p, p);
    }
    let graph = b.build();
    for file in ["trips/uk/day1.jpg", "trips/2023/uk/day1.jpg"] {
        let dest = insert_for(&asset(file), &ctx(AssetPath, "", "trips/index.md"));
        let mut doc = crate::ast::parser::parse(&format!("![t]({dest})"));
        let out = crate::ast::resolve_urls::resolve_urls(&mut doc, &graph, "trips/index.md");
        let got: Vec<&str> = out.outgoing.iter().map(|o| o.target_path.as_str()).collect();
        assert_eq!(got, [file], "inserted {dest:?}");
    }
}

#[test]
fn a_page_is_linked_by_its_relative_source_path_with_extension() {
    let c = ctx(Inline, "al", FROM);
    for (source, want) in [("n/alpha.md", "../n/alpha.md"), ("m/alpha.md", "../m/alpha.md")] {
        let dest = insert_for(&page_at(source, "Alpha", "/alpha/"), &c);
        assert_eq!(dest, want);
        assert_eq!(build_resolves(false, &dest, FROM).as_deref(), Some(source), "{dest}");
    }
    // Same folder: just the file name, still with its extension.
    assert_eq!(insert_for(&page("a/page.md"), &ctx(Inline, "pa", "a/other.md")), "page.md");
}

#[test]
fn a_leading_slash_keeps_published_addresses_and_root_paths() {
    let c = ctx(Inline, "/", FROM);
    assert_eq!(insert_for(&page_at("n/alpha.md", "Alpha", "/alpha/"), &c), "/alpha/");
    assert_eq!(insert_for(&asset("c/other.jpg"), &c), "/c/other.jpg");
}

#[test]
fn wiki_contexts_keep_their_forms() {
    let a = asset("c/my photo.jpg");
    assert_eq!(insert_for(&a, &ctx(Wikilink, "my", FROM)), "my photo.jpg");
    assert_eq!(insert_for(&a, &ctx(Embed, "my", FROM)), "my photo.jpg");
    assert_eq!(insert_for(&page("n/alpha.md"), &ctx(Wikilink, "al", FROM)), "alpha");
}

// ── Search ───────────────────────────────────────────────────────────

#[test]
fn spaces_are_found_whether_typed_raw_or_as_percent_20() {
    let t = vec![asset("c/my photo.jpg"), asset("c/other.jpg")];
    for typed in ["my ph", "my%20ph", "c/my%20ph", "c/my ph"] {
        for syntax in [Inline, AssetPath, Embed] {
            let ranked = rank(&t, &ctx(syntax, typed, FROM));
            assert_eq!(ranked, vec![&t[0]], "{syntax:?} typed {typed:?}");
        }
    }
}

#[test]
fn a_partial_percent_escape_while_typing_does_not_hide_everything() {
    let t = vec![asset("c/my photo.jpg"), asset("c/other.jpg")];
    for typed in ["my", "my%", "my%2", "my%20"] {
        assert_eq!(rank(&t, &ctx(Inline, typed, FROM)), vec![&t[0]], "typed {typed:?}");
    }
}

// ── Order: nearer to the page first among equal matches ──────────────

#[test]
fn equal_matches_come_nearest_the_page_first_then_alphabetical() {
    let t = vec![
        asset("photo.png"),
        asset("c/photo.png"),
        asset("a/sub/photo.png"),
        asset("a/aaa/photo.png"),
        asset("a/photo.png"),
        asset("a/sub/deep/photo.png"),
    ];
    let order: Vec<String> = rank(&t, &ctx(Inline, "photo", FROM))
        .into_iter()
        .map(|x| match x {
            Target::Asset { source } => source.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(
        order,
        ["a/photo.png", "a/aaa/photo.png", "a/sub/deep/photo.png", "a/sub/photo.png", "c/photo.png", "photo.png"]
    );
}

#[test]
fn a_better_match_still_beats_a_nearer_one() {
    let t = vec![asset("a/xx-photo.png"), asset("c/photo.png")];
    let ranked = rank(&t, &ctx(Inline, "photo", FROM));
    assert_eq!(ranked[0], &t[1], "a name starting with the query outranks one merely containing it");
}
