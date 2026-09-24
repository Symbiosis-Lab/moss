//! `moss doctor` — read-only audits of a vault, reported never enforced.
//!
//! v1 ships exactly one check, `--math`.
//!
//! ## Why `--math` exists
//!
//! `[site].math` defaults ON, which means pulldown-cmark's `ENABLE_MATH` is
//! live for every vault that never mentions math in its config. That is safe
//! for the vaults we measured (zero `$` outside fenced code in any published
//! vault), but pulldown's open/close rule is an ASCII byte test:
//! **any non-whitespace byte before the closing `$` closes the span.** So
//! prose that merely quotes two prices becomes an equation:
//!
//! ```text
//! (cost: $5) and ($10)     →  one inline-math span
//! 一个$5，两个$10            →  one inline-math span (no spaces in CJK prose)
//! ```
//!
//! An author importing a corpus we never measured — a shop's price list, a
//! translated finance post — has no way to know this before publishing. This
//! command is that way: it reports every `$`-span that the real parser would
//! turn into math, with `path:line:` locations, so the author can audit the
//! vault and choose a remedy: escape as `\$` (renders as a plain `$`, so the
//! prose is untouched — the recommended fix), add a space after the `$`
//! (changes what readers see), or set `math = false` for the whole site.
//!
//! Note the report says "parsed as math", not "typeset": P1 ships no
//! typesetting engine, so a reported span renders as its LaTeX source with
//! the `$` delimiters consumed.
//!
//! ## Why it runs the real parser
//!
//! A hand-rolled `$` regex would be a *second* implementation of the
//! delimiter rules, and it would disagree with the build in exactly the
//! confusing cases (CJK, escaped `$`, code spans, fenced blocks) — the ones
//! the author most needs a straight answer about. So the scan builds
//! `moss_core::ast::parser_options(true)` (the same constructor the build
//! renderer uses) and reads pulldown's own `InlineMath`/`DisplayMath` events.
//! When pulldown's rules change, this report changes with them for free.
//!
//! It is a report, not a gate: findings exit 0. Only a usage error exits
//! non-zero.

use pulldown_cmark::{Event, Parser};
use std::path::{Path, PathBuf};

/// One `$`-span that the real parser would turn into math.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathSpan {
    /// 1-indexed line in the file, counted from byte 0 of the whole file
    /// (frontmatter included) so the number matches what an editor shows.
    pub line: usize,
    /// `$$…$$` rather than `$…$`.
    pub display: bool,
    /// The source span as written, delimiters included, newlines collapsed
    /// so one finding stays one line of report.
    pub source: String,
}

/// Longest span echoed in the report before an ellipsis. A price-list false
/// positive is short; a real equation can be a screenful and would bury the
/// findings around it.
const MAX_SOURCE_ECHO: usize = 120;

/// Find every span in `content` that the build's parser would parse as math.
///
/// The pure core of `--math`: no I/O, so the delimiter behavior is testable
/// without a vault on disk.
///
/// Frontmatter is excluded — the build never renders it as markdown, so a `$`
/// in a `price:` field is not a finding. Line numbers still count from the top
/// of the file.
pub fn scan_markdown(content: &str) -> Vec<MathSpan> {
    // Normalize CRLF exactly as `frontmatter::parse` does, so the offsets it
    // reports index into the same bytes we slice and count newlines in.
    let normalized: std::borrow::Cow<'_, str> = if content.contains("\r\n") {
        std::borrow::Cow::Owned(content.replace("\r\n", "\n"))
    } else {
        std::borrow::Cow::Borrowed(content)
    };

    let parsed = moss_core::frontmatter::parse(&normalized);
    let body_offset = parsed.frontmatter_range.map_or(0, |(_, end)| end);
    let Some(body) = normalized.get(body_offset..) else {
        return Vec::new();
    };

    let options = moss_core::ast::parser_options(true);

    let mut spans = Vec::new();
    for (event, range) in Parser::new_ext(body, options).into_offset_iter() {
        // The two math events are the whole point of this scan; every other
        // event is someone else's concern.
        let display = match event {
            Event::InlineMath(_) => false,
            Event::DisplayMath(_) => true,
            _ => continue,
        };

        let start = body_offset + range.start;
        let end = body_offset + range.end;
        let Some(source) = normalized.get(start..end) else {
            continue;
        };

        spans.push(MathSpan {
            line: line_of(&normalized, start),
            display,
            source: condense(source),
        });
    }
    spans
}

/// 1-indexed line number of `offset` within `content`.
fn line_of(content: &str, offset: usize) -> usize {
    content
        .get(..offset)
        .map_or(1, |prefix| prefix.bytes().filter(|b| *b == b'\n').count() + 1)
}

/// Collapse a source span to one readable line: whitespace runs become single
/// spaces, and anything past `MAX_SOURCE_ECHO` chars becomes an ellipsis.
fn condense(source: &str) -> String {
    let flattened: String = source.split_whitespace().collect::<Vec<_>>().join(" ");
    if flattened.chars().count() <= MAX_SOURCE_ECHO {
        return flattened;
    }
    let head: String = flattened.chars().take(MAX_SOURCE_ECHO).collect();
    format!("{head}…")
}

/// Entry point for `moss doctor`. Returns an exit code.
pub fn run(args: &[String]) -> i32 {
    if !args.iter().any(|a| a == "--math") {
        print_usage();
        // 2, not 1: "you asked for a check that does not exist" is a usage
        // error, distinct from a check that ran and failed. Findings exit 0.
        return 2;
    }

    let folder = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .map_or_else(
            || std::env::current_dir().unwrap_or_default(),
            crate::vault_root::resolve_input,
        );

    if !folder.is_dir() {
        eprintln!("Error: not a folder: {}", folder.display());
        return 1;
    }

    let mut files_with_math = 0usize;
    let mut total_spans = 0usize;

    for path in markdown_files(&folder) {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let spans = scan_markdown(&content);
        if spans.is_empty() {
            continue;
        }
        files_with_math += 1;
        total_spans += spans.len();

        let shown = path.strip_prefix(&folder).unwrap_or(&path).display();
        for span in spans {
            println!("{}:{}: {}", shown, span.line, span.source);
        }
    }

    report_summary(total_spans, files_with_math);
    0
}

/// Print the closing summary. Split out so the two outcomes read as one
/// decision rather than a branch buried at the end of `run`.
fn report_summary(total_spans: usize, files_with_math: usize) {
    if total_spans == 0 {
        println!("No math spans found — `$` in this vault is safe to leave as-is.");
        return;
    }

    println!();
    println!(
        "{} math span(s) in {} file(s) would be parsed as math.",
        total_spans, files_with_math
    );
    println!("(moss does not typeset math yet — these render as their LaTeX source.)");
    println!();
    println!("If any of these is ordinary prose (prices, currency, shell variables),");
    println!("escape the dollar signs — this is the only fix that leaves your");
    println!("prose exactly as written, since `\\$` renders as a plain `$`:");
    println!();
    println!("    一个\\$5，两个\\$10");
    println!();
    println!("Putting a space after the `$` also works, but it changes the text your");
    println!("readers see. Or turn math off for the whole site:");
    println!();
    println!("    # .moss/config.toml");
    println!("    [site]");
    println!("    math = false");
}

/// Every markdown file the build would read, using the build's own directory
/// exclusions (`is_excluded_dir_name` prunes `.moss`, `.git`, dotfiles,
/// `node_modules`) so the report covers exactly the files that get published.
fn markdown_files(folder: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(folder)
        .into_iter()
        .filter_entry(|e| {
            if !e.file_type().is_dir() {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            !crate::build::scan::classify::is_excluded_dir_name(&name)
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("md") | Some("markdown")
            )
        })
        .collect();

    // WalkDir order is filesystem order; sort so two runs over the same vault
    // produce diffable output.
    files.sort();
    files
}

fn print_usage() {
    eprintln!("Usage: moss doctor --math [<folder>]");
    eprintln!();
    eprintln!("Reports every `$`-span in the vault that moss would parse as math.");
    eprintln!("Run it before publishing an imported corpus: pulldown's delimiter rule");
    eprintln!("closes a span at any non-whitespace byte, so `一个$5，两个$10` and");
    eprintln!("`(cost: $5) and ($10)` both become equations.");
    eprintln!();
    eprintln!("moss does not typeset math yet — a span reported here renders as its");
    eprintln!("LaTeX source, and its `$` delimiters are consumed. Fix false positives");
    eprintln!("by escaping (`\\$`, which renders as a plain `$` and leaves your prose");
    eprintln!("unchanged), by adding a space after the `$`, or with `math = false`.");
    eprintln!();
    eprintln!("<folder> defaults to the current directory. Findings exit 0 — this is a");
    eprintln!("report, not a gate.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_prose_has_no_math() {
        assert!(scan_markdown("Just some prose with no dollar signs.\n").is_empty());
    }

    #[test]
    fn inline_math_is_found_with_its_line() {
        let spans = scan_markdown("# Title\n\nEnergy $E = mc^2$ is famous.\n");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].line, 3);
        assert!(!spans[0].display);
        assert_eq!(spans[0].source, "$E = mc^2$");
    }

    #[test]
    fn display_math_is_flagged_as_display() {
        let spans = scan_markdown("intro\n\n$$\na + b\n$$\n");
        assert_eq!(spans.len(), 1);
        assert!(spans[0].display);
        assert_eq!(spans[0].line, 3);
        // Newlines collapsed so the report stays one line per finding.
        assert_eq!(spans[0].source, "$$ a + b $$");
    }

    /// The false positive this whole command exists to surface. Unspaced CJK
    /// prose quoting two prices parses as one inline-math span, because
    /// pulldown's close rule only requires a non-whitespace byte before `$`.
    #[test]
    fn unspaced_cjk_prices_are_reported() {
        let spans = scan_markdown("一个$5，两个$10\n");
        assert_eq!(spans.len(), 1, "expected the CJK price span to parse as math");
        assert_eq!(spans[0].source, "$5，两个$");
    }

    /// The ASCII sibling of the CJK case above. Same rule, same finding.
    #[test]
    fn parenthesized_prices_are_reported() {
        let spans = scan_markdown("(cost: $5) and ($10)\n");
        assert_eq!(spans.len(), 1);
        assert!(!spans[0].display);
    }

    /// Fenced code is why every published vault measured clean. If this ever
    /// regresses, the report floods with shell snippets and authors stop
    /// reading it.
    #[test]
    fn fenced_code_is_never_math() {
        let md = "```sh\nexport A=$HOME\necho $PATH and $USER\n```\n";
        assert!(scan_markdown(md).is_empty());
    }

    #[test]
    fn code_spans_are_never_math() {
        assert!(scan_markdown("Set `$HOME` and `$PATH` first.\n").is_empty());
    }

    /// Frontmatter is not markdown — the build never renders it, so a `$` in a
    /// YAML value is not a finding. Line numbers still count from byte 0.
    #[test]
    fn frontmatter_is_excluded_but_lines_stay_absolute() {
        let md = "---\ntitle: Costs $5 to $10\n---\n\nBody $x$ here.\n";
        let spans = scan_markdown(md);
        assert_eq!(spans.len(), 1, "the frontmatter `$…$` must not be reported");
        assert_eq!(spans[0].source, "$x$");
        assert_eq!(spans[0].line, 5, "line must count the frontmatter lines");
    }

    #[test]
    fn crlf_files_report_the_same_lines_as_lf() {
        let lf = scan_markdown("a\n\nEnergy $E$ here.\n");
        let crlf = scan_markdown("a\r\n\r\nEnergy $E$ here.\r\n");
        assert_eq!(lf, crlf);
    }

    #[test]
    fn long_equations_are_truncated_in_the_report() {
        let long = "x".repeat(400);
        let spans = scan_markdown(&format!("${long}$\n"));
        assert_eq!(spans.len(), 1);
        assert!(spans[0].source.ends_with('…'));
        assert_eq!(spans[0].source.chars().count(), MAX_SOURCE_ECHO + 1);
    }

    /// Every finding must be a distinct location, not a repeated one — a
    /// per-file report is only auditable if the lines are right.
    #[test]
    fn multiple_findings_carry_distinct_lines() {
        let spans = scan_markdown("$a$\n\n$b$\n\n$c$\n");
        let lines: Vec<usize> = spans.iter().map(|s| s.line).collect();
        assert_eq!(lines, vec![1, 3, 5]);
    }

    #[test]
    fn missing_math_flag_is_a_usage_error() {
        assert_eq!(run(&[]), 2);
        assert_eq!(run(&["--links".to_string()]), 2);
    }
}
