//! Sync test: every pulldown-cmark walker in this crate must be math-aware —
//! it either handles `Event::InlineMath` / `Event::DisplayMath`, or it
//! declares with `// allow:math-events-ignored <reason>` that ignoring them
//! is correct.
//!
//! Open-half twin of `src-tauri/tests/math_wiring_invariant_test.rs`
//! (desktop repo) — that file's `SOURCE_ROOTS` scans `src-tauri/src`,
//! `open/crates/moss-core/src`, `open/crates/moss-build/src` independently
//! for the same pattern (class B per
//! docs/archive/2026-09-16-boundary-gates-remeasured-for-dependency-model.md).
//! This is moss-core's copy, scoped to this crate's own `src`; moss-build
//! carries the twin for its own `src`.
//!
//! Also carries `shortcode_sub_parses_use_the_callers_config`, which the plan
//! doc calls out as reading only `open/crates/moss-core/src/ast/shortcode_extract.rs`
//! — wholly this crate's own file — and moves here outright rather than
//! staying split.
//!
//! ## Why this exists
//!
//! Turning on `Options::ENABLE_MATH` does not make math render. It makes
//! pulldown emit two NEW leaf events, `InlineMath` and `DisplayMath`. A walker
//! that does not match them does not fall back to the raw text — the events
//! carry the equation and nothing else does, so an unmatched arm **deletes the
//! author's content**.
//!
//! See docs/decisions/ADR-030-latex-math-rendering.md.

use std::fs;
use std::path::{Path, PathBuf};

const SOURCE_ROOTS: &[&str] = &["src"];

const WALKER_PATTERNS: &[&str] = &["Parser::new_ext("];
const MATH_ENABLING_PATTERNS: &[&str] = &["Options::ENABLE_MATH", "parser_options(true)"];
const MATH_EVENT_PATTERNS: &[&str] = &["InlineMath", "DisplayMath"];
const ALLOW_MARKER: &str = "allow:math-events-ignored";

fn is_code_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    !(trimmed.starts_with("//") || trimmed.starts_with("*"))
}

fn math_relevant(content: &str) -> bool {
    content.lines().filter(|l| is_code_line(l)).any(|line| {
        WALKER_PATTERNS.iter().any(|p| line.contains(p))
            || MATH_ENABLING_PATTERNS.iter().any(|p| line.contains(p))
    })
}

fn handles_math_events(content: &str) -> bool {
    MATH_EVENT_PATTERNS.iter().all(|p| content.contains(p))
}

#[test]
fn every_pulldown_walker_is_math_aware() {
    let repo_root = repo_root();

    let mut violations: Vec<(PathBuf, String)> = Vec::new();

    for root in SOURCE_ROOTS {
        let root_path = repo_root.join(root);
        assert!(
            root_path.exists(),
            "source root {} not found — did the repo layout move?",
            root_path.display()
        );

        walk_rust_files(&root_path, &mut |path| {
            let Ok(content) = fs::read_to_string(path) else {
                return;
            };
            if !math_relevant(&content) {
                return;
            }
            if handles_math_events(&content) || content.contains(ALLOW_MARKER) {
                return;
            }
            let missing: Vec<&str> = MATH_EVENT_PATTERNS
                .iter()
                .filter(|p| !content.contains(**p))
                .copied()
                .collect();
            violations.push((path.to_path_buf(), missing.join(" + ")));
        });
    }

    if !violations.is_empty() {
        let mut msg = String::from(
            "Found pulldown-cmark walker(s) that neither handle math events nor declare\n\
             that ignoring them is correct. See docs/decisions/ADR-030-latex-math-rendering.md.\n\nViolations:\n",
        );
        for (path, missing) in &violations {
            let rel = path.strip_prefix(&repo_root).unwrap_or(path).display();
            msg.push_str(&format!("  - {rel}: no handling for {missing}\n"));
        }
        panic!("{msg}");
    }
}

/// The exemption must stay a deliberate, reasoned declaration.
#[test]
fn the_only_exemption_is_the_one_we_reasoned_about() {
    let repo_root = repo_root();

    let mut marked: Vec<String> = Vec::new();
    for root in SOURCE_ROOTS {
        walk_rust_files(&repo_root.join(root), &mut |path| {
            let Ok(content) = fs::read_to_string(path) else {
                return;
            };
            if content.contains(ALLOW_MARKER) {
                let rel = path.strip_prefix(&repo_root).unwrap_or(path);
                marked.push(rel.display().to_string().replace('\\', "/"));
            }
        });
    }
    marked.sort();

    assert_eq!(
        marked,
        vec![
            // The typed AST parser: three match blocks pinned by tests that
            // would fail if the claim were false
            // (display_math_block_survives_on_its_own_lines,
            // math_survives_inside_list_items,
            // math_survives_inside_a_table_cell); the code-fence and
            // HTML-block collectors are blind because pulldown emits no
            // math event inside either construct.
            "src/ast/parser.rs".to_string(),
        ],
        "The set of math-blind walkers in this crate changed. Adding one is a real decision — \
         confirm the code inspects event kinds for control flow or side effects and the \
         payload survives elsewhere, then update this list in the same commit as the marker."
    );
}

// ---------------------------------------------------------------------------
// Match-block granularity
// ---------------------------------------------------------------------------

const PULLDOWN_IMPORT: &str = "pulldown_cmark";

struct MatchBlock {
    line: usize,
    own: String,
    body: std::ops::Range<usize>,
    kw: usize,
}

fn blank_non_code(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0usize;
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
        for k in from..to.min(out.len()) {
            if out[k] != b'\n' {
                out[k] = b' ';
            }
        }
    };
    while i < b.len() {
        match b[i] {
            b'/' if i + 1 < b.len() && b[i + 1] == b'/' => {
                let end = b[i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(b.len(), |p| i + p);
                blank(&mut out, i, end);
                i = end;
            }
            b'/' if i + 1 < b.len() && b[i + 1] == b'*' => {
                let start = i;
                let mut depth = 1usize;
                i += 2;
                while i < b.len() && depth > 0 {
                    if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
                        depth += 1;
                        i += 2;
                    } else if b[i] == b'*' && i + 1 < b.len() && b[i + 1] == b'/' {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                blank(&mut out, start, i);
            }
            b'r' | b'b' if !(i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) => {
                let mut j = i;
                if b[j] == b'b' {
                    j += 1;
                    if j >= b.len() || b[j] != b'r' {
                        i += 1;
                        continue;
                    }
                }
                j += 1;
                let hash_start = j;
                while j < b.len() && b[j] == b'#' {
                    j += 1;
                }
                if j >= b.len() || b[j] != b'"' {
                    i += 1;
                    continue;
                }
                let hashes = j - hash_start;
                let start = i;
                j += 1;
                while j < b.len() {
                    if b[j] == b'"'
                        && b[j + 1..]
                            .iter()
                            .take(hashes)
                            .filter(|&&c| c == b'#')
                            .count()
                            == hashes
                    {
                        j += 1 + hashes;
                        break;
                    }
                    j += 1;
                }
                blank(&mut out, start, j);
                i = j;
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() {
                    match b[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                blank(&mut out, start, i);
            }
            b'\'' => {
                let is_escape = i + 1 < b.len() && b[i + 1] == b'\\';
                let is_simple = i + 2 < b.len() && b[i + 2] == b'\'';
                if is_escape || is_simple {
                    let start = i;
                    i += 1;
                    while i < b.len() {
                        match b[i] {
                            b'\\' => i += 2,
                            b'\'' => {
                                i += 1;
                                break;
                            }
                            _ => i += 1,
                        }
                    }
                    blank(&mut out, start, i);
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    String::from_utf8(out).expect("blanking only replaces whole tokens with ASCII spaces")
}

fn blank_matches_macro(own: &mut [u8]) {
    let mut i = 0usize;
    while i + 9 <= own.len() {
        if &own[i..i + 9] == b"matches!(" {
            let mut depth = 0i32;
            let mut j = i + 8;
            while j < own.len() {
                match own[j] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            for k in i..=j.min(own.len() - 1) {
                if own[k] != b'\n' {
                    own[k] = b' ';
                }
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
}

fn find_match_blocks(sanitized: &str) -> Vec<MatchBlock> {
    let b = sanitized.as_bytes();
    let mut raw: Vec<(usize, std::ops::Range<usize>)> = Vec::new();

    let mut search = 0usize;
    while let Some(rel) = sanitized[search..].find("match") {
        let kw = search + rel;
        search = kw + 5;
        let before_ok = kw == 0 || !(b[kw - 1].is_ascii_alphanumeric() || b[kw - 1] == b'_');
        let after = b.get(kw + 5).copied().unwrap_or(b' ');
        if !before_ok || after.is_ascii_alphanumeric() || after == b'_' || after == b'!' {
            continue;
        }
        let (mut paren, mut bracket) = (0i32, 0i32);
        let mut j = kw + 5;
        let mut open = None;
        while j < b.len() {
            match b[j] {
                b'(' => paren += 1,
                b')' => paren -= 1,
                b'[' => bracket += 1,
                b']' => bracket -= 1,
                b'{' if paren == 0 && bracket == 0 => {
                    open = Some(j);
                    break;
                }
                b';' if paren == 0 && bracket == 0 => break,
                _ => {}
            }
            j += 1;
        }
        let Some(open) = open else { continue };
        let mut depth = 0i32;
        let mut k = open;
        let mut close = None;
        while k < b.len() {
            match b[k] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(k);
                        break;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        let Some(close) = close else { continue };
        raw.push((kw, open..close + 1));
    }

    raw.iter()
        .map(|(kw, body)| {
            let mut own: Vec<u8> = sanitized[body.clone()].as_bytes().to_vec();
            for (_, other) in &raw {
                if other.start > body.start && other.end <= body.end {
                    for k in other.start..other.end {
                        let idx = k - body.start;
                        if own[idx] != b'\n' {
                            own[idx] = b' ';
                        }
                    }
                }
            }
            blank_matches_macro(&mut own);
            MatchBlock {
                line: sanitized[..*kw].matches('\n').count() + 1,
                own: String::from_utf8(own).expect("blanking preserves UTF-8 boundaries"),
                body: body.clone(),
                kw: *kw,
            }
        })
        .collect()
}

fn block_is_marked(original: &str, block: &MatchBlock) -> bool {
    let mut window_start = original[..block.kw].rfind('\n').map_or(0, |p| p);
    for _ in 0..3 {
        window_start = original[..window_start].rfind('\n').unwrap_or(0);
    }
    original[window_start..block.body.end].contains(ALLOW_MARKER)
}

#[test]
fn every_event_match_arm_set_is_math_aware() {
    let repo_root = repo_root();
    let mut violations: Vec<(PathBuf, usize, String)> = Vec::new();
    let mut scanned_files = 0usize;

    for root in SOURCE_ROOTS {
        walk_rust_files(&repo_root.join(root), &mut |path| {
            let Ok(content) = fs::read_to_string(path) else {
                return;
            };
            if !content.contains(PULLDOWN_IMPORT) {
                return;
            }
            scanned_files += 1;
            let sanitized = blank_non_code(&content);
            for block in find_match_blocks(&sanitized) {
                if !block.own.contains("Event::") {
                    continue;
                }
                if MATH_EVENT_PATTERNS.iter().all(|p| block.own.contains(p)) {
                    continue;
                }
                if block_is_marked(&content, &block) {
                    continue;
                }
                let missing: Vec<&str> = MATH_EVENT_PATTERNS
                    .iter()
                    .filter(|p| !block.own.contains(**p))
                    .copied()
                    .collect();
                let arms: Vec<&str> = block
                    .own
                    .lines()
                    .filter_map(|l| {
                        let t = l.trim();
                        (t.contains("Event::") || t.starts_with("_ =>")).then_some(t)
                    })
                    .take(6)
                    .collect();
                violations.push((
                    path.to_path_buf(),
                    block.line,
                    format!(
                        "missing {} | arms: {}",
                        missing.join(" + "),
                        arms.join("  ")
                    ),
                ));
            }
        });
    }

    assert!(
        scanned_files >= 3,
        "only {scanned_files} pulldown-cmark file(s) scanned — the gate on \
         `{PULLDOWN_IMPORT}` or the source root must have drifted."
    );

    if !violations.is_empty() {
        let mut msg = String::from(
            "Found `match` arm set(s) over pulldown events that neither handle math nor declare\n\
             that ignoring it is correct. See docs/decisions/ADR-030-latex-math-rendering.md.\n\nViolations:\n",
        );
        for (path, line, detail) in &violations {
            let rel = path.strip_prefix(&repo_root).unwrap_or(path).display();
            msg.push_str(&format!("  - {rel}:{line}: {detail}\n"));
        }
        panic!("{msg}");
    }
}

/// The scanner is the load-bearing part of the test above; if `blank_non_code`
/// or the nesting subtraction breaks, the test goes quietly green.
#[test]
fn the_block_scanner_reads_rust_the_way_it_claims_to() {
    let src = r#"
fn outer(events: &[Event]) {
    match &events[0] {
        Event::InlineMath(t) => a(t),
        Event::DisplayMath(t) => b(t),
        Event::Start(_) => {
            match &events[1] {
                Event::Text(t) => alt.push_str(t),
                _ => {}
            }
        }
        _ => {}
    }
}
"#;
    let blocks = find_match_blocks(&blank_non_code(src));
    let event_blocks: Vec<_> = blocks
        .iter()
        .filter(|b| b.own.contains("Event::"))
        .collect();
    assert_eq!(
        event_blocks.len(),
        2,
        "parent and nested match must both be seen"
    );
    assert!(
        event_blocks.iter().any(|b| !b.own.contains("InlineMath")),
        "the nested arm set must NOT inherit its parent's math arms"
    );

    let noise = r#"
fn f() {
    // match &events[0] { Event::Text(t) => {} }
    let s = "match x { Event::Text => } {{{";
    let c = '{';
}
"#;
    assert!(
        find_match_blocks(&blank_non_code(noise)).is_empty(),
        "a match in a comment or string is not a match"
    );

    let m = "fn f() { if matches!(e, Event::Text(_)) { g(); } }";
    assert!(find_match_blocks(&blank_non_code(m)).is_empty());

    let tag_match = r#"
fn f(tag: Tag) {
    match tag {
        Tag::Emphasis => scan(|e| matches!(e, Event::End(TagEnd::Emphasis))),
        _ => (None, 1),
    }
}
"#;
    assert!(
        find_match_blocks(&blank_non_code(tag_match))
            .iter()
            .all(|b| !b.own.contains("Event::")),
        "a `matches!` in an arm body must not make a `match tag` look like an event walker"
    );
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn walk_rust_files(dir: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name.starts_with('.') {
                continue;
            }
            walk_rust_files(&path, visit);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with("_tests.rs") {
                continue;
            }
            visit(&path);
        }
    }
}

/// Shortcode bodies are sub-parsed, so a `ParseConfig` that stops at the
/// shortcode boundary does not stop being observable — it renders one page
/// in two dialects. The only durable guard is to make the bare call
/// unwritable.
///
/// Wholly this crate's own file — moved here outright from
/// `math_wiring_invariant_test.rs` (desktop repo) rather than staying split.
#[test]
fn shortcode_sub_parses_use_the_callers_config() {
    let path = repo_root().join("src/ast/shortcode_extract.rs");
    let content = fs::read_to_string(&path).expect("shortcode_extract.rs must be readable");

    let offenders: Vec<usize> = content
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let code = line.split("//").next().unwrap_or("");
            code.contains("parser::parse(") || code.contains(" parse(")
        })
        .map(|(i, _)| i + 1)
        .collect();

    assert!(
        offenders.is_empty(),
        "shortcode_extract.rs calls the default-config parser at line(s) {offenders:?}.\n\
         Shortcode inner content must be parsed with `parse_with_config(raw, config)` so the \
         caller's ParseConfig reaches it. A bare `parse()` re-introduces the leak that made a \
         math-on site render `$E=mc^2$` as an equation in prose and as literal text inside a \
         `:::hero` — the same page in two dialects. Thread `&ParseConfig` down rather than \
         re-defaulting it here."
    );
}
