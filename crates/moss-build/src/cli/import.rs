//! CLI entry point for `moss import`.
//!
//! Wires the existing scrape pipeline to a command-line front-end. No Tauri
//! runtime, no AppHandle — progress is printed to stderr.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::vault::import::scrape::run::{import_local_file, scrape_to_folder, ScrapeProgress};
use crate::vault::import::scrape::service::ScrapeConfig;

/// Top-level dispatcher for `moss import …`. Returns a process exit code.
pub fn run(args: &[String]) -> i32 {
    let parsed = match parse_args(args) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("{}", msg);
            print_usage();
            return 1;
        }
    };

    let folder = match resolve_folder(parsed.folder.as_deref()) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("error: {}", msg);
            return 1;
        }
    };

    let urls = match collect_urls(&parsed) {
        Ok(u) => u,
        Err(msg) => {
            eprintln!("error: {}", msg);
            return 1;
        }
    };

    if urls.is_empty() {
        eprintln!("error: no URLs to import");
        print_usage();
        return 1;
    }

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: failed to start runtime: {}", e);
            return 1;
        }
    };

    let mut any_failed = false;
    let mut total_pages = 0usize;
    let mut total_failed_pages = 0usize;

    for url in &urls {
        eprintln!("→ {}", redact_query(url));
        let result = if is_remote_url(url) {
            let mut config = ScrapeConfig::new(url.clone(), folder.clone());
            config.recursive = parsed.recursive;
            runtime.block_on(scrape_to_folder(config, print_progress))
        } else {
            // Local file (MHTML web-archive or plain .html). `file://` is
            // accepted and stripped to a filesystem path.
            let path = url.strip_prefix("file://").unwrap_or(url);
            runtime.block_on(import_local_file(Path::new(path), &folder))
        };

        match result {
            Ok(res) => {
                total_pages += res.total_pages;
                total_failed_pages += res.failed_pages;
                eprintln!(
                    "  ✓ {} page(s) imported, {} failed",
                    res.total_pages, res.failed_pages
                );
                if res.failed_pages > 0 {
                    any_failed = true;
                }
            }
            Err(e) => {
                eprintln!("  ✗ {}", e);
                any_failed = true;
            }
        }
    }

    eprintln!(
        "Done: {} page(s) imported into {} ({} failed)",
        total_pages,
        folder.display(),
        total_failed_pages
    );

    if any_failed && total_pages == 0 {
        1
    } else if any_failed {
        2
    } else {
        0
    }
}

#[derive(Debug, Default)]
struct ParsedArgs {
    url: Option<String>,
    folder: Option<String>,
    list_file: Option<String>,
    recursive: bool,
}

fn parse_args(args: &[String]) -> Result<ParsedArgs, String> {
    let mut out = ParsedArgs::default();
    let mut positional: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--list" => {
                i += 1;
                if i >= args.len() {
                    return Err("--list requires a file path".to_string());
                }
                out.list_file = Some(args[i].clone());
            }
            s if s.starts_with("--list=") => {
                out.list_file = Some(s.strip_prefix("--list=").unwrap_or(s).to_string());
            }
            "-r" | "--recursive" => {
                out.recursive = true;
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            // A `-`-prefixed argument is a flag, never a URL or a folder.
            // Without this arm `--bogus-flag <url>` fell into the positional
            // slots and the error read `folder does not exist: <url>` — the
            // "your argument is wrong" failure `cli::commands::help_for`
            // documents the repo already fixed for six other verbs.
            s if s.starts_with('-') && s != "-" => {
                return Err(format!("unknown option: {}", s));
            }
            _ => positional.push(a.clone()),
        }
        i += 1;
    }

    match (positional.len(), out.list_file.is_some()) {
        (0, false) => return Err("missing URL".to_string()),
        (0, true) => {}
        (1, false) => {
            out.url = Some(positional.remove(0));
        }
        (1, true) => {
            out.folder = Some(positional.remove(0));
        }
        (2, false) => {
            out.url = Some(positional.remove(0));
            out.folder = Some(positional.remove(0));
        }
        (2, true) => {
            return Err("with --list, pass at most one positional argument (the folder)".to_string());
        }
        _ => return Err("too many positional arguments".to_string()),
    }

    Ok(out)
}

fn resolve_folder(arg: Option<&str>) -> Result<PathBuf, String> {
    let folder = match arg {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir()
            .map_err(|e| format!("could not determine current directory: {}", e))?,
    };
    if !folder.exists() {
        return Err(format!("folder does not exist: {}", folder.display()));
    }
    if !folder.is_dir() {
        return Err(format!("not a directory: {}", folder.display()));
    }
    // `vault::paths` owns normalization; the exists/is_dir checks above are this
    // command's UX contract and stay here.
    Ok(crate::vault::paths::resolve_input(&folder))
}

fn collect_urls(parsed: &ParsedArgs) -> Result<Vec<String>, String> {
    let mut urls: Vec<String> = Vec::new();

    if let Some(single) = &parsed.url {
        urls.push(single.clone());
    }

    if let Some(list_path) = &parsed.list_file {
        let body = fs::read_to_string(list_path)
            .map_err(|e| format!("cannot read --list file {}: {}", list_path, e))?;
        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            urls.push(trimmed.to_string());
        }
    }

    Ok(urls)
}

fn print_progress(p: ScrapeProgress) {
    if p.complete {
        return;
    }
    if let Some(url) = &p.current_url {
        let _ = writeln!(
            io::stderr(),
            "    [{}] {}",
            p.pages_scraped + p.pages_failed + 1,
            redact_query(url)
        );
    }
}

/// Strip the query string from a URL before logging — share tokens
/// (`?utm_*`, `?token=…`) and unlisted-share params can leak via a tailed
/// log or pasted bug report. Falls back to the raw URL if parsing fails.
fn redact_query(url: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(url) else {
        return url.to_string();
    };
    if parsed.query().is_none() {
        return url.to_string();
    }
    parsed.set_query(None);
    let s = parsed.to_string();
    format!("{} (query redacted)", s.trim_end_matches('?'))
}

/// Whether an import argument is a remote URL (fetched over HTTP) rather than a
/// local file path. Anything that isn't `http(s)://` is treated as a local file
/// (MHTML web-archive or `.html`).
fn is_remote_url(arg: &str) -> bool {
    let lower = arg.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn print_usage() {
    eprintln!("Usage: moss import <url|file> [<folder>] [-r|--recursive]");
    eprintln!("       moss import --list <file> [<folder>] [-r|--recursive]");
    eprintln!();
    eprintln!("Imports your own content — published on the web or saved as a file —");
    eprintln!("as markdown into <folder> (default: current directory).");
    eprintln!();
    eprintln!("A URL (http/https) is fetched and distilled with a defuddle-based");
    eprintln!("extractor; metadata is read from schema.org, OpenGraph, and <html lang>.");
    eprintln!("A local .mhtml/.mht web-archive (\"Save Page As\") or .html file is parsed");
    eprintln!("offline, with embedded images written alongside the note — useful for");
    eprintln!("login-gated or JS-heavy pages a plain fetch can't reach.");
    eprintln!();
    eprintln!("By default imports only the URL given. Pass --recursive (-r) to walk");
    eprintln!("every in-scope page (same host + path prefix). On filename collisions,");
    eprintln!("the new file is renamed `name 2.md`, `name 3.md`, etc.");
    eprintln!();
    eprintln!("The vault copy is canonical; the source URL is recorded in `syndicated`");
    eprintln!("frontmatter (POSSE), the same field that lets a syndicated comment link");
    eprintln!("back to its origin. Import is for content you have the right to republish.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn is_remote_url_distinguishes_urls_from_local_paths() {
        assert!(is_remote_url("https://book.douban.com/review/8218385/"));
        assert!(is_remote_url("http://example.com"));
        assert!(is_remote_url("  HTTPS://Example.com  "));
        assert!(!is_remote_url("/Users/me/Downloads/note.mhtml"));
        assert!(!is_remote_url("relative/page.html"));
        assert!(!is_remote_url("file:///Users/me/note.mhtml"));
    }

    #[test]
    fn parses_single_url_no_folder() {
        let p = parse_args(&s(&["https://example.com/"])).unwrap();
        assert_eq!(p.url.as_deref(), Some("https://example.com/"));
        assert!(p.folder.is_none());
        assert!(p.list_file.is_none());
    }

    #[test]
    fn parses_url_and_folder() {
        let p = parse_args(&s(&["https://example.com/", "/tmp/out"])).unwrap();
        assert_eq!(p.url.as_deref(), Some("https://example.com/"));
        assert_eq!(p.folder.as_deref(), Some("/tmp/out"));
    }

    #[test]
    fn parses_list_long_flag() {
        let p = parse_args(&s(&["--list", "urls.txt"])).unwrap();
        assert_eq!(p.list_file.as_deref(), Some("urls.txt"));
        assert!(p.url.is_none());
    }

    #[test]
    fn parses_list_with_folder() {
        let p = parse_args(&s(&["--list", "urls.txt", "/tmp/out"])).unwrap();
        assert_eq!(p.list_file.as_deref(), Some("urls.txt"));
        assert_eq!(p.folder.as_deref(), Some("/tmp/out"));
    }

    #[test]
    fn parses_list_equals_form() {
        let p = parse_args(&s(&["--list=urls.txt"])).unwrap();
        assert_eq!(p.list_file.as_deref(), Some("urls.txt"));
    }

    #[test]
    fn rejects_no_args() {
        assert!(parse_args(&s(&[])).is_err());
    }

    #[test]
    fn rejects_too_many_positional() {
        assert!(parse_args(&s(&["a", "b", "c"])).is_err());
    }

    #[test]
    fn rejects_url_with_list() {
        assert!(parse_args(&s(&["https://x.example/", "--list", "u.txt", "/tmp"])).is_err());
    }

    #[test]
    fn collect_urls_from_list_skips_blanks_and_comments() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("urls.txt");
        std::fs::write(
            &p,
            "https://a.example/\n\n# a comment\nhttps://b.example/post\n  \n",
        )
        .unwrap();

        let parsed = ParsedArgs {
            url: None,
            folder: None,
            list_file: Some(p.to_string_lossy().to_string()),
            recursive: false,
        };
        let urls = collect_urls(&parsed).unwrap();
        assert_eq!(
            urls,
            vec![
                "https://a.example/".to_string(),
                "https://b.example/post".to_string(),
            ]
        );
    }

    /// The `-`-prefixed arm. Without it `--bogus <url>` fell into a positional
    /// slot and the error read `folder does not exist: <url>`.
    #[test]
    fn an_unknown_option_is_refused_rather_than_read_as_a_path() {
        let args = ["--bogus".to_string(), "https://x.example/".to_string()];
        let err = parse_args(&args).expect_err("an unknown option must not parse");
        assert!(err.contains("--bogus"), "the message must name it: {err}");
    }
}
