//! `moss doctor --surface` — a read-only report of markdown constructs that a
//! cleaner form could replace. Reported, never enforced: findings exit 0.
//!
//! ## Classed divs repeated across pages
//!
//! An authored `::: {.eyebrow}` div that appears on many pages is usually a
//! fact about each page (a station number, a date, a series name) that the
//! author restated by hand because nothing else can draw it. The report lists
//! every such class on three or more pages, with the folders it appears in, so
//! the repeated facts can move into frontmatter fields that the theme reads.
//! `--all` lowers the bar to one page, so after a migration the report can say
//! whether any classed div is left at all.
//!
//! The detector runs the real shortcode extractor over each page body rather
//! than matching fence lines itself, so a div inside a code fence or a typed
//! shortcode (`:::grid {.x}`) is never counted.
//!
//! ## Theme rules that style nothing
//!
//! A theme rule whose selectors match no element of the last build's pages;
//! see `theme_rules.rs`.
//!
//! ## Theme selectors keyed on a page address
//!
//! A selector such as `body[data-path^="/housing/"]` in a theme stylesheet
//! stops matching when the page it names is renamed. These are found with a
//! plain line scan, since the stylesheet is CSS and not markdown.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use moss_core::ast::shortcode_extract::{extract_shortcodes_with_config, CssRegionSpan};
use moss_core::ast::ParseConfig;

mod theme_rules;
pub use theme_rules::{ThemeRules, UnmatchedRule};

/// A class must sit on at least this many pages before it is reported, unless `--all`.
const MIN_PAGES: usize = 3;

/// Folder label for pages at the site root.
const ROOT_FOLDER: &str = "(root)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassedDiv {
    pub class: String,
    pub pages: usize,
    /// Folder and the number of this class's pages in it, most pages first.
    pub folders: Vec<(String, usize)>,
    /// Site-relative path of the first page, in sorted order, that uses the class.
    pub example_path: String,
    /// Line of that page's first opener for the class.
    pub example_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressSelector {
    /// Site-relative path of the stylesheet.
    pub path: String,
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceReport {
    /// The fewest pages a reported class sits on: [`MIN_PAGES`], or 1 with `--all`.
    pub min_pages: usize,
    pub classed_divs: Vec<ClassedDiv>,
    pub theme_rules: ThemeRules,
    pub address_selectors: Vec<AddressSelector>,
}

/// Per-class tally while the pages are walked in sorted order.
#[derive(Default)]
struct ClassTally {
    pages: usize,
    folders: BTreeMap<String, usize>,
    example: Option<(String, usize)>,
}

/// Empty-name fenced divs in one page, as the build's extractor sees them.
pub(crate) fn page_css_regions(content: &str) -> Vec<CssRegionSpan> {
    let (body, line_offset) = super::page_body(content);
    let config = ParseConfig {
        warning_line_offset: line_offset,
        ..ParseConfig::default()
    };
    extract_shortcodes_with_config(&body, &config).css_regions
}

/// Site-relative path with forward slashes, e.g. `Prints/Okabe.md`.
fn site_relative(folder: &Path, file: &Path) -> String {
    file.strip_prefix(folder)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

fn folder_of(rel: &str) -> String {
    rel.rsplit_once('/')
        .map_or_else(|| ROOT_FOLDER.to_string(), |(dir, _)| dir.to_string())
}

/// Every page the build reads, with its classed divs tallied per class.
fn classed_divs(folder: &Path, min_pages: usize) -> Vec<ClassedDiv> {
    let mut tallies: BTreeMap<String, ClassTally> = BTreeMap::new();
    for page in super::markdown_files(folder) {
        let Ok(content) = std::fs::read_to_string(&page) else {
            continue;
        };
        let rel = site_relative(folder, &page);
        // Regions arrive in opener order, so `or_insert` keeps the first line.
        let mut first_line: BTreeMap<String, usize> = BTreeMap::new();
        for region in page_css_regions(&content) {
            for class in region.classes {
                first_line.entry(class).or_insert(region.line);
            }
        }
        for (class, line) in first_line {
            let tally = tallies.entry(class).or_default();
            tally.pages += 1;
            *tally.folders.entry(folder_of(&rel)).or_insert(0) += 1;
            if tally.example.is_none() {
                tally.example = Some((rel.clone(), line));
            }
        }
    }

    let mut out: Vec<ClassedDiv> = tallies
        .into_iter()
        .filter(|(_, t)| t.pages >= min_pages)
        .filter_map(|(class, t)| {
            let (example_path, example_line) = t.example?;
            let mut folders: Vec<(String, usize)> = t.folders.into_iter().collect();
            folders.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            Some(ClassedDiv {
                class,
                pages: t.pages,
                folders,
                example_path,
                example_line,
            })
        })
        .collect();
    out.sort_by(|a, b| b.pages.cmp(&a.pages).then_with(|| a.class.cmp(&b.class)));
    out
}

/// Every `.moss/theme/**/*.css` file, as its site-relative path and text, in path order.
fn theme_sheets(folder: &Path) -> Vec<(String, String)> {
    let theme = folder.join(".moss").join("theme");
    let mut sheets: Vec<PathBuf> = walkdir::WalkDir::new(&theme)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("css"))
        .collect();
    sheets.sort();
    sheets
        .into_iter()
        .filter_map(|sheet| Some((site_relative(folder, &sheet), std::fs::read_to_string(&sheet).ok()?)))
        .collect()
}

/// Lines of the theme stylesheets that mention `data-path`.
fn address_selectors(sheets: &[(String, String)]) -> Vec<AddressSelector> {
    let mut out = Vec::new();
    for (rel, content) in sheets {
        for (idx, line) in content.lines().enumerate() {
            if line.contains("data-path") {
                out.push(AddressSelector {
                    path: rel.clone(),
                    line: idx + 1,
                    text: line.trim().to_string(),
                });
            }
        }
    }
    out
}

/// Run the detectors over the site at `folder`, matching theme rules against
/// the pages of its last build. `all` reports a class on any number of pages
/// instead of [`MIN_PAGES`] or more.
pub fn audit(folder: &Path, all: bool) -> SurfaceReport {
    let min_pages = if all { 1 } else { MIN_PAGES };
    let sheets = theme_sheets(folder);
    let built = crate::moss_paths::MossPaths::new(folder).initial_serve_dir();
    SurfaceReport {
        min_pages,
        classed_divs: classed_divs(folder, min_pages),
        theme_rules: theme_rules::check(&sheets, &built),
        address_selectors: address_selectors(&sheets),
    }
}

/// Print the report and return the exit code. Findings exit 0.
pub fn run(folder: &Path, json: bool, all: bool) -> i32 {
    let report = audit(folder, all);
    if json {
        println!("{}", to_json(&report));
    } else {
        print!("{}", render_text(&report));
    }
    0
}

fn to_json(report: &SurfaceReport) -> serde_json::Value {
    let classed: Vec<serde_json::Value> = report
        .classed_divs
        .iter()
        .map(|d| {
            let folders: Vec<serde_json::Value> = d
                .folders
                .iter()
                .map(|(f, n)| serde_json::json!({ "folder": f, "pages": n }))
                .collect();
            serde_json::json!({
                "class": d.class,
                "pages": d.pages,
                "folders": folders,
                "example": { "path": d.example_path, "line": d.example_line },
            })
        })
        .collect();
    let selectors: Vec<serde_json::Value> = report
        .address_selectors
        .iter()
        .map(|s| serde_json::json!({ "path": s.path, "line": s.line, "text": s.text }))
        .collect();
    let theme_rules = match &report.theme_rules {
        ThemeRules::NoBuild => serde_json::json!({ "checked": false }),
        ThemeRules::Checked { pages, unmatched } => {
            let rules: Vec<serde_json::Value> = unmatched
                .iter()
                .map(|r| serde_json::json!({ "path": r.path, "line": r.line, "selector": r.selector, "scripted": r.scripted }))
                .collect();
            serde_json::json!({ "checked": true, "pages": pages, "unmatched": rules })
        }
    };
    serde_json::json!({
        "min_pages": report.min_pages,
        "classed_divs": classed,
        "theme_rules": theme_rules,
        "address_selectors": selectors,
    })
}

fn render_text(report: &SurfaceReport) -> String {
    let mut out = String::new();
    if report.classed_divs.is_empty() {
        if report.min_pages == 1 {
            out.push_str("No page has a classed div.\n");
        } else {
            out.push_str(&format!("No class is repeated on {} or more pages.\n", report.min_pages));
        }
    } else {
        if report.min_pages == 1 {
            out.push_str("Every classed div, by the pages it is on:\n");
        } else {
            out.push_str("Classed divs repeated across pages — if a div holds a fact about its page, it can be a frontmatter field:\n");
        }
        let class_width = report
            .classed_divs
            .iter()
            .map(|d| d.class.chars().count() + 1)
            .max()
            .unwrap_or(0);
        let pages_width = report.classed_divs.iter().map(|d| format!("{} pages", d.pages).len()).max().unwrap_or(0);
        for d in &report.classed_divs {
            let folders: Vec<String> = d
                .folders
                .iter()
                .map(|(f, n)| {
                    let label = if f == ROOT_FOLDER { f.clone() } else { format!("{f}/") };
                    format!("{label} {n}")
                })
                .collect();
            let pages = pages_text(d.pages);
            out.push_str(&format!(
                "  {:<class_width$}  {:<pages_width$}  {}  e.g. {}:{}\n",
                format!(".{}", d.class),
                pages,
                folders.join(", "),
                d.example_path,
                d.example_line,
            ));
        }
    }
    out.push('\n');

    let dead = render_theme_rules(&report.theme_rules, &mut out);
    out.push('\n');

    if report.address_selectors.is_empty() {
        out.push_str("No theme selector is keyed on a page address.\n");
    } else {
        out.push_str("Theme selectors keyed on a page address — they break when the page is renamed:\n");
        let loc_of = |s: &AddressSelector| format!("{}:{}", s.path, s.line);
        let loc_width = report.address_selectors.iter().map(|s| loc_of(s).chars().count()).max().unwrap_or(0);
        for s in &report.address_selectors {
            out.push_str(&format!("  {:<loc_width$}  {}\n", loc_of(s), s.text));
        }
    }
    out.push('\n');

    let kinds = usize::from(!report.classed_divs.is_empty()) + usize::from(dead) + usize::from(!report.address_selectors.is_empty());
    if kinds == 0 {
        out.push_str("Nothing found.\n");
    } else {
        let noun = if kinds == 1 { "kind" } else { "kinds" };
        out.push_str(&format!("{kinds} {noun} of finding. This is a report; nothing was changed.\n"));
    }
    out
}

fn pages_text(n: usize) -> String {
    if n == 1 { "1 page".to_string() } else { format!("{n} pages") }
}

/// The theme-rules section. Returns whether it found a rule that styles nothing.
fn render_theme_rules(rules: &ThemeRules, out: &mut String) -> bool {
    let (pages, unmatched) = match rules {
        ThemeRules::NoBuild => {
            out.push_str("Theme rules were not checked: the site has no build yet. Run `moss build`, then this again.\n");
            return false;
        }
        ThemeRules::Checked { pages, unmatched } => (pages, unmatched),
    };
    let (scripted, dead): (Vec<&UnmatchedRule>, Vec<&UnmatchedRule>) = unmatched.iter().partition(|r| !r.scripted.is_empty());
    let loc = |r: &UnmatchedRule| format!("{}:{}", r.path, r.line);
    let width = unmatched.iter().map(|r| loc(r).chars().count()).max().unwrap_or(0);
    if dead.is_empty() {
        out.push_str(&format!("Every theme rule matches an element in the last build ({}).\n", pages_text(*pages)));
    } else {
        out.push_str(&format!("Theme rules that match nothing in the last build ({}) — they style no element on any page:\n", pages_text(*pages)));
        for r in &dead {
            out.push_str(&format!("  {:<width$}  {}\n", loc(r), r.selector));
        }
    }
    if !scripted.is_empty() {
        out.push_str("Theme rules that match nothing in the built HTML but name something a script mentions — they may apply once it runs:\n");
        for r in &scripted {
            out.push_str(&format!("  {:<width$}  {}  (script mentions {})\n", loc(r), r.selector, r.scripted.join(", ")));
        }
    }
    !dead.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// Three `Prints/` pages with an eyebrow after frontmatter (opener on line
    /// 5), one page with a one-off class, one page whose only eyebrow sits in
    /// a code fence, and a stylesheet with one address-keyed selector on line 4.
    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for name in ["a", "b", "c"] {
            write(
                root,
                &format!("Prints/{name}.md"),
                &format!("---\ntitle: {name}\n---\n\n::: {{.eyebrow}}\nStation\n:::\n"),
            );
        }
        write(root, "once.md", "---\ntitle: o\n---\n\n::: {.once}\nx\n:::\n");
        write(root, "Prints/d.md", "```md\n::: {.eyebrow}\nx\n:::\n```\n");
        write(root, ".moss/theme/style.css", "/* theme */\nbody {}\n\nbody[data-path^=\"/housing/\"] { color: blue; }\n");
        dir
    }

    #[test]
    fn repeated_eyebrow_is_reported_with_its_real_example_line() {
        let dir = fixture();
        let report = audit(dir.path(), false);
        let eyebrow = report.classed_divs.iter().find(|d| d.class == "eyebrow").expect("eyebrow reported");
        assert_eq!(eyebrow.pages, 3);
        assert_eq!(eyebrow.folders, vec![("Prints".to_string(), 3)]);
        assert_eq!(eyebrow.example_path, "Prints/a.md");
        assert_eq!(eyebrow.example_line, 5);
    }

    #[test]
    fn a_class_on_one_page_is_below_the_threshold() {
        let dir = fixture();
        let report = audit(dir.path(), false);
        assert!(report.classed_divs.iter().all(|d| d.class != "once"));
    }

    #[test]
    fn all_reports_a_class_on_one_page_and_still_skips_code_fences() {
        let dir = fixture();
        let report = audit(dir.path(), true);
        let once = report.classed_divs.iter().find(|d| d.class == "once").expect("one-page class reported");
        assert_eq!((once.pages, once.example_path.as_str()), (1, "once.md"));
        let eyebrow = report.classed_divs.iter().find(|d| d.class == "eyebrow").unwrap();
        assert_eq!(eyebrow.pages, 3, "the fenced eyebrow in Prints/d.md is not a div");
        let text = render_text(&report);
        assert!(text.starts_with("Every classed div"));
        assert!(text.contains(" 1 page  "), "{text}");
        assert_eq!(to_json(&report)["min_pages"], 1);
    }

    #[test]
    fn all_on_a_site_without_divs_says_none_is_left() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.md", "plain\n");
        assert!(render_text(&audit(dir.path(), true)).starts_with("No page has a classed div.\n"));
    }

    #[test]
    fn one_address_keyed_selector_is_reported_with_its_line() {
        let dir = fixture();
        let report = audit(dir.path(), false);
        assert_eq!(report.address_selectors.len(), 1);
        let s = &report.address_selectors[0];
        assert_eq!(s.path, ".moss/theme/style.css");
        assert_eq!(s.line, 4);
        assert_eq!(s.text, "body[data-path^=\"/housing/\"] { color: blue; }");
    }

    #[test]
    fn json_output_parses_and_carries_the_findings() {
        let dir = fixture();
        let text = to_json(&audit(dir.path(), false)).to_string();
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(value["classed_divs"][0]["class"], "eyebrow");
        assert_eq!(value["classed_divs"][0]["pages"], 3);
        assert_eq!(value["classed_divs"][0]["folders"][0]["folder"], "Prints");
        assert_eq!(value["classed_divs"][0]["example"]["path"], "Prints/a.md");
        assert_eq!(value["classed_divs"][0]["example"]["line"], 5);
        assert_eq!(value["address_selectors"][0]["path"], ".moss/theme/style.css");
        assert_eq!(value["address_selectors"][0]["line"], 4);
    }

    #[test]
    fn a_theme_rule_no_built_page_matches_is_a_finding() {
        let dir = fixture();
        write(dir.path(), ".moss/build.nosync/staging/index.html", "<html><body><p>x</p></body></html>");
        let report = audit(dir.path(), false);
        let ThemeRules::Checked { pages, unmatched } = &report.theme_rules else { panic!("the staged build was read") };
        assert_eq!(*pages, 1);
        assert_eq!(unmatched.iter().map(|r| (r.line, r.selector.as_str())).collect::<Vec<_>>(), vec![(4, "body[data-path^=\"/housing/\"]")]);
        let text = render_text(&report);
        assert!(text.contains("Theme rules that match nothing in the last build (1 page)"), "{text}");
        assert!(text.ends_with("3 kinds of finding. This is a report; nothing was changed.\n"), "{text}");
        assert_eq!(to_json(&report)["theme_rules"]["unmatched"][0]["line"], 4);
    }

    #[test]
    fn without_a_build_the_theme_rules_are_not_checked() {
        let dir = fixture();
        let report = audit(dir.path(), false);
        assert_eq!(report.theme_rules, ThemeRules::NoBuild);
        assert!(render_text(&report).contains("Theme rules were not checked"));
        assert_eq!(to_json(&report)["theme_rules"]["checked"], false);
    }

    #[test]
    fn empty_site_says_nothing_found() {
        let dir = tempfile::tempdir().unwrap();
        assert!(render_text(&audit(dir.path(), false)).ends_with("Nothing found.\n"));
    }

    #[test]
    fn neither_check_flag_is_a_usage_error() {
        assert_eq!(super::super::run(&[]), 2);
    }
}
