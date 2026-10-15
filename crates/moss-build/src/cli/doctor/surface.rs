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
//!
//! The detector runs the real shortcode extractor over each page body rather
//! than matching fence lines itself, so a div inside a code fence or a typed
//! shortcode (`:::grid {.x}`) is never counted.
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

/// A class must sit on at least this many pages before it is reported.
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
    pub classed_divs: Vec<ClassedDiv>,
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
fn page_css_regions(content: &str) -> Vec<CssRegionSpan> {
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
fn classed_divs(folder: &Path) -> Vec<ClassedDiv> {
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
        .filter(|(_, t)| t.pages >= MIN_PAGES)
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

/// Lines of every `.moss/theme/**/*.css` file that mention `data-path`.
fn address_selectors(folder: &Path) -> Vec<AddressSelector> {
    let theme = folder.join(".moss").join("theme");
    let mut sheets: Vec<PathBuf> = walkdir::WalkDir::new(&theme)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("css"))
        .collect();
    sheets.sort();

    let mut out = Vec::new();
    for sheet in sheets {
        let Ok(content) = std::fs::read_to_string(&sheet) else {
            continue;
        };
        let rel = site_relative(folder, &sheet);
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

/// Run both detectors over the site at `folder`.
pub fn audit(folder: &Path) -> SurfaceReport {
    SurfaceReport {
        classed_divs: classed_divs(folder),
        address_selectors: address_selectors(folder),
    }
}

/// Print the report and return the exit code. Findings exit 0.
pub fn run(folder: &Path, json: bool) -> i32 {
    let report = audit(folder);
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
    serde_json::json!({ "classed_divs": classed, "address_selectors": selectors })
}

fn render_text(report: &SurfaceReport) -> String {
    let mut out = String::new();
    if report.classed_divs.is_empty() {
        out.push_str(&format!("No class is repeated on {MIN_PAGES} or more pages.\n"));
    } else {
        out.push_str("Classed divs repeated across pages — if a div holds a fact about its page, it can be a frontmatter field:\n");
        let class_width = report
            .classed_divs
            .iter()
            .map(|d| d.class.chars().count() + 1)
            .max()
            .unwrap_or(0);
        for d in &report.classed_divs {
            let folders: Vec<String> = d
                .folders
                .iter()
                .map(|(f, n)| {
                    let label = if f == ROOT_FOLDER { f.clone() } else { format!("{f}/") };
                    format!("{label} {n}")
                })
                .collect();
            out.push_str(&format!(
                "  {:<class_width$}  {} pages  {}  e.g. {}:{}\n",
                format!(".{}", d.class),
                d.pages,
                folders.join(", "),
                d.example_path,
                d.example_line,
            ));
        }
    }
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

    let kinds = usize::from(!report.classed_divs.is_empty()) + usize::from(!report.address_selectors.is_empty());
    if kinds == 0 {
        out.push_str("Nothing found.\n");
    } else {
        let noun = if kinds == 1 { "kind" } else { "kinds" };
        out.push_str(&format!("{kinds} {noun} of finding. This is a report; nothing was changed.\n"));
    }
    out
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
        let report = audit(dir.path());
        let eyebrow = report.classed_divs.iter().find(|d| d.class == "eyebrow").expect("eyebrow reported");
        assert_eq!(eyebrow.pages, 3);
        assert_eq!(eyebrow.folders, vec![("Prints".to_string(), 3)]);
        assert_eq!(eyebrow.example_path, "Prints/a.md");
        assert_eq!(eyebrow.example_line, 5);
    }

    #[test]
    fn a_class_on_one_page_is_below_the_threshold() {
        let dir = fixture();
        let report = audit(dir.path());
        assert!(report.classed_divs.iter().all(|d| d.class != "once"));
    }

    #[test]
    fn one_address_keyed_selector_is_reported_with_its_line() {
        let dir = fixture();
        let report = audit(dir.path());
        assert_eq!(report.address_selectors.len(), 1);
        let s = &report.address_selectors[0];
        assert_eq!(s.path, ".moss/theme/style.css");
        assert_eq!(s.line, 4);
        assert_eq!(s.text, "body[data-path^=\"/housing/\"] { color: blue; }");
    }

    #[test]
    fn json_output_parses_and_carries_the_findings() {
        let dir = fixture();
        let text = to_json(&audit(dir.path())).to_string();
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
    fn empty_site_says_nothing_found() {
        let dir = tempfile::tempdir().unwrap();
        assert!(render_text(&audit(dir.path())).ends_with("Nothing found.\n"));
    }

    #[test]
    fn neither_check_flag_is_a_usage_error() {
        assert_eq!(super::super::run(&[]), 2);
    }
}
