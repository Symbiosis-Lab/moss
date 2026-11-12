//! The pass that finishes an import once every page is on disk.
//!
//! Two things about a page are only knowable after the crawl, so they are
//! decided here, over the pages the run wrote, with a metadata pass and at most one
//! write per page:
//!
//! - **Folder indexes do not list their children.** moss renders a folder's
//!   child pages under its index page's content. A page imported from a live
//!   site never had that listing — the source already has its own — so the
//!   result came out many times taller than the original. Any `index.md`, and
//!   any `<name>.md` beside a folder `<name>/` (moss's folder-note convention), gets `children: false` when the folder it indexes holds
//!   another written page; the folder can be written after the page that owns
//!   it. A bundle leaf, or a folder note over an empty folder, gets nothing.
//! - **The site name is not part of a page title.** A segment of every page's
//!   title that is the site's own name (`About | Studio Name`) is stripped. The
//!   site's name is the one fact taken from the home page: its `title:` as
//!   written, or its `publisher:`. A segment shared by every page that is not
//!   that (a tutorial series' `Tutorial - Intro`, `Tutorial - Setup`) is
//!   content and stays.
//!
//! Leaf pages with nothing to change are never rewritten, and a rerun changes
//! nothing.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use regex::Regex;
use std::sync::LazyLock;

use super::metadata::{same_name, strip_edge_name, strip_site_name, SEPARATOR};
use moss_core::frontmatter_typed::FrontMatter;
use crate::vault::import::page::{with_fields_absent, with_title};

/// A shared site name is only trusted over at least this many pages besides
/// the home page; one other page proves nothing.
const MIN_OTHER_PAGES: usize = 2;

struct Page {
    relative: String,
    frontmatter: String,
    title: Option<String>,
}

/// Finish the pages in `written` (paths relative to `out_dir`).
pub(crate) fn finalize_written_pages(
    out_dir: &Path,
    written: &[String],
    listings: &HashSet<String>,
) -> Result<(), String> {
    let mut pages = Vec::with_capacity(written.len());
    for relative in written {
        // allow:raw_read the importer's own output, written moments ago in this run — not a cloud-evictable vault input
        let content = fs::read_to_string(out_dir.join(relative))
            .map_err(|e| format!("Failed to read {relative}: {e}"))?;
        let title = frontmatter_text(&content, "title");
        let frontmatter = moss_core::frontmatter::frontmatter_span(&content)
            .and_then(|span| content.get(..span.body)).unwrap_or("").to_string();
        pages.push(Page { relative: relative.clone(), frontmatter, title });
    }
    let site_name = shared_site_name(&pages);
    let tag_pages = tags_carried(&pages);
    let mut claimed_tags: HashSet<String> = HashSet::new();

    for page in &pages {
        // allow:raw_read the importer's own output; retain only one body while finishing pages
        let content = fs::read_to_string(out_dir.join(&page.relative))
            .map_err(|e| format!("Failed to read {}: {e}", page.relative))?;
        let mut updated = content.clone();
        if is_folder_index(written, &page.relative) {
            if let Some(next) = with_children_off(&updated) {
                updated = next;
            }
        }
        if let (Some(name), Some(title)) = (&site_name, &page.title) {
            let stripped = strip_site_name(title, &[name]);
            if stripped != *title {
                if let Some(next) = with_title(&updated, &stripped) {
                    updated = next;
                }
            }
        }
        // A tag archive claims its tag and lets moss draw the listing.
        if listings.contains(&page.relative) {
            if let Some(next) = frontmatter_text(&updated, "title")
                .filter(|title| tag_pages.carried_elsewhere(title, &page.relative))
                // One page can hold a tag; any other listing keeps what it imported.
                .filter(|title| claimed_tags.insert(title.trim().to_string()))
                .and_then(|tag| as_tag_page(&updated, &tag))
            {
                updated = next;
            }
        }
        // After the title is final: the heading repeating it is dropped last.
        if let Some(title) = frontmatter_text(&updated, "title") {
            let publisher = frontmatter_text(&updated, "publisher");
            let names: Vec<&str> =
                site_name.iter().chain(publisher.iter()).map(String::as_str).collect();
            if let Some(span) = moss_core::frontmatter::frontmatter_span(&updated) {
                // A page that is only its title keeps it, as `compose_note` does.
                // The crawl's notes were composed with adoption already; a second look
                // would adopt the heading after the one that was.
                match without_title_heading(&updated[span.body..], &title, &names, false) {
                    Some(TitleHeading { body, .. }) if !body.trim().is_empty() => {
                        updated = format!("{}{body}", &updated[..span.body]);
                    }
                    _ => {}
                }
            }
        }
        if updated != content {
            // allow:raw_write the importer rewrites a page it wrote moments ago in this run, adding what only the finished crawl can decide
            fs::write(out_dir.join(&page.relative), updated)
                .map_err(|e| format!("Failed to write {}: {e}", page.relative))?;
        }
    }
    Ok(())
}

/// Which pages carry each tag in `tags:`.
struct TagsCarried(HashMap<String, HashSet<String>>);

impl TagsCarried {
    /// Whether some page other than `relative` carries `tag`.
    fn carried_elsewhere(&self, tag: &str, relative: &str) -> bool {
        self.0.get(tag.trim()).is_some_and(|pages| pages.iter().any(|p| p != relative))
    }
}

fn tags_carried(pages: &[Page]) -> TagsCarried {
    let mut by_tag: HashMap<String, HashSet<String>> = HashMap::new();
    for page in pages {
        let map = moss_core::frontmatter::frontmatter_map(&page.frontmatter);
        let Some(serde_yaml::Value::Sequence(tags)) = map.get("tags") else { continue };
        for tag in tags.iter().filter_map(moss_core::frontmatter::value_as_string) {
            by_tag.entry(tag.trim().to_string()).or_default().insert(page.relative.clone());
        }
    }
    TagsCarried(by_tag)
}

/// The page as the page of `tag`: `tag_page:` added, and the body reduced to
/// the page's description, since moss draws the listing under it.
fn as_tag_page(content: &str, tag: &str) -> Option<String> {
    let claimed = with_fields_absent(content, &FrontMatter {
        tag_page: Some(moss_core::terms::TermClaim::Name(tag.to_string())), ..Default::default()
    })?;
    let span = moss_core::frontmatter::frontmatter_span(&claimed)?;
    let body = frontmatter_text(&claimed, "description").map(|d| format!("\n{d}\n")).unwrap_or_default();
    Some(format!("{}{body}", claimed.get(..span.body)?))
}

fn frontmatter_text(content: &str, key: &str) -> Option<String> {
    moss_core::frontmatter::frontmatter_map(content)
        .get(key)
        .and_then(moss_core::frontmatter::value_as_string)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// A page indexes a folder when that folder holds another page written in this
/// run: an `index.md` indexes its parent, a `<name>.md` indexes `<name>/`.
/// A bundle leaf (`2026/09/slug/index.md` with only images beside it) indexes
/// nothing. Decided from the run's own page list, never the file system.
fn is_folder_index(written: &[String], relative: &str) -> bool {
    let path = Path::new(relative);
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    let folder = if stem == "index" {
        path.parent().unwrap_or(Path::new("")).to_path_buf()
    } else {
        path.with_extension("")
    };
    written
        .iter()
        .any(|other| other != relative && Path::new(other).starts_with(&folder))
}

/// The home page's own name (its `title:` or `publisher:`) when every other
/// titled page carries it at an edge behind a separator.
/// `None` when there is no home page, no such name, or too few pages.
fn shared_site_name(pages: &[Page]) -> Option<String> {
    let home = pages.iter().find(|p| p.relative == "index.md")?;
    let others: Vec<&str> = pages
        .iter()
        .filter(|p| p.relative != "index.md")
        .filter_map(|p| p.title.as_deref())
        .collect();
    let candidates = [home.title.clone(), frontmatter_text(&home.frontmatter, "publisher")];
    candidates.into_iter().flatten().find(|name| {
        let others: Vec<&&str> = others.iter().filter(|t| !same_name(t, name)).collect();
        others.len() >= MIN_OTHER_PAGES && others.iter().all(|t| strip_edge_name(t, name).is_some())
    })
}

/// `content` with `children: false` appended as the last frontmatter line.
fn with_children_off(content: &str) -> Option<String> {
    with_fields_absent(content, &FrontMatter { children: Some(false), ..Default::default() })
}

static ATX_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ {0,3}#{1,6}[ \t]+(.*?)(?:[ \t]+#+)?[ \t]*$").expect("atx heading regex")
});
static MD_LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]*)\]\([^)]*\)").expect("link regex"));
static MD_IMAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^!(?:\[[^\]]*\]\([^)]*\)|\[\[[^\]]*\]\])$").expect("image regex")
});

/// The text a heading shows: links unwrapped, emphasis marks and backslash
/// escapes gone.
fn heading_text(raw: &str) -> String {
    let unlinked = MD_LINK.replace_all(raw, "$1");
    let mut out = String::new();
    let mut chars = unlinked.chars().peekable();
    let mut prev: Option<char> = None;
    while let Some(c) = chars.next() {
        // An emphasis mark sits at a word edge; one between two letters or
        // digits (`snake_case`) is part of the word. Unsure fails safe: the
        // heading then differs from the title and is kept.
        let in_word = prev.is_some_and(char::is_alphanumeric)
            && chars.peek().is_some_and(|n| n.is_alphanumeric());
        match c {
            '\\' if chars.peek().is_some_and(|n| n.is_ascii_punctuation()) => {}
            '*' | '_' if !in_word => {}
            '`' => {}
            c => out.push(c),
        }
        prev = Some(c);
    }
    out
}

/// What `without_title_heading` decided: the body without its first heading,
/// and, when that heading replaced an internal page name, the title it became.
pub(super) struct TitleHeading {
    pub(super) title: Option<String>,
    pub(super) body: String,
}

/// Text as a reader sees it, for comparing: links unwrapped, case folded,
/// punctuation, underscores and hyphens read as spaces, whitespace collapsed.
fn visible_words(text: &str) -> String {
    let unlinked = MD_LINK.replace_all(text, "$1");
    let spaced: String =
        unlinked.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Whether the title shows anywhere in `body`, as a whole or as one of its
/// separator-delimited segments (a site name not yet known would otherwise
/// make `Page | Name` look absent from a body that says `Page`).
fn title_is_shown(body: &str, title: &str) -> bool {
    let shown = format!(" {} ", visible_words(body));
    std::iter::once(title).chain(SEPARATOR.split(title)).any(|part| {
        let words = visible_words(part);
        !words.is_empty() && shown.contains(&format!(" {words} "))
    })
}

/// Builders' internal page names (`T_Camino`) are not titles a reader sees.
fn looks_like_internal_name(title: &str) -> bool {
    let t = title.trim();
    !t.is_empty()
        && t.contains('_')
        && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `body` without its first heading when that heading is the page's own
/// title, which moss renders itself, so keeping it prints the title twice.
/// Equal after whitespace and case normalisation, with or without the site
/// name (`About | Studio` is `About`). Only the first heading of any level is
/// considered: a different one, or any later one, stays.
///
/// With `adopt`, a leading heading (only blank lines, a lone link line or a
/// lone image line before it) also goes when the title shows nowhere in the page: that title
/// is a name the builder kept internally, and the heading is the real one, so
/// it is returned as the new title. `None` when nothing changes.
pub(super) fn without_title_heading(
    body: &str,
    title: &str,
    site_names: &[&str],
    adopt: bool,
) -> Option<TitleHeading> {
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    let mut fenced = false;
    let mut leading = true;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            leading = false;
            continue;
        }
        if fenced {
            continue;
        }
        let text = line.trim_end_matches(['\r', '\n']);
        // A setext heading (`Text` over `===`) is a heading too; it is the
        // first one, and this pass does not rewrite that form.
        if lines.get(i + 1).is_some_and(|u| {
            let u = u.trim();
            !text.trim().is_empty() && !u.is_empty() && u.chars().all(|c| c == '=')
        }) {
            return None;
        }
        let Some(caps) = ATX_HEADING.captures(text) else {
            let t = text.trim();
            let lone_link = MD_LINK.is_match(t) && MD_LINK.replace(t, "").is_empty();
            let lone_image = MD_IMAGE.is_match(t);
            leading &= t.is_empty() || lone_link || lone_image;
            continue;
        };
        let heading = heading_text(&caps[1]);
        let bare_title = strip_site_name(title, site_names);
        let repeats = same_name(&heading, title)
            || same_name(&strip_site_name(&heading, site_names), &bare_title);
        let new_title = (!repeats
            && adopt
            && leading
            && looks_like_internal_name(&bare_title)
            && !title_is_shown(body, &bare_title))
        .then(|| strip_site_name(&heading, site_names))
            .filter(|t| !t.trim().is_empty());
        if !repeats && new_title.is_none() {
            return None;
        }
        // Take one of the blank lines around it too, so no gap is left.
        let blank = |j: Option<&&str>| j.is_none_or(|l| l.trim().is_empty());
        let skip_after = blank(i.checked_sub(1).and_then(|j| lines.get(j)))
            && lines.get(i + 1).is_some_and(|l| l.trim().is_empty());
        let rest = lines[i + 1..].iter().skip(usize::from(skip_after));
        let body = lines[..i].iter().chain(rest).copied().collect();
        return Some(TitleHeading { title: new_title, body });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dropped(body: &str, title: &str, names: &[&str]) -> Option<String> {
        without_title_heading(body, title, names, false).map(|r| r.body)
    }

    fn adopted(body: &str, title: &str) -> Option<(String, String)> {
        let r = without_title_heading(body, title, &[], true)?;
        Some((r.title?, r.body))
    }

    fn page(title: &str) -> String {
        format!("---\ntitle: \"{title}\"\norigin: \"https://example.com/\"\n---\n\nBody\n")
    }

    fn put(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    fn run(root: &Path, pages: &[&str]) {
        let written: Vec<String> = pages.iter().map(|s| s.to_string()).collect();
        finalize_written_pages(root, &written, &HashSet::new()).unwrap();
    }

    fn read(root: &Path, rel: &str) -> String {
        fs::read_to_string(root.join(rel)).unwrap()
    }

    #[test]
    fn only_folder_indexes_get_children_false_and_a_rerun_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pages = ["index.md", "events.md", "events/2026/one.md", "about.md"];
        for p in pages {
            put(root, p, &page("T"));
        }

        run(root, &pages);

        let marked = "---\ntitle: \"T\"\norigin: \"https://example.com/\"\nchildren: false\n---\n\nBody\n";
        for p in ["index.md", "events.md"] {
            assert_eq!(read(root, p), marked, "{p}");
        }
        for p in ["about.md", "events/2026/one.md"] {
            assert_eq!(read(root, p), page("T"), "leaf {p}");
        }

        run(root, &pages);
        assert_eq!(read(root, "index.md"), marked);
    }

    #[test]
    fn bundle_leaves_and_folder_notes_over_empty_folders_get_no_children_key() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pages = ["index.md", "events.md", "events/2026/one.md", "about.md", "2026/09/slug/index.md"];
        for p in pages {
            put(root, p, &page("T"));
        }
        fs::write(root.join("2026/09/slug/cover.png"), b"png").unwrap();
        put(root, "empty.md", &page("T"));
        fs::create_dir_all(root.join("empty")).unwrap();

        run(root, &[&pages[..], &["empty.md"]].concat());

        for p in ["index.md", "events.md"] {
            assert!(read(root, p).contains("children: false\n"), "{p}");
        }
        for p in ["about.md", "2026/09/slug/index.md", "empty.md", "events/2026/one.md"] {
            assert_eq!(read(root, p), page("T"), "{p}");
        }
    }

    #[test]
    fn a_lone_home_page_is_not_an_index() {
        let tmp = tempfile::tempdir().unwrap();
        put(tmp.path(), "index.md", &page("T"));
        run(tmp.path(), &["index.md"]);
        assert_eq!(read(tmp.path(), "index.md"), page("T"));
    }

    #[test]
    fn a_nested_index_counts_and_a_page_without_frontmatter_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        put(root, "news/index.md", &page("T"));
        put(root, "news/a.md", &page("T"));
        put(root, "plain/x.md", &page("T"));
        put(root, "plain.md", "no frontmatter\n");

        run(root, &["news/index.md", "news/a.md", "plain/x.md", "plain.md"]);

        assert!(read(root, "news/index.md").contains("children: false\n"));
        assert_eq!(read(root, "plain.md"), "no frontmatter\n");
    }

    #[test]
    fn the_home_pages_own_name_is_stripped_from_every_page_that_shares_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pages = ["index.md", "about.md", "press.md"];
        put(root, "index.md", &page("Studio Name"));
        put(root, "about.md", &page("About | Studio Name"));
        put(root, "press.md", &page("Press | Studio Name"));

        run(root, &pages);

        assert_eq!(
            read(root, "index.md"),
            "---\ntitle: \"Studio Name\"\norigin: \"https://example.com/\"\nchildren: false\n---\n\nBody\n",
            "the home page keeps its name"
        );
        assert!(read(root, "about.md").contains("title: \"About\"\n"));
        assert!(read(root, "press.md").contains("title: \"Press\"\n"));

        run(root, &pages);
        assert!(read(root, "press.md").contains("title: \"Press\"\n"));
    }

    #[test]
    fn a_series_word_every_page_shares_is_content_not_a_site_name() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let titles = [
            ("index.md", "Welcome"),
            ("intro.md", "Tutorial - Intro"),
            ("setup.md", "Tutorial - Setup"),
            ("deploy.md", "Tutorial - Deploy"),
        ];
        for (rel, t) in titles {
            put(root, rel, &page(t));
        }

        run(root, &titles.map(|(r, _)| r));

        for (rel, t) in &titles[1..] {
            assert_eq!(read(root, rel), page(t), "{rel}");
        }
    }

    #[test]
    fn a_home_name_that_contains_a_separator_is_stripped_from_every_page() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let name = "Studio Name - Costume and set design";
        put(root, "index.md", &page(name));
        for p in ["a", "b", "c"] {
            put(root, &format!("{p}.md"), &page(&format!("{p} — {name}")));
        }

        run(root, &["index.md", "a.md", "b.md", "c.md"]);

        for p in ["a", "b", "c"] {
            assert!(read(root, &format!("{p}.md")).contains(&format!("title: \"{p}\"\n")));
        }
    }

    #[test]
    fn the_name_is_taken_from_the_home_pages_publisher_when_its_title_differs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        put(
            root,
            "index.md",
            "---\ntitle: \"Welcome\"\npublisher: \"Studio Name\"\n---\n\nBody\n",
        );
        put(root, "a.md", &page("Studio Name · Alpha"));
        put(root, "b.md", &page("Studio Name · Beta"));

        run(root, &["index.md", "a.md", "b.md"]);

        assert!(read(root, "a.md").contains("title: \"Alpha\"\n"));
        assert!(read(root, "b.md").contains("title: \"Beta\"\n"));
    }

    #[test]
    fn a_first_heading_repeating_the_title_is_dropped() {
        let body = "[Back](/x)\n\n# **About**  \n\nIntro.\n\n## About\n\nMore.\n";
        assert_eq!(
            dropped(body, "about", &[]).as_deref(),
            Some("[Back](/x)\n\nIntro.\n\n## About\n\nMore.\n"),
            "only the first heading goes, at any level"
        );
        assert!(dropped("## About\n\nText\n", "About", &[]).is_some());
    }

    #[test]
    fn the_site_name_does_not_hide_a_repeated_title() {
        let body = "# About | Studio Name\n\nText\n";
        assert_eq!(dropped(body, "About", &["Studio Name"]).as_deref(), Some("Text\n"));
        assert_eq!(
            dropped("# About\n\nText\n", "About | Studio Name", &["Studio Name"])
                .as_deref(),
            Some("Text\n")
        );
    }

    #[test]
    fn an_underscore_inside_a_word_is_not_emphasis() {
        assert_eq!(dropped("# snake_case\n\nText\n", "snakecase", &[]), None);
        assert!(dropped("# _snake_case_\n\nText\n", "snake_case", &[]).is_some());
    }

    #[test]
    fn a_heading_replaces_a_title_the_page_never_shows() {
        let body = "# The Winter's Tale\n\nA staging in two acts.\n";
        assert_eq!(
            adopted(body, "T_Winter_Tale_S"),
            Some(("The Winter's Tale".to_string(), "A staging in two acts.\n".to_string()))
        );
    }

    #[test]
    fn a_cjk_title_absent_from_the_body_is_kept_over_a_leading_heading() {
        assert_eq!(adopted("## 欢迎\n\n正文。\n", "关于我们"), None);
    }

    #[test]
    fn an_english_title_absent_from_the_body_is_kept_over_a_leading_heading() {
        assert_eq!(adopted("## Editor's Note\n\nText.\n", "A Reading List"), None);
    }

    #[test]
    fn a_title_shown_in_the_body_is_left_alone() {
        let body = "# The Winter's Tale\n\nAlso known as winter tale s.\n";
        assert_eq!(adopted(body, "Winter-Tale_S"), None);
        // A site name still attached to the title does not make it look absent.
        assert_eq!(adopted("# Longer heading\n\nAbout us.\n", "About | Studio"), None);
    }

    #[test]
    fn no_leading_heading_leaves_the_title_alone() {
        assert_eq!(adopted("Just text.\n", "T_Winter_Tale_S"), None);
        assert_eq!(adopted("Intro paragraph.\n\n# The Winter's Tale\n", "T_Winter_Tale_S"), None);
    }

    #[test]
    fn a_category_link_line_does_not_stop_the_heading_leading() {
        let body = "[OPERA](/work/opera)\n\n## The Winter's Tale\n\nText.\n";
        assert_eq!(
            adopted(body, "T_Winter_Tale_S"),
            Some(("The Winter's Tale".to_string(), "[OPERA](/work/opera)\n\nText.\n".to_string()))
        );
    }

    #[test]
    fn a_lead_image_before_the_heading_does_not_stop_it_leading() {
        let body = "![](./assets/imported/abc.jpg)\n\n## The Night Walk\n\nText.\n";
        assert_eq!(
            adopted(body, "P_Night_Walk"),
            Some(("The Night Walk".to_string(), "![](./assets/imported/abc.jpg)\n\nText.\n".to_string()))
        );
        let wiki = "![[abc.jpg]]  \n\n## The Night Walk\n\nText.\n";
        assert_eq!(adopted(wiki, "P_Night_Walk").map(|(t, _)| t).as_deref(), Some("The Night Walk"));
    }

    #[test]
    fn a_text_paragraph_before_the_heading_still_stops_it_leading() {
        assert_eq!(adopted("A paragraph of text.\n\n## The Night Walk\n\nText.\n", "P_Night_Walk"), None);
    }

    #[test]
    fn a_page_that_is_only_its_title_keeps_the_heading_after_the_crawl() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        put(root, "index.md", &page("Studio Name"));
        let only = "---\ntitle: \"a\"\n---\n\n# a\n";
        put(root, "a.md", only);
        run(root, &["index.md", "a.md"]);
        assert_eq!(read(root, "a.md"), only);
    }

    #[test]
    fn a_different_first_heading_stays_and_shields_later_ones() {
        let body = "# Welcome\n\n## About\n\nText\n";
        assert_eq!(dropped(body, "About", &[]), None);
        // A heading inside a code fence is not a heading.
        let fenced = "```\n# About\n```\n\n# Other\n";
        assert_eq!(dropped(fenced, "About", &[]), None);
    }

    #[test]
    fn a_crawl_drops_the_heading_that_repeats_the_title_the_site_name_left_behind() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let name = "Studio Name";
        put(root, "index.md", &page(name));
        for p in ["a", "b"] {
            put(root, &format!("{p}.md"), &format!(
                "---\ntitle: \"{p} | {name}\"\n---\n\n# {p}\n\nText\n"
            ));
        }
        run(root, &["index.md", "a.md", "b.md"]);
        assert_eq!(read(root, "a.md"), "---\ntitle: \"a\"\n---\n\nText\n");
    }

    fn post(title: &str) -> String {
        format!("---\ntitle: \"{title}\"\ntags: [\"Field Notes\", \"Slow Reading\"]\n---\n\nBody\n")
    }

    const ARCHIVE: &str = "---\ntitle: \"Field Notes\"\ndescription: \"Tuesday notes.\"\ncover: \"./assets/imported/a.jpg\"\norigin: \"https://notes.example.test/tag/tuesdays/\"\nnav: true\nweight: 3\n---\n\n[First light](/first-light/)\n\n[Second draft](/second-draft/)\n";

    #[test]
    fn a_listing_titled_like_a_carried_tag_claims_it_and_keeps_its_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        put(root, "first-light.md", &post("First light"));
        put(root, "second-draft.md", &post("Second draft"));
        put(root, "tag/tuesdays.md", ARCHIVE);
        put(root, "notes.md", ARCHIVE);
        let written: Vec<String> =
            ["first-light.md", "second-draft.md", "tag/tuesdays.md", "notes.md"].map(String::from).to_vec();
        let listings = HashSet::from(["tag/tuesdays.md".to_string()]);

        finalize_written_pages(root, &written, &listings).unwrap();

        let claimed = read(root, "tag/tuesdays.md");
        assert!(claimed.contains("tag_page: \"Field Notes\"\n"), "{claimed}");
        for kept in ["title: \"Field Notes\"", "description: \"Tuesday notes.\"", "cover: \"./assets/imported/a.jpg\"",
            "origin: \"https://notes.example.test/tag/tuesdays/\"", "nav: true", "weight: 3"] {
            assert!(claimed.contains(kept), "{kept} lost: {claimed}");
        }
        assert!(claimed.ends_with("---\n\nTuesday notes.\n"), "body is the description only: {claimed}");
        assert_eq!(read(root, "notes.md"), ARCHIVE, "no listing signal: stays as written");
        assert_eq!(read(root, "first-light.md"), post("First light"));
    }

    #[test]
    fn only_the_first_listing_titled_like_a_tag_claims_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        put(root, "first-light.md", &post("First light"));
        put(root, "a.md", ARCHIVE);
        put(root, "b.md", ARCHIVE);
        let written: Vec<String> = ["first-light.md", "a.md", "b.md"].map(String::from).to_vec();
        let listings = HashSet::from(["a.md".to_string(), "b.md".to_string()]);

        finalize_written_pages(root, &written, &listings).unwrap();

        assert!(read(root, "a.md").contains("tag_page:"));
        assert_eq!(read(root, "b.md"), ARCHIVE, "the second listing keeps its body");
    }
}
