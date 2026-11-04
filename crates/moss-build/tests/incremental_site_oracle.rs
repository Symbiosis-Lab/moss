//! The incremental-rebuild oracle over real sites, and its timing.
//!
//! For each site folder named in `MOSS_ORACLE_SITES` (separated by `:`), on a
//! clone of it, never the folder itself:
//!
//! 1. a cold full build, then a warm full build (the cost with no incremental
//!    reuse at all);
//! 2. a control: a clean full build of a fresh clone of the same source must
//!    match the warm build byte for byte, or the oracle below proves nothing;
//! 3. for each edit case, one paragraph of one page changes and the site is
//!    rebuilt with the trigger a watch save carries; each rebuild is timed
//!    phase by phase from the `timing` and `incremental` log targets;
//! 4. the oracle: every output file after the incremental rebuild is
//!    byte-identical to a clean full build of a fresh clone of the edited
//!    source.
//!
//! Ignored by default: it needs real sites and minutes per site.
//!
//! ```sh
//! MOSS_ORACLE_SITES="/scratch/a:/scratch/b" MOSS_ORACLE_OUT=/scratch/out \
//!   cargo test --release -p moss-build --test incremental_site_oracle -- --ignored --nocapture
//! ```
//!
//! `MOSS_ORACLE_OUT` receives `<site>.json` (timings, rendered and changed
//! page counts, each check's outcome) and, per case, every changed page's HTML
//! before and after the last timed rebuild, for timing the preview's morph in
//! a browser. `MOSS_ORACLE_WORK` is where the clones go (default: the output).
//! `MOSS_ORACLE_REPEAT` (default 3) is how many timed rebuilds each case gets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use moss_build::build::BuildTrigger;

struct Capture {
    lines: Mutex<Vec<String>>,
}

static CAPTURE: Capture = Capture { lines: Mutex::new(Vec::new()) };

impl log::Log for Capture {
    fn enabled(&self, m: &log::Metadata) -> bool {
        matches!(m.target(), "timing" | "incremental") && m.level() <= log::Level::Debug
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            self.lines.lock().unwrap().push(format!("[{}] {}", r.target(), r.args()));
        }
    }
    fn flush(&self) {}
}

fn take_log() -> Vec<String> {
    std::mem::take(&mut *CAPTURE.lines.lock().unwrap())
}

/// A build the way `moss build` runs one: it owns its runtime and finishes
/// everything (search index included) before returning.
fn build(root: &Path, trigger: BuildTrigger) -> Result<(Duration, Vec<String>), String> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    build_in(&rt, root, trigger, true)
}

/// One `run_pipeline` call. `exits_after_build: false` is the watch rebuild's
/// shape: background work (media, the search lane, the seal) is spawned on
/// `rt` and outlives the call, as it does in a long-lived preview process.
fn build_in(rt: &tokio::runtime::Runtime, root: &Path, trigger: BuildTrigger, exits_after_build: bool)
    -> Result<(Duration, Vec<String>), String>
{
    use moss_build::build::{run_pipeline, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;
    take_log();
    let folder = root.to_string_lossy().into_owned();
    let start = Instant::now();
    let result = rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(root),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(&folder),
        trigger,
        exits_after_build,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }));
    let wall = start.elapsed();
    result.map(|_| (wall, take_log())).map_err(|e| format!("build of {folder} failed: {e}"))
}

/// One check: `pass`, `fail` or `inconclusive`. Inconclusive means the check
/// could not see enough to judge (a build failed, the incremental path never
/// ran, two clean builds disagree on the file in question); it never counts as
/// a pass.
fn check(name: &str, outcome: &str, reason: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "check": name, "outcome": outcome, "reason": reason.into() })
}

/// Why a timed rebuild did not exercise incremental reuse, if it did not.
fn fallback_reason(sample: &serde_json::Value) -> Option<String> {
    if let Some(cause) = sample.get("full_render_cause").and_then(|c| c.as_str()) {
        return Some(format!("full render: {cause}"));
    }
    match sample.get("parse_cache").and_then(|c| c.as_str()) {
        Some(line) if line.contains("replayed") => None,
        Some(line) => Some(line.trim_start_matches("[incremental] ").to_string()),
        None => Some("no parse cache verdict logged".into()),
    }
}

/// An APFS clone where the filesystem has one, so a multi-gigabyte site costs
/// no disk; times are preserved, because a source mtime can reach a hash.
fn copy_tree(src: &Path, dst: &Path) {
    let cp = |flags: &str| std::process::Command::new("cp").arg(flags).arg(src).arg(dst).status();
    if !matches!(cp("-cRp"), Ok(s) if s.success()) {
        assert!(matches!(cp("-Rp"), Ok(s) if s.success()), "copy {} -> {}", src.display(), dst.display());
    }
}

fn clone_source(src: &Path, dst: &Path) {
    copy_tree(src, dst);
    // The build tree stays behind, so the clean build parses and renders every
    // page from source. The content-addressed media store comes along: it is
    // keyed by source bytes, and re-encoding every video per clean build would
    // cost hours on a video-heavy site without testing anything rendered.
    let moss = dst.join(".moss");
    for sub in ["build.nosync", "build"] {
        let _ = std::fs::remove_dir_all(moss.join(sub));
    }
    // Two per-machine caches under the build tree hold inputs, not render
    // state: what the network answered for external links, and the local
    // object store their downloaded covers (and other encoded bytes) live in.
    // A clean build that fetched them again would be judged on different input.
    // The incremental caches beside them (dependency, facade, hash index) stay out.
    for cache in [".moss/build.nosync/cache/link-meta", ".moss/build.nosync/cache/local-objects"] {
        if src.join(cache).is_dir() {
            std::fs::create_dir_all(dst.join(cache).parent().unwrap()).unwrap();
            copy_tree(&src.join(cache), &dst.join(cache));
        }
    }
}

fn staging(root: &Path) -> PathBuf {
    root.join(".moss/build.nosync/staging")
}

/// Text outputs by bytes; everything else (images, video, fonts) by length,
/// which the clean build's own copy of the same source matches.
#[derive(PartialEq)]
enum Content {
    Bytes(Vec<u8>),
    Len(u64),
}

/// Files that vanish mid-walk (a background encode's temp file) are skipped:
/// the judged snapshot is taken with nothing in flight.
fn snapshot(dir: &Path) -> BTreeMap<String, Content> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let rel = e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
            let text = matches!(
                e.path().extension().and_then(|x| x.to_str()),
                Some("html" | "xml" | "json" | "txt" | "css" | "js" | "svg" | "webmanifest")
            );
            let content = if text {
                Content::Bytes(std::fs::read(e.path()).ok()?)
            } else {
                Content::Len(e.metadata().ok()?.len())
            };
            Some((rel, content))
        })
        .collect()
}

/// Every divergence between two output trees. `_moss/og/` (sharing cards) and
/// `_moss/link/` (external-link covers) are content-addressed: an incremental
/// build keeps one no page references any more, which a clean build never
/// wrote, so only their missing files count. Every page that would reference
/// one is still compared byte for byte.
fn divergences(incremental: &BTreeMap<String, Content>, full: &BTreeMap<String, Content>) -> Vec<String> {
    let mut out = Vec::new();
    for (rel, f) in full {
        match incremental.get(rel) {
            None => out.push(format!("missing: {rel}")),
            Some(i) if i != f => out.push(format!("differs: {rel}")),
            _ => {}
        }
    }
    for rel in incremental.keys() {
        if !full.contains_key(rel) && !rel.starts_with("_moss/og/") && !rel.starts_with("_moss/link/") {
            out.push(format!("extra: {rel}"));
        }
    }
    out
}

fn changed_files(before: &BTreeMap<String, Content>, after: &BTreeMap<String, Content>) -> Vec<String> {
    after
        .iter()
        .filter(|(rel, c)| before.get(*rel) != Some(*c))
        .map(|(rel, _)| rel.clone())
        .chain(before.keys().filter(|rel| !after.contains_key(*rel)).cloned())
        .collect()
}

/// Milliseconds from a `{:?}`-formatted `Duration` such as `12.3ms`, `1.2s`, `950µs`.
fn ms(debug: &str) -> Option<f64> {
    let d = debug.trim().trim_end_matches(|c: char| c == ')' || c == ',');
    let (num, unit) = d.split_at(d.find(|c: char| c.is_alphabetic() || c == 'µ')?);
    let n: f64 = num.parse().ok()?;
    Some(match unit {
        "s" => n * 1000.0,
        "ms" => n,
        "µs" | "us" => n / 1000.0,
        "ns" => n / 1_000_000.0,
        _ => return None,
    })
}

/// The phase times of one build. Render-phase lines carry a running total
/// since render start, so a phase is the difference between two marks.
fn phases(log: &[String]) -> serde_json::Value {
    let mut marks: BTreeMap<&str, f64> = BTreeMap::new();
    let mut out = serde_json::Map::new();
    let after_colon = |line: &str| line.rsplit(": ").next().and_then(ms);
    for line in log {
        let mark = |key| (line.contains(key)).then(|| after_colon(line)).flatten();
        if line.contains("[render] markdown (") {
            if let Some(w) = line.split("wall=").nth(1).and_then(|s| s.split(' ').next()).and_then(ms) {
                out.insert("parse_ms".into(), w.into());
            }
            for part in ["read", "resolve", "process"] {
                let key = format!("{part}=");
                if let Some(v) = line.split(key.as_str()).nth(1).and_then(|s| s.split(' ').next()).and_then(ms) {
                    out.insert(format!("parse_cpu_{part}_ms"), v.into());
                }
            }
        }
        if let Some(v) = mark("[reduce] total") { out.insert("reduce_ms".into(), v.into()); }
        if let Some(v) = mark("[render] verdict computed") { marks.insert("verdict", v); }
        if let Some(v) = mark("[render] html_pages: partition done") {
            marks.insert("partition", v);
            let counts: Vec<u64> = line
                .split(|c: char| !c.is_ascii_digit())
                .filter(|s| !s.is_empty())
                .take(2)
                .filter_map(|s| s.parse().ok())
                .collect();
            if let [render, carry] = counts[..] {
                out.insert("pages_rendered".into(), render.into());
                out.insert("pages_carried".into(), carry.into());
            }
        }
        if let Some(v) = mark("[render] html_pages: ") .filter(|_| !line.contains("partition") && !line.contains("phase1")) {
            marks.insert("html_pages", v);
        }
        if let Some(v) = mark("[render] total") { marks.insert("render_total", v); }
        if let Some(v) = mark("[build] staging: generate_blocking_content") { out.insert("blocking_ms".into(), v.into()); }
        // Every later staging mark, by name, as time since build start: what
        // runs between the render and the build's return.
        if let Some(rest) = line.strip_prefix("[timing] [build] staging: ") {
            if let (Some(name), Some(v)) = (rest.split([':', ' ']).next(), after_colon(line)) {
                out.insert(format!("at_{name}_ms"), v.into());
            }
        }
        if let Some(v) = mark("[build] total") { out.insert("build_total_ms".into(), v.into()); }
        if line.contains("full render:") {
            out.insert("full_render_cause".into(), line.split("full render:").nth(1).unwrap_or("").trim().into());
        }
        // The verdict line, not the debug list of reparsed pages after it.
        if line.starts_with("[incremental] parse cache") && !line.starts_with("[incremental] parse cache reparsed") {
            out.insert("parse_cache".into(), line.clone().into());
        }
    }
    if let (Some(p), Some(h)) = (marks.get("partition"), marks.get("html_pages")) {
        out.insert("html_ms".into(), (h - p).into());
    }
    if let (Some(v), Some(p)) = (marks.get("verdict"), marks.get("partition")) {
        out.insert("verdict_to_partition_ms".into(), (p - v).into());
    }
    if let (Some(h), Some(t)) = (marks.get("html_pages"), marks.get("render_total")) {
        out.insert("after_html_ms".into(), (t - h).into());
    }
    if let Some(t) = marks.get("render_total") {
        out.insert("render_total_ms".into(), (*t).into());
    }
    out.insert("timing_lines".into(), log.iter().filter(|l| l.starts_with("[timing]")).cloned().collect::<Vec<_>>().into());
    serde_json::Value::Object(out)
}

/// A line that is one paragraph of prose: outside frontmatter and code, not
/// markup, long enough to be text.
fn prose_lines(text: &str) -> Vec<usize> {
    let mut lines = Vec::new();
    let mut in_fm = false;
    let mut in_code = false;
    for (i, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if i == 0 && t == "---" {
            in_fm = true;
            continue;
        }
        if in_fm {
            if t == "---" { in_fm = false; }
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            in_code = !in_code;
            continue;
        }
        let markup = t.starts_with(|c: char| "#-*+>|<![:{`=$%0123456789".contains(c)) || line.starts_with("    ");
        if !in_code && !markup && t.chars().filter(|c| c.is_alphabetic()).count() >= 12 && !t.ends_with('\\') {
            lines.push(i);
        }
    }
    lines
}

/// Every markdown page of a site, by path relative to its root.
fn pages(root: &Path) -> Vec<String> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".moss" && e.file_name() != ".git" && e.file_name() != "node_modules")
        .filter_map(Result::ok)
        .filter_map(|e| Some(e.path().strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/")))
        .filter(|rel| rel.ends_with(".md") && !rel.starts_with('.') && !rel.contains("/."))
        .collect()
}

struct Case {
    name: &'static str,
    page: String,
    line: usize,
}

/// The cases: the first and the last prose paragraph of the deepest, longest
/// page (the first can reach a listing's excerpt, the last should not), and
/// the first of the home page, which every page's description can fall back to.
fn pick_cases(root: &Path) -> Vec<Case> {
    let mut pages: Vec<(usize, u64, String)> = pages(root)
        .into_iter()
        .filter_map(|rel| {
            let stem = Path::new(&rel).file_stem()?.to_string_lossy().to_lowercase();
            let slot = !rel.contains('/') && matches!(stem.as_str(), "footer" | "header" | "sidebar" | "nav");
            let len = std::fs::metadata(root.join(&rel)).ok()?.len();
            (!matches!(stem.as_str(), "index" | "readme" | "_index") && !slot)
                .then(|| (rel.matches('/').count(), len, rel))
        })
        .filter(|(_, _, rel)| !prose_lines(&std::fs::read_to_string(root.join(rel)).unwrap_or_default()).is_empty())
        .collect();
    pages.sort();
    let mut cases = Vec::new();
    if let Some((_, _, leaf)) = pages.last() {
        let lines = prose_lines(&std::fs::read_to_string(root.join(leaf)).unwrap());
        cases.push(Case { name: "leaf-first-paragraph", page: leaf.clone(), line: lines[0] });
        if lines.len() > 1 {
            cases.push(Case { name: "leaf-last-paragraph", page: leaf.clone(), line: *lines.last().unwrap() });
        }
    }
    for home in ["index.md", "README.md", "readme.md"] {
        if let Ok(text) = std::fs::read_to_string(root.join(home)) {
            if let Some(&line) = prose_lines(&text).first() {
                cases.push(Case { name: "home-first-paragraph", page: home.into(), line });
            }
            break;
        }
    }
    cases
}

/// Append a word to one line, keeping every other byte.
fn edit_line(root: &Path, case: &Case, word: &str) {
    let path = root.join(&case.page);
    let text = std::fs::read_to_string(&path).unwrap();
    let mut out = String::with_capacity(text.len() + word.len() + 1);
    for (i, seg) in text.split_inclusive('\n').enumerate() {
        if i == case.line {
            let body = seg.trim_end_matches(['\n', '\r']);
            let content = body.trim_end();
            out.push_str(content);
            out.push(' ');
            out.push_str(word);
            out.push_str(&body[content.len()..]);
            out.push_str(&seg[body.len()..]);
        } else {
            out.push_str(seg);
        }
    }
    std::fs::write(&path, out).unwrap();
}

fn html_route(rel: &str) -> bool {
    rel.ends_with(".html")
}

#[test]
#[ignore = "needs MOSS_ORACLE_SITES: real site folders, minutes each"]
fn incremental_rebuilds_match_a_clean_full_build_on_real_sites() {
    let Ok(sites) = std::env::var("MOSS_ORACLE_SITES") else {
        eprintln!("MOSS_ORACLE_SITES not set; nothing to check");
        return;
    };
    // What `moss` seeds at startup; a site with a plugin block reads it.
    moss_build::system::seed_app_version(env!("CARGO_PKG_VERSION"));
    log::set_max_level(log::LevelFilter::Debug);
    let _ = log::set_logger(&CAPTURE);
    let out_dir = PathBuf::from(std::env::var("MOSS_ORACLE_OUT").expect("MOSS_ORACLE_OUT"));
    let repeat: usize = std::env::var("MOSS_ORACLE_REPEAT").ok().and_then(|s| s.parse().ok()).unwrap_or(3);
    let work_base = std::env::var("MOSS_ORACLE_WORK").map(PathBuf::from).unwrap_or_else(|_| out_dir.clone());
    let mut totals = serde_json::Map::new();

    for site in sites.split(':').filter(|s| !s.is_empty()) {
        let site = PathBuf::from(site);
        let name = site.file_name().unwrap().to_string_lossy().into_owned();
        let site_out = out_dir.join(name.replace(' ', "_"));
        std::fs::create_dir_all(&site_out).unwrap();
        let report = check_site(&site, &name, &site_out, &work_base, repeat);
        let count = |o: &str| report["checks"].as_array().unwrap().iter().filter(|c| c["outcome"] == o).count();
        let (pass, fail, inconclusive) = (count("pass"), count("fail"), count("inconclusive"));
        eprintln!("{name}: {pass} pass, {fail} fail, {inconclusive} inconclusive");
        for c in report["checks"].as_array().unwrap().iter().filter(|c| c["outcome"] != "pass") {
            eprintln!("  {} {}: {}", c["outcome"], c["check"], c["reason"]);
        }
        totals.insert(name.clone(), serde_json::json!({ "pass": pass, "fail": fail, "inconclusive": inconclusive }));
        std::fs::write(site_out.with_extension("json"), serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
    let green = totals.values().all(|t| t["fail"] == 0 && t["inconclusive"] == 0 && t["pass"].as_u64() > Some(0));
    assert!(green, "not green (a run is green only with zero fails and zero inconclusive): {}",
        serde_json::to_string_pretty(&totals).unwrap());
}

fn check_site(site: &Path, name: &str, site_out: &Path, work_base: &Path, repeat: usize) -> serde_json::Value {
    let mut checks: Vec<serde_json::Value> = Vec::new();
    let mut report = serde_json::json!({ "site": name, "cases": [] });
    let work = tempfile::tempdir_in(work_base).unwrap();
    let inc = work.path().join("incremental").join(name);
    std::fs::create_dir_all(inc.parent().unwrap()).unwrap();
    clone_source(site, &inc);
    report["markdown_files"] = pages(&inc).len().into();

    let builds = build(&inc, BuildTrigger::Full).and_then(|c| build(&inc, BuildTrigger::Full).map(|w| (c, w)));
    let ((cold, cold_log), (warm, warm_log)) = match builds {
        Ok(b) => b,
        Err(e) => {
            checks.push(check("site builds", "inconclusive", e));
            report["checks"] = checks.into();
            return report;
        }
    };
    report["cold_full"] = serde_json::json!({ "wall_ms": cold.as_secs_f64() * 1000.0, "phases": phases(&cold_log) });
    report["warm_full"] = serde_json::json!({ "wall_ms": warm.as_secs_f64() * 1000.0, "phases": phases(&warm_log) });
    let warm_snap = snapshot(&staging(&inc));
    report["output_files"] = warm_snap.len().into();
    report["output_html"] = warm_snap.keys().filter(|k| html_route(k)).count().into();

    // Two clean builds of one source: a file they disagree on cannot judge
    // incremental reuse, so a case that differs only there is inconclusive.
    let control = work.path().join("control").join(name);
    std::fs::create_dir_all(control.parent().unwrap()).unwrap();
    clone_source(&inc, &control);
    let control_div = match build(&control, BuildTrigger::Full) {
        Ok(_) => {
            let control_snap = snapshot(&staging(&control));
            let div = divergences(&warm_snap, &control_snap);
            for d in div.iter().filter(|d| d.ends_with(".html")) {
                let rel = d.split_once(": ").map_or(d.as_str(), |(_, r)| r);
                let dir = site_out.join("control").join(rel);
                std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
                for (tag, snap) in [("a", &warm_snap), ("b", &control_snap)] {
                    if let Some(Content::Bytes(b)) = snap.get(rel) {
                        std::fs::write(dir.with_extension(format!("{tag}.html")), b).unwrap();
                    }
                }
            }
            div
        }
        Err(e) => {
            checks.push(check("control build", "inconclusive", e));
            report["checks"] = checks.into();
            return report;
        }
    };
    let _ = std::fs::remove_dir_all(work.path().join("control"));
    report["control_divergences"] = serde_json::json!(control_div);

    let cases = pick_cases(&inc);
    if cases.is_empty() {
        checks.push(check("cases", "inconclusive", "no page with a prose paragraph to edit"));
    }
    for case in cases {
        let name_of_check = format!("{} ({})", case.name, case.page);
        let mut samples = Vec::new();
        let mut before = snapshot(&staging(&inc));
        let mut changed = Vec::new();
        let mut build_error = None;
        // Timed rebuilds in the watch shape, on one runtime the way a preview
        // process keeps one; the judged tree comes from one more incremental
        // rebuild in the CLI shape after them, so nothing is left in flight.
        let mut watch_rt = tokio::runtime::Runtime::new().unwrap();
        for r in 0..=repeat {
            let judged = r == repeat;
            if judged {
                // Let the watch rebuilds' background work settle first, so the
                // judged tree is not written by two builds at once.
                std::mem::replace(&mut watch_rt, tokio::runtime::Runtime::new().unwrap())
                    .shutdown_timeout(Duration::from_secs(120));
            }
            edit_line(&inc, &case, &format!("mossedit{r}"));
            let changed_path = inc.join(&case.page);
            let trigger = BuildTrigger::ContentOnly(vec![changed_path]);
            let built = if judged { build(&inc, trigger) } else { build_in(&watch_rt, &inc, trigger, false) };
            let (wall, log) = match built {
                Ok(b) => b,
                Err(e) => { build_error = Some(e); break; }
            };
            let after = snapshot(&staging(&inc));
            changed = changed_files(&before, &after);
            let mut p = phases(&log);
            p["wall_ms"] = (wall.as_secs_f64() * 1000.0).into();
            p["changed_outputs"] = changed.len().into();
            p["changed_html"] = changed.iter().filter(|c| html_route(c)).count().into();
            p["shape"] = if judged { "cli" } else { "watch" }.into();
            samples.push(p);
            if r + 1 == repeat {
                for rel in changed.iter().filter(|c| html_route(c)).take(40) {
                    let dir = site_out.join(case.name).join(rel);
                    std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
                    if let Some(Content::Bytes(b)) = before.get(rel) {
                        std::fs::write(dir.with_extension("before.html"), b).unwrap();
                    }
                    if let Some(Content::Bytes(b)) = after.get(rel) {
                        std::fs::write(dir.with_extension("after.html"), b).unwrap();
                    }
                }
            }
            before = after;
        }

        let full = work.path().join("full").join(name);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        clone_source(&inc, &full);
        let clean = build(&full, BuildTrigger::Full);
        let div = match &clean {
            Ok(_) => divergences(&snapshot(&staging(&inc)), &snapshot(&staging(&full))),
            Err(_) => Vec::new(),
        };
        let _ = std::fs::remove_dir_all(work.path().join("full"));
        let (judged, unjudgeable): (Vec<String>, Vec<String>) = div.iter().cloned().partition(|d| !control_div.contains(d));
        // The judged tree is the last rebuild's; an earlier sample that fell
        // back is recorded but does not void the check.
        let fallback = samples.last().and_then(fallback_reason);
        for s in samples.iter_mut() {
            if let Some(reason) = fallback_reason(s) {
                s["fallback"] = reason.into();
            }
        }
        let c = |outcome, reason: String| check(&name_of_check, outcome, reason);
        let v = if let Some(e) = build_error {
            c("inconclusive", e)
        } else if let Err(e) = clean {
            c("inconclusive", e)
        } else if !judged.is_empty() {
            c("fail", format!("incremental differs from a clean full build: {:?}", &judged[..judged.len().min(8)]))
        } else if let Some(reason) = fallback {
            c("inconclusive", format!("the incremental path did not run: {reason}"))
        } else if !unjudgeable.is_empty() {
            c("inconclusive", format!("differs only where two clean builds also differ: {unjudgeable:?}"))
        } else {
            c("pass", "byte-identical to a clean full build".into())
        };
        report["cases"].as_array_mut().unwrap().push(serde_json::json!({
            "case": case.name,
            "page": case.page,
            "line": case.line,
            "samples": samples,
            "changed_files_last": changed,
            "oracle_divergences": div,
            "outcome": v["outcome"],
        }));
        checks.push(v.clone());
        eprintln!("{name} [{}] {}", case.name, v["outcome"]);
    }
    report["checks"] = checks.into();
    report
}
