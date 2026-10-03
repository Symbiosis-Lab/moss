//! The pass that finishes an import once every page is on disk.
//!
//! Two things about a page are only knowable after the crawl, so they are
//! decided here, over the pages the run wrote, with one read and at most one
//! write per page:
//!
//! - **Folder indexes do not list their children.** moss renders a folder's
//!   child pages under its index page's content. A page imported from a live
//!   site never had that listing — the source already has its own — so the
//!   result came out many times taller than the original. Any `index.md`, and
//!   any `<name>.md` with a sibling folder `<name>/` (moss's folder-note
//!   convention), gets `children: false`; the folder can be written after the
//!   page that owns it.
//! - **The site name is not part of a page title.** A segment of every page's
//!   title that is the site's own name (`About | Studio Name`) is stripped. The
//!   site's name is the one fact taken from the home page: its `title:` as
//!   written, or its `publisher:`. A segment shared by every page that is not
//!   that (a tutorial series' `Tutorial - Intro`, `Tutorial - Setup`) is
//!   content and stays.
//!
//! Leaf pages with nothing to change are never rewritten, and a rerun changes
//! nothing.

use std::fs;
use std::path::Path;

use super::metadata::{same_name, strip_site_name, SEPARATOR};
use super::service::escape_yaml_string;

/// The one line added to a folder index, in the shape the importer's
/// frontmatter writer uses for every other key.
const CHILDREN_OFF: &str = "children: false\n";

/// A shared site name is only trusted over at least this many pages besides
/// the home page; one other page proves nothing.
const MIN_OTHER_PAGES: usize = 2;

struct Page {
    relative: String,
    content: String,
    title: Option<String>,
}

/// Finish the pages in `written` (paths relative to `out_dir`).
pub(crate) fn finalize_written_pages(out_dir: &Path, written: &[String]) -> Result<(), String> {
    let mut pages = Vec::with_capacity(written.len());
    for relative in written {
        // allow:raw_read the importer's own output, written moments ago in this run — not a cloud-evictable vault input
        let content = fs::read_to_string(out_dir.join(relative))
            .map_err(|e| format!("Failed to read {relative}: {e}"))?;
        let title = frontmatter_text(&content, "title");
        pages.push(Page { relative: relative.clone(), content, title });
    }
    let site_name = shared_site_name(&pages);

    for page in &pages {
        let mut updated = page.content.clone();
        if is_folder_index(out_dir, &page.relative) {
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
        if updated != page.content {
            // allow:raw_write the importer rewrites a page it wrote moments ago in this run, adding what only the finished crawl can decide
            fs::write(out_dir.join(&page.relative), updated)
                .map_err(|e| format!("Failed to write {}: {e}", page.relative))?;
        }
    }
    Ok(())
}

fn frontmatter_text(content: &str, key: &str) -> Option<String> {
    moss_core::frontmatter::frontmatter_map(content)
        .get(key)
        .and_then(moss_core::frontmatter::value_as_string)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn is_folder_index(out_dir: &Path, relative: &str) -> bool {
    let path = Path::new(relative);
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    stem == "index" || out_dir.join(path.with_extension("")).is_dir()
}

/// The home page's own name (its `title:` or `publisher:`) when every other
/// titled page carries it as a segment at the same edge — all trailing, or all
/// leading. `None` when there is no home page, no such name, or too few pages.
fn shared_site_name(pages: &[Page]) -> Option<String> {
    let home = pages.iter().find(|p| p.relative == "index.md")?;
    let others: Vec<&str> = pages
        .iter()
        .filter(|p| p.relative != "index.md")
        .filter_map(|p| p.title.as_deref())
        .collect();
    let candidates = [home.title.clone(), frontmatter_text(&home.content, "publisher")];
    candidates.into_iter().flatten().find(|name| {
        let at_edge = |pick: fn(Vec<&str>) -> Option<&str>| {
            others.iter().filter(|t| !same_name(t, name)).all(|t| {
                let parts: Vec<&str> = SEPARATOR.split(t).collect();
                parts.len() > 1 && pick(parts).is_some_and(|s| same_name(s, name))
            })
        };
        let enough = others.iter().filter(|t| !same_name(t, name)).count() >= MIN_OTHER_PAGES;
        enough && (at_edge(|p| p.last().copied()) || at_edge(|p| p.first().copied()))
    })
}

/// `content` with `children: false` appended as the last frontmatter line;
/// `None` when there is nothing to do (no YAML frontmatter, or the key is
/// already there). Spliced in as text so every other byte stays as written.
fn with_children_off(content: &str) -> Option<String> {
    let span = moss_core::frontmatter::frontmatter_span(content)?;
    if span.kind != moss_core::frontmatter::FrontmatterKind::Yaml
        || moss_core::frontmatter::frontmatter_map(content).contains_key("children")
    {
        return None;
    }
    let at = span.fields.end;
    Some(format!("{}{CHILDREN_OFF}{}", &content[..at], &content[at..]))
}

/// `content` with its `title:` line replaced, every other byte kept.
fn with_title(content: &str, title: &str) -> Option<String> {
    let span = moss_core::frontmatter::frontmatter_span(content)?;
    let fields = &content[span.fields.clone()];
    let line = fields.split_inclusive('\n').find(|l| l.starts_with("title:"))?;
    let at = span.fields.start + fields.find(line)?;
    Some(format!(
        "{}title: \"{}\"\n{}",
        &content[..at],
        escape_yaml_string(title),
        &content[at + line.len()..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        finalize_written_pages(root, &written).unwrap();
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
    fn a_nested_index_counts_and_a_page_without_frontmatter_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        put(root, "news/index.md", &page("T"));
        fs::create_dir_all(root.join("plain")).unwrap();
        put(root, "plain.md", "no frontmatter\n");

        run(root, &["news/index.md", "plain.md"]);

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
}
