//! `children_listed: here` keeps a folder's pages out of the listings above it,
//! and `![[./]]` places a page's own listing where the body puts it.

#[path = "support/site.rs"]
mod site;

use site::Site;

fn articles() -> Vec<(&'static str, &'static str)> {
    vec![
        ("writing/One.md", "---\ntitle: Article One\ndate: 2026-01-01\n---\n\nText.\n"),
        ("writing/Two.md", "---\ntitle: Article Two\ndate: 2026-01-02\n---\n\nText.\n"),
    ]
}

fn site_with(home: &str, posts_home: &str) -> Site {
    let mut files = articles();
    files.push(("index.md", home));
    files.push(("posts/index.md", posts_home));
    files.push(("posts/20260101-0900.md", "---\ntitle: Alpha Post\n---\n\nShort post.\n"));
    files.push(("posts/20260102-0900.md", "---\ntitle: Beta Post\n---\n\nShort post.\n"));
    files.push(("posts/nested/20260103-0900.md", "---\ntitle: Gamma Post\n---\n\nShort post.\n"));
    Site::new(&files)
}

const HOME: &str = "---\ntitle: Home\n---\n\nHello.\n";

#[test]
fn a_here_folder_stays_out_of_the_home_and_keeps_its_own_listing() {
    let site = site_with(HOME, "---\ntitle: Posts\nchildren_listed: here\n---\n\nPosts.\n");
    let home = site.html("index.html");
    assert!(home.contains("Article One"), "{home}");
    assert!(!home.contains("Alpha Post") && !home.contains("Gamma Post"), "{home}");
    let posts = site.html("posts/index.html");
    assert!(posts.contains("Alpha Post") && posts.contains("Beta Post"), "{posts}");
}

#[test]
fn without_the_field_the_home_lists_the_folder() {
    let site = site_with(HOME, "---\ntitle: Posts\n---\n\nPosts.\n");
    let home = site.html("index.html");
    assert!(home.contains("Alpha Post") && home.contains("Gamma Post"), "{home}");
}

#[test]
fn a_here_folder_stays_out_of_an_ancestor_archive_but_its_own_page_is_a_direct_card() {
    let mut files = articles();
    files.push(("index.md", HOME));
    files.push(("Archive.md", "---\ntitle: Archive\nchildren: \"[[index]]\"\nchildren_depth: all\n---\n\nAll.\n"));
    files.push(("posts/index.md", "---\ntitle: Posts\nchildren_listed: here\n---\n\nPosts.\n"));
    files.push(("posts/20260101-0900.md", "---\ntitle: Alpha Post\n---\n\nShort post.\n"));
    files.push(("Direct.md", "---\ntitle: Direct\nchildren: \"[[index]]\"\nchildren_depth: direct\n---\n\nD.\n"));
    let site = Site::new(&files);
    let archive = site.html("archive/index.html");
    assert!(archive.contains("Article One"), "{archive}");
    assert!(!archive.contains("Alpha Post"), "{archive}");
    let direct = site.html("direct/index.html");
    assert!(listing_of(&direct).contains("href=\"/posts/\""), "{direct}");
}

/// The listing markup of a page, without the navigation around it.
fn listing_of(html: &str) -> &str {
    let from = html.find("moss-cards-container").unwrap_or_else(|| panic!("no listing in {html}"));
    &html[from..html[from..].find("</article>").map_or(html.len(), |i| from + i)]
}

fn order(html: &str, needles: &[&str]) -> Vec<usize> {
    needles.iter().map(|n| html.find(n).unwrap_or_else(|| panic!("{n} missing in {html}"))).collect()
}

#[test]
fn own_listing_marker_places_the_listing_between_paragraphs() {
    let home = "---\ntitle: Home\n---\n\nBefore text.\n\n![[./]]\n\nAfter text.\n";
    let site = site_with(home, "---\ntitle: Posts\nchildren_listed: here\n---\n\nPosts.\n");
    let html = site.html("index.html");
    let at = order(&html, &["Before text.", "Article One", "After text."]);
    assert!(at[0] < at[1] && at[1] < at[2], "{html}");
    assert!(!html.contains("MOSS_OWN_LISTING"), "{html}");
    assert!(site.notes.iter().all(|n| !n.contains("listed both")), "{:?}", site.notes);
}

#[test]
fn without_the_marker_the_listing_follows_the_body() {
    let home = "---\ntitle: Home\n---\n\nBefore text.\n\nAfter text.\n";
    let site = site_with(home, "---\ntitle: Posts\nchildren_listed: here\n---\n\nPosts.\n");
    let html = site.html("index.html");
    let at = order(&html, &["Before text.", "After text.", "Article One"]);
    assert!(at[0] < at[1] && at[1] < at[2], "{html}");
}

#[test]
fn with_children_false_the_marker_is_a_folder_embed() {
    let home = "---\ntitle: Home\nchildren: false\n---\n\nBefore text.\n\n![[./]]\n\nAfter text.\n";
    let site = site_with(home, "---\ntitle: Posts\n---\n\nPosts.\n");
    let html = site.html("index.html");
    assert!(html.contains("data-embed"), "{html}");
    assert!(!html.contains("MOSS_OWN_LISTING"), "{html}");
}

#[test]
fn the_home_recipe_lists_articles_then_the_newest_post() {
    let home = "---\ntitle: Home\nchildren_limit: 4\n---\n\nHello.\n\n![[./]]\n\n## Latest posts\n\n![[posts/|limit:1]]\n";
    let site = site_with(home, "---\ntitle: Posts\nchildren_listed: here\n---\n\nPosts.\n");
    let html = site.html("index.html");
    let at = order(&html, &["Hello.", "Article One", "Latest posts", "href=\"/posts/2026"]);
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{html}");
    assert_eq!(html.matches("href=\"/posts/2026").count(), 1, "{html}");
}

#[test]
fn an_article_setting_the_field_does_not_hide_itself() {
    let mut files = articles();
    files.push(("index.md", HOME));
    files.push(("writing/Odd.md", "---\ntitle: Odd Article\nchildren_listed: here\ndate: 2026-02-01\n---\n\nText.\n"));
    let site = Site::new(&files);
    assert!(listing_of(&site.html("index.html")).contains("Odd Article"));
}

#[test]
fn a_page_under_a_here_folder_naming_the_ancestor_in_also_in_is_still_listed() {
    let mut files = articles();
    files.push(("index.md", HOME));
    files.push(("posts/index.md", "---\ntitle: Posts\nchildren_listed: here\n---\n\nPosts.\n"));
    files.push(("posts/Pinned.md", "---\ntitle: Pinned Post\nalso_in: [\"\"]\ndate: 2026-02-01\n---\n\nText.\n"));
    files.push(("posts/Plain.md", "---\ntitle: Plain Post\ndate: 2026-02-02\n---\n\nText.\n"));
    let site = Site::new(&files);
    let home = site.html("index.html");
    assert!(listing_of(&home).contains("Pinned Post") && !home.contains("Plain Post"), "{home}");
}

fn own_listing_site(home_body: &str) -> Site {
    let home = format!("---\ntitle: Home\n---\n\n{home_body}\n");
    let mut files = articles();
    files.push(("index.md", home.leak()));
    files.push(("Direct.md", "---\ntitle: Direct Page\ndate: 2026-03-01\n---\n\nText.\n"));
    Site::new(&files)
}

fn card_count(html: &str) -> usize {
    html.matches("class=\"moss-card\"").count()
}

#[test]
fn a_bare_self_embed_in_any_spelling_places_the_listing() {
    for spelling in ["![[./]]", "![[/]]"] {
        let site = own_listing_site(&format!("Before text.\n\n{spelling}\n\nAfter text."));
        let html = site.html("index.html");
        let at = order(&html, &["Before text.", "Article One", "After text."]);
        assert!(at[0] < at[1] && at[1] < at[2], "{spelling}: {html}");
        assert_eq!(card_count(&html), 2, "{spelling}: one listing, not an embed and a listing\n{html}");
    }
}

#[test]
fn a_folder_note_embedding_its_own_folder_by_name_places_its_listing() {
    let files = [
        ("index.md", "---\ntitle: Home\nchildren: false\n---\n\nHi.\n"),
        ("Opera/Opera.md", "---\ntitle: Opera\n---\n\nBefore text.\n\n![[Opera/]]\n\nAfter text.\n"),
        ("Opera/One.md", "---\ntitle: Act One\ndate: 2026-01-01\n---\n\nText.\n"),
    ];
    let html = Site::new(&files).html("opera/index.html");
    let at = order(&html, &["Before text.", "Act One", "After text."]);
    assert!(at[0] < at[1] && at[1] < at[2], "{html}");
    assert_eq!(card_count(&html), 1, "{html}");
}

#[test]
fn a_second_bare_self_embed_is_a_plain_embed_and_leaves_no_placeholder() {
    let html = own_listing_site("![[./]]\n\nMiddle.\n\n![[./]]").html("index.html");
    assert!(!html.contains("MOSS_OWN_LISTING"), "{html}");
    let at = order(&html, &["Article One", ">Middle.<", "href=\"/direct/\" class=\"moss-card"]);
    assert!(at[0] < at[1] && at[1] < at[2], "{html}");
}

#[test]
fn a_self_embed_with_options_stays_a_plain_embed() {
    let html = own_listing_site("Before text.\n\n![[./|limit:1]]\n\nAfter text.").html("index.html");
    let at = order(&html, &["Before text.", "href=\"/direct/\" class=\"moss-card", "After text.", "Article One"]);
    assert!(at.windows(2).all(|w| w[0] < w[1]) && !html.contains("MOSS_OWN_LISTING"), "{html}");
}

mod empty_sections {
    use super::*;

    fn home(body: &str) -> String {
        format!("---\ntitle: Home\nchildren: false\n---\n\n{body}\n")
    }

    fn site(body: &str, with_post: bool) -> Site {
        let mut files = vec![("index.md", home(body)), ("posts/index.md", "---\ntitle: Posts\n---\n\nPosts.\n".to_string())];
        if with_post {
            files.push(("posts/20260101-0900.md", "A post.\n".to_string()));
        }
        let files: Vec<(&str, &str)> = files.iter().map(|(p, b)| (*p, b.as_str())).collect();
        Site::new(&files)
    }

    const EMBED: &str = "Intro.\n\n## Posts\n\n![[posts/|limit:1]]\n\n## Other\n\nTail.";

    #[test]
    fn a_heading_over_only_empty_embeds_is_left_out_and_the_note_names_its_id() {
        let site = site(EMBED, false);
        let html = site.html("index.html");
        assert!(!html.contains("Posts</h2>") && !html.contains("id=\"posts\""), "{html}");
        assert!(html.contains("Other</h2>") && html.contains("Tail."), "{html}");
        assert!(site.notes.iter().any(|n| n.contains("[index.md] left out the heading #posts")), "{:?}", site.notes);
    }

    #[test]
    fn nested_headings_over_only_empty_embeds_are_both_left_out() {
        let site = site("## Posts\n\n### Latest\n\n![[posts/|limit:1]]\n\n## Other\n\nTail.", false);
        let html = site.html("index.html");
        assert!(!html.contains("Posts</h2>") && !html.contains("Latest</h3>"), "{html}");
        assert!(html.contains("Other</h2>"), "{html}");
        assert!(site.notes.iter().any(|n| n.contains("#latest, #posts")), "{:?}", site.notes);
    }

    #[test]
    fn a_heading_is_kept_when_a_page_exists() {
        let site = site(EMBED, true);
        let html = site.html("index.html");
        assert!(html.contains("Posts</h2>") && html.contains("/posts/2026"), "{html}");
        assert!(site.notes.iter().all(|n| !n.contains("left out")), "{:?}", site.notes);
    }

    #[test]
    fn a_nested_heading_with_a_page_keeps_both() {
        let html = site("## Posts\n\n### Latest\n\n![[posts/|limit:1]]", true).html("index.html");
        assert!(html.contains("Posts</h2>") && html.contains("Latest</h3>"), "{html}");
    }

    #[test]
    fn a_sub_heading_with_prose_keeps_the_outer_heading_and_drops_only_the_empty_one() {
        let html = site("## Posts\n\nWords.\n\n### Latest\n\n![[posts/|limit:1]]", false).html("index.html");
        assert!(html.contains("Posts</h2>") && html.contains("Words."), "{html}");
        assert!(!html.contains("Latest</h3>"), "{html}");
    }

    #[test]
    fn a_missing_folder_keeps_the_heading() {
        let html = site("## Posts\n\n![[nowhere/|limit:1]]", false).html("index.html");
        assert!(html.contains("Posts</h2>") && html.contains("moss-embed-missing"), "{html}");
    }

    #[test]
    fn a_top_heading_is_never_left_out() {
        let html = site("# Posts\n\n![[posts/|limit:1]]", false).html("index.html");
        assert!(html.contains("Posts</h1>"), "{html}");
    }

    #[test]
    fn a_section_holding_the_pages_own_listing_stays() {
        let mut files = articles();
        files.push(("index.md", "---\ntitle: Home\n---\n\nHi.\n\n## Latest\n\n![[./]]\n"));
        let html = Site::new(&files).html("index.html");
        let at = order(&html, &["Latest</h2>", "Article One"]);
        assert!(at[0] < at[1], "{html}");
    }

    #[test]
    fn the_first_page_brings_the_heading_back_on_the_next_build() {
        let site = site(EMBED, false);
        assert!(!site.html("index.html").contains("Posts</h2>"));
        site.write("posts/20260102-0900.md", "A first post.\n");
        site.rebuild();
        assert!(site.html("index.html").contains("Posts</h2>"));
    }

    #[test]
    fn watch_builds_bring_the_heading_back_and_take_it_away() {
        use moss_build::build::BuildTrigger::{ContentOnly, Structural};
        let site = site(EMBED, false);
        let root = site.root.to_string_lossy().to_string();
        let post = site.root.join("posts/2026-01-02 0900.md");
        site.write("posts/2026-01-02 0900.md", "A first post.\n");
        super::site::build_sync(&root, Structural(vec![post.clone()])).unwrap();
        assert!(site.html("index.html").contains("Posts</h2>"), "a new post brings the heading back");
        std::fs::remove_file(&post).unwrap();
        super::site::build_sync(&root, Structural(vec![post])).unwrap();
        assert!(!site.html("index.html").contains("Posts</h2>"), "removing the only post takes it away");
        site.write("index.md", &home(&format!("{EMBED}\n\nEdited.")));
        super::site::build_sync(&root, ContentOnly(vec![site.root.join("index.md")])).unwrap();
        let html = site.html("index.html");
        assert!(html.contains("Edited.") && !html.contains("Posts</h2>"), "an edit of the page keeps the empty section out: {html}");
    }

    #[test]
    fn a_later_heading_of_the_same_name_answers_the_anchor_the_dropped_one_had() {
        let site = site("## Posts\n\n![[posts/|limit:1]]\n\n## Posts\n\nWords about posts, see [above](#posts-1).", false);
        let html = site.html("index.html");
        assert!(html.contains(r#"<h2 id="posts-1""#), "the kept heading keeps its own id: {html}");
        assert!(html.contains(r#"<span id="posts"></span>"#), "and answers #posts too: {html}");
        assert!(html.contains(r##"href="#posts-1""##), "{html}");
        let note = site.notes.iter().find(|n| n.contains("left out the heading #posts")).unwrap_or_else(|| panic!("{:?}", site.notes));
        assert!(!note.contains("no target"), "#posts still names a heading: {note}");
    }

    #[test]
    fn the_first_body_namesake_answers_the_anchor_not_an_endnote_heading_written_above_it() {
        let body = "## Posts\n\n![[posts/|limit:1]]\n\n## Notes\n\nSee[^a].\n\n[^a]: ## Posts\n\n## Posts\n\nWords.";
        let html = site(body, false).html("index.html");
        let heading = |id: &str| {
            let at = html.find(&format!(r#" id="{id}""#)).unwrap_or_else(|| panic!("no #{id}: {html}"));
            html[at..].split("</h").next().unwrap().to_string()
        };
        assert!(heading("posts-1").contains(r#"<span id="posts">"#), "the body heading answers #posts: {html}");
        assert!(!heading("posts-2").contains(r#"<span id="posts">"#), "the endnote heading does not: {html}");
    }

    #[test]
    fn a_heading_of_another_name_keeps_its_id_and_answers_nothing_new() {
        for kept in ["## Posts 2", "## Notes {#posts-2}"] {
            let site = site(&format!("## Posts\n\n![[posts/|limit:1]]\n\n{kept}\n\nWords, see [here](#posts-2)."), false);
            let html = site.html("index.html");
            assert!(html.contains(r#"<h2 id="posts-2""#) && html.contains(r##"href="#posts-2""##), "{kept}: {html}");
            assert!(!html.contains(r#"id="posts""#), "{kept} is not another Posts: {html}");
            let note = site.notes.iter().find(|n| n.contains("left out the heading #posts")).unwrap_or_else(|| panic!("{:?}", site.notes));
            assert!(note.contains("no target"), "{note}");
        }
    }
}

#[test]
fn a_renamed_folders_own_embed_draws_no_more_link_back_to_itself() {
    let site = Site::new(&[
        ("Jottings/index.md", "---\ntitle: Notes\nurl: notes\nchildren: false\n---\n\n![[./|limit:1]]\n"),
        ("Jottings/2026-01-01 0900.md", "---\ntitle: First Jot\n---\n\nFirst.\n"),
        ("Jottings/2026-01-02 0900.md", "---\ntitle: Second Jot\n---\n\nSecond.\n"),
    ]);
    let html = site.html("notes/index.html");
    assert!(html.contains("Second Jot") && !html.contains("First Jot"), "{html}");
    assert!(!html.contains("moss-embed-more"), "a More link here would lead back to this page: {html}");
}
