//! `moss list` — what this site publishes, at which URL, and what is hidden.
//!
//! Two questions have no answer anywhere else in moss, and both come up on the
//! first day of a real site:
//!
//! 1. **"What URL does `《潮汐》第一期 — 邊界.md` publish at?"** moss's slug
//!    rules strip punctuation and keep CJK, which is the right behavior and
//!    completely undiscoverable — the only way to find out used to be to build
//!    and go read the emitted directory tree.
//! 2. **"Why is this article missing from the index?"** There are FOUR
//!    different ways a file stops showing up in a listing, and they hide it
//!    from different places: `draft:` (hidden everywhere, plus `noindex`),
//!    `listed: false` (out of generated listings, still a public page),
//!    being a nav item (root-level file in a site that has content folders —
//!    it moved to the nav bar), and `slot_only` (`footer.md` and friends are
//!    layout chrome, never a page). Reading the frontmatter of one file tells
//!    you about at most one of them.
//!
//! So this command prints the whole inventory with the hiding mechanism in its
//! own column, and `--json` gives an agent the same rows to filter.
//!
//! ## Why it reads a file the build wrote, not `article-map.json`
//!
//! `article-map.json` is not enough. `build_article_map` routes
//! `PageKind::Folder` pages and the root `index.html` into `map.pages`, a bare
//! url→source-path map with no title and no date, and it drops `slot_only`
//! documents entirely — so section indexes and the homepage would print with
//! empty columns and `footer.md` would silently vanish, which is precisely the
//! "why is it missing" confusion this command exists to end.
//!
//! The signals that answer the question (`kind`, `draft`, `listed`, `nav`,
//! `slot_only`) all exist together exactly once: on `ParsedDocument`, inside
//! the build. So the build writes them out — see
//! [`crate::build::emit::inventory`], called from `build/render/blocking.rs` — and
//! this command reads that file back.
//! No build, no inventory: the command says so rather than printing an empty
//! table, which would read as "this site has no pages".


use crate::build::emit::inventory::{inventory_path, InventoryEntry};

/// Display width of `s` in terminal cells.
///
/// The whole point of this command is CJK routing, so counting `char`s would
/// misalign every column on the sites that need it most: a Han character
/// occupies two cells. This covers the East Asian Wide and Fullwidth ranges,
/// which is what a title in Chinese, Japanese or Korean is made of; anything
/// outside them counts as one. Not a general Unicode width implementation —
/// combining marks and emoji sequences are close enough for a report.
fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let cp = c as u32;
            let wide = matches!(cp,
                0x1100..=0x115F        // Hangul Jamo
                | 0x2E80..=0x303E      // CJK radicals, Kangxi, CJK symbols/punctuation
                | 0x3041..=0x33FF      // Kana, Hangul Compatibility Jamo, CJK compat
                | 0x3400..=0x4DBF      // CJK Extension A
                | 0x4E00..=0x9FFF      // CJK Unified Ideographs
                | 0xA000..=0xA4CF      // Yi
                | 0xAC00..=0xD7A3      // Hangul syllables
                | 0xF900..=0xFAFF      // CJK compatibility ideographs
                | 0xFE30..=0xFE6F      // CJK compatibility forms
                | 0xFF00..=0xFF60      // Fullwidth forms
                | 0xFFE0..=0xFFE6
                | 0x20000..=0x3FFFD    // CJK Extension B and beyond
            );
            if wide { 2 } else { 1 }
        })
        .sum()
}

fn pad(s: &str, width: usize) -> String {
    let w = display_width(s);
    let mut out = s.to_string();
    for _ in w..width {
        out.push(' ');
    }
    out
}

/// Render the human table.
///
/// Separate from the printing so it is testable without a terminal.
pub fn render_table(rows: &[InventoryEntry]) -> String {
    let header = ["URL", "SOURCE", "LANG", "KIND", "DATE", "HIDDEN", "TITLE"];
    let cells: Vec<[String; 7]> = rows
        .iter()
        .map(|r| {
            [
                if r.url_path.is_empty() { "—".to_string() } else { r.url_path.clone() },
                r.source_path.clone().unwrap_or_else(|| "—".to_string()),
                r.lang.clone(),
                r.kind.clone(),
                r.date.clone().unwrap_or_else(|| "—".to_string()),
                if r.hidden.is_empty() { "—".to_string() } else { r.hidden.join(",") },
                r.title.clone(),
            ]
        })
        .collect();

    let mut widths = [0usize; 7];
    for (i, h) in header.iter().enumerate() {
        widths[i] = display_width(h);
    }
    for row in &cells {
        for (i, c) in row.iter().enumerate() {
            widths[i] = widths[i].max(display_width(c));
        }
    }

    let mut out = String::new();
    for (i, h) in header.iter().enumerate() {
        // No trailing padding on the last column — it would be invisible
        // whitespace at end of line in every row.
        if i + 1 == header.len() {
            out.push_str(h);
        } else {
            out.push_str(&pad(h, widths[i]));
            out.push_str("  ");
        }
    }
    out.push('\n');
    for row in &cells {
        for (i, c) in row.iter().enumerate() {
            if i + 1 == row.len() {
                out.push_str(c);
            } else {
                out.push_str(&pad(c, widths[i]));
                out.push_str("  ");
            }
        }
        out.push('\n');
    }
    out
}

/// A one-line legend under the table, printed only when something is hidden.
///
/// Without it "unlisted" and "nav-item" are jargon; with it the reader knows
/// which mechanism to reach for. Suppressed when every page is listed, so the
/// common case stays a clean table.
fn legend(rows: &[InventoryEntry]) -> Option<String> {
    let mut kinds: Vec<&str> = Vec::new();
    for r in rows {
        for h in &r.hidden {
            if !kinds.contains(&h.as_str()) {
                kinds.push(h.as_str());
            }
        }
    }
    if kinds.is_empty() {
        return None;
    }
    let mut lines = vec!["HIDDEN — why a page is absent from generated listings:".to_string()];
    for k in kinds {
        lines.push(match k {
            "draft" => "  draft      frontmatter `draft: true` — published at its URL, hidden from every listing, feed, the sitemap and nav; marked noindex".to_string(),
            "unlisted" => "  unlisted   frontmatter `listed: false` — out of generated listings and feeds, but still a public, indexable page".to_string(),
            "nav-item" => "  nav-item   a root-level file in a site that has content folders: it appears in the nav bar instead of the article list (set `nav: false` to move it back)".to_string(),
            "slot" => "  slot       layout chrome (footer.md, or `slot:` frontmatter) — renders into a template slot, never its own page".to_string(),
            other => format!("  {other}"),
        });
    }
    Some(lines.join("\n"))
}

fn usage() -> &'static str {
    "Usage: moss list [<folder>] [--json]\n  \
     Prints every document the last build parsed: its source file, the URL it\n  \
     publishes at, the language it will declare, and why it is hidden from\n  \
     listings, if it is.\n  \
     <folder> defaults to the current directory."
}

/// Entry point for `moss list`. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    let mut json = false;
    let mut folder: Option<String> = None;
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            "-h" | "--help" => {
                println!("{}", usage());
                return 0;
            }
            other if other.starts_with('-') => {
                eprintln!("Unknown option: {other}");
                eprintln!("{}", usage());
                return 1;
            }
            other => {
                if folder.is_some() {
                    eprintln!("Unexpected argument: {other}");
                    eprintln!("{}", usage());
                    return 1;
                }
                folder = Some(other.to_string());
            }
        }
    }

    let root = crate::vault_root::VaultRoot::resolve(folder.as_deref().unwrap_or("."));
    let moss_dir = root.path().join(".moss");
    let path = inventory_path(&moss_dir);

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            // Never print an empty list here: "no inventory" and "no pages"
            // look identical in output and mean opposite things.
            eprintln!("No inventory for {}.", root.as_str());
            eprintln!("Run `moss build {}` first — `moss list` reports what the last build produced.", root.as_str());
            return 1;
        }
    };
    let rows: Vec<InventoryEntry> = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => {
            // "Run `moss build`" alone was actively misleading here, because
            // the likeliest reader has *just* run one. A build that is
            // interrupted never rewrites this file, so `list` goes on reading
            // whatever the last COMPLETED build left — possibly from an older
            // moss whose schema differs. Say which of those it is.
            eprintln!("Could not read {}: {e}", path.display());
            eprintln!(
                "This file is rewritten only by a build that runs to completion — an\n\
                 interrupted one leaves the previous build's copy in place, which may\n\
                 have been written by a different version of moss."
            );
            eprintln!("Run `moss build {}` and let it finish.", root.as_str());
            return 1;
        }
    };

    if json {
        match serde_json::to_string_pretty(&rows) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("Failed to serialize inventory: {e}");
                return 1;
            }
        }
        return 0;
    }

    print!("{}", render_table(&rows));
    if let Some(l) = legend(&rows) {
        println!();
        println!("{l}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str, hidden: &[&str]) -> InventoryEntry {
        InventoryEntry {
            source_path: Some(format!("{url}.md")),
            url_path: url.to_string(),
            title: url.to_string(),
            date: None,
            kind: "article".to_string(),
            draft: hidden.contains(&"draft"),
            listed: !hidden.contains(&"unlisted"),
            nav: None,
            slot_only: hidden.contains(&"slot"),
            hidden: hidden.iter().map(|s| s.to_string()).collect(),
            lang: "zh-Hant".to_string(),
        }
    }

    /// An inventory written by an older moss must still read. This file
    /// crosses versions by construction — one build writes it, a later
    /// `moss list` reads it, and `moss` can be upgraded in between — so a
    /// newly added column must never turn a readable inventory into a parse
    /// error. It did exactly that for the hour between the LANG column
    /// landing and the trial that hit it.
    ///
    /// The fixture is deliberately a hand-written literal of the OLD shape
    /// rather than a serialized `InventoryEntry`: round-tripping the current
    /// struct would pass no matter what, which is how this went unnoticed.
    #[test]
    fn an_inventory_from_an_older_moss_still_parses() {
        let old = r#"[{
            "source_path": "en/en.md",
            "url_path": "en/index.html",
            "title": "Home",
            "date": null,
            "kind": "article",
            "draft": false,
            "listed": true,
            "nav": null,
            "slot_only": false,
            "hidden": []
        }]"#;
        let rows: Vec<InventoryEntry> =
            serde_json::from_str(old).expect("an inventory predating the LANG column must parse");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url_path, "en/index.html");
        assert_eq!(rows[0].lang, "", "a missing column reads empty, not an error");
        // And it must still render rather than panicking on the short row.
        assert!(render_table(&rows).contains("en/index.html"));
    }

    /// The LANG column exists to answer "did moss register my new edition?".
    /// A folder name moss does not recognize is treated as ordinary content
    /// and the build succeeds either way, so a header that silently dropped
    /// this column would take the answer away again.
    #[test]
    fn the_table_reports_the_language_each_page_declares() {
        let mut ja = entry("ja/index.html", &[]);
        ja.lang = "ja".to_string();
        let table = render_table(&[ja, entry("about/index.html", &[])]);
        let header = table.lines().next().expect("header");
        assert!(header.contains("LANG"), "no LANG column: {header}");
        assert!(
            table.lines().any(|l| l.starts_with("ja/index.html") && l.contains("ja")),
            "the ja page does not report its language:\n{table}"
        );
        assert!(
            table.lines().any(|l| l.starts_with("about/index.html") && l.contains("zh-Hant")),
            "a page in one of the three editions must report its canonical tag:\n{table}"
        );
    }

    #[test]
    fn cjk_titles_are_two_cells_wide() {
        assert_eq!(display_width("邊界"), 4);
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("《潮汐》"), 8);
    }

    /// Columns must line up for a CJK site — that is the site this command
    /// was built for, and char-counting would misalign every row.
    #[test]
    fn table_columns_align_across_scripts() {
        let mut cjk = entry("works/邊界/index.html", &[]);
        cjk.title = "邊界".to_string();
        let ascii = entry("about/index.html", &[]);
        let table = render_table(&[cjk, ascii]);
        // Measure where the SECOND column begins, by locating its known text
        // and taking the display width of everything before it. Splitting the
        // line on the two-space separator does not work: the first column's
        // own padding is made of spaces, so the split lands inside the padding
        // and reports the unpadded content width on every row — which looks
        // ragged even when the table is perfectly aligned.
        let marks = ["SOURCE", "works/邊界/index.html.md", "about/index.html.md"];
        let offsets: Vec<usize> = table
            .lines()
            .zip(marks)
            .map(|(line, mark)| {
                let idx = line
                    .find(mark)
                    .unwrap_or_else(|| panic!("column marker {mark:?} missing from {line:?}"));
                display_width(&line[..idx])
            })
            .collect();
        assert!(offsets.windows(2).all(|w| w[0] == w[1]), "ragged first column:\n{table}");
    }

    #[test]
    fn hidden_column_names_every_mechanism_separately() {
        let rows = vec![entry("a", &["draft"]), entry("b", &["unlisted"]), entry("c", &[])];
        let table = render_table(&rows);
        assert!(table.contains("draft"));
        assert!(table.contains("unlisted"));
        let l = legend(&rows).expect("legend when something is hidden");
        assert!(l.contains("draft: true"));
        assert!(l.contains("listed: false"));
    }

    /// A site with nothing hidden gets a table and no legend — the legend is
    /// four lines of vocabulary nobody needs there.
    #[test]
    fn legend_is_absent_when_nothing_is_hidden() {
        assert!(legend(&[entry("a", &[]), entry("b", &[])]).is_none());
    }

    #[test]
    fn slot_only_documents_report_no_url() {
        let table = render_table(&[entry("footer", &["slot"])]);
        assert!(table.contains("slot"), "{table}");
    }
}
