//! What a released moss promised a folder, held for every frozen fixture.
//!
//! Each folder under `tests/fixtures/content-lock/` is one shape an author's
//! folder may hold: an alias, a removed shortcode, a refusal that still
//! builds, a silent change. `tests/content.lock` records, as released, each
//! one's input hash, its `--strict` problem totals and its served paths. A
//! fixture fails when its input changed (a released fixture is frozen; a new
//! shape is a new fixture), its build stops, its problems exceed the lowest
//! recorded total, or a recorded path is no longer served and is not a
//! former path. A redirect stub is written at the old address, so a path a
//! redirect answers is still served.
//!
//! What a shape promises a page shows is declared beside its folder, in
//! `<shape>.shows`: one `<page> <css selector>` per line, naming only moss's
//! own released markup, and part of the frozen input. A fixture without one
//! promises only that it builds, its count and its paths.
//!
//! Only a release cut writes the lock, and only by appending a new fixture,
//! a lower total or a newly served path:
//!
//! ```sh
//! CONTRACT_LOCK=update cargo test -p moss-build --test it content_lock
//! ```
//!
//! Before that run, the last released CLI builds each fixture with
//! `--strict`: a total above its count, or fewer paths than it served, is a
//! regression to fix, not to record.

use crate::copy_dir::copy_dir_recursive;
use crate::site::{self, Site};
use moss_build::build::feeds::redirect_table::FORMER_PATHS;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn manifest(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

#[derive(Default, Debug)]
struct Recorded {
    input: Option<String>,
    problems: Vec<usize>,
    paths: BTreeSet<String>,
}

struct Outcome {
    input: String,
    problems: usize,
    paths: BTreeSet<String>,
    /// The `.shows` lines the built pages do not match.
    unshown: Vec<String>,
}

fn parse(text: &str) -> BTreeMap<String, Recorded> {
    let mut lock: BTreeMap<String, Recorded> = BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let mut parts = line.splitn(3, ' ');
        let (Some(shape), Some(kind), Some(value)) = (parts.next(), parts.next(), parts.next()) else {
            panic!("unreadable line in content.lock: {line}");
        };
        let r = lock.entry(shape.to_string()).or_default();
        match kind {
            "input" => r.input = Some(value.to_string()),
            "problems" => r.problems.push(value.parse().unwrap_or_else(|_| panic!("unreadable total: {line}"))),
            "path" => {
                r.paths.insert(value.to_string());
            }
            _ => panic!("unknown kind in content.lock: {line}"),
        }
    }
    lock
}

/// What `out` breaks of what was recorded for its fixture.
fn broken(r: &Recorded, out: &Outcome) -> Vec<String> {
    let mut why = Vec::new();
    if r.input.as_deref().is_some_and(|h| h != out.input) {
        why.push("its input changed; a released fixture is frozen, so add a new one".to_string());
    }
    if let Some(ceiling) = r.problems.iter().min() {
        if out.problems > *ceiling {
            why.push(format!("{} problems, over the released {ceiling}", out.problems));
        }
    }
    why.extend(out.unshown.iter().map(|line| format!("no longer shows {line}")));
    let former = |p: &str| FORMER_PATHS.iter().any(|(old, _)| old.trim_start_matches('/') == p);
    for p in r.paths.iter().filter(|p| !out.paths.contains(*p) && !former(p)) {
        why.push(format!("{p} is no longer served"));
    }
    why
}

/// The lines a release cut appends for one fixture: everything for a new
/// one, else a lower total and the paths it newly serves.
fn appended(shape: &str, r: Option<&Recorded>, out: &Outcome) -> Vec<String> {
    let mut lines = Vec::new();
    if r.is_none() {
        lines.push(format!("{shape} input {}", out.input));
    }
    if r.and_then(|r| r.problems.iter().min()).is_none_or(|min| out.problems < *min) {
        lines.push(format!("{shape} problems {}", out.problems));
    }
    for p in out.paths.iter().filter(|p| !r.is_some_and(|r| r.paths.contains(*p))) {
        lines.push(format!("{shape} path {p}"));
    }
    lines
}

fn input_hash(root: &Path, shows: Option<&str>) -> String {
    let mut files: Vec<(String, PathBuf)> = walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| (e.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"), e.into_path()))
        .collect();
    files.sort();
    let mut h = Sha256::new();
    for (rel, path) in files {
        let bytes = std::fs::read(&path).unwrap();
        h.update(rel.as_bytes());
        h.update([0]);
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(&bytes);
    }
    if let Some(shows) = shows {
        h.update(b".shows\0");
        h.update(shows.as_bytes());
    }
    format!("{:x}", h.finalize())
}

/// The `.shows` lines no built page matches.
fn unshown(site: &Site, shows: &str) -> Vec<String> {
    shows
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter(|line| {
            let (page, selector) = line.split_once(' ').unwrap_or_else(|| panic!("unreadable .shows line: {line}"));
            let selector = scraper::Selector::parse(selector).unwrap_or_else(|e| panic!("bad selector in {line}: {e}"));
            let html = std::fs::read_to_string(site.output(page)).unwrap_or_default();
            scraper::Html::parse_document(&html).select(&selector).next().is_none()
        })
        .map(String::from)
        .collect()
}

/// Build a copy of the fixture, since the build writes into its folder.
fn build(fixture: &Path) -> Result<Outcome, String> {
    let shows = std::fs::read_to_string(fixture.with_extension("shows")).ok();
    let site = Site { root: std::env::temp_dir().join(format!("moss_content_lock_{}", uuid::Uuid::new_v4())) };
    copy_dir_recursive(fixture, &site.root).unwrap();
    let built = site::build_logged(&site.root.to_string_lossy(), moss_build::build::BuildTrigger::Full)?;
    Ok(Outcome {
        input: input_hash(fixture, shows.as_deref()),
        // What `--strict` counts: logged warnings that are not notes, and the problems the CLI reports itself.
        problems: built.problems.len() + built.cli_problems,
        paths: site.served(),
        unshown: shows.as_deref().map(|s| unshown(&site, s)).unwrap_or_default(),
    })
}

#[test]
fn content_lock_every_released_fixture_still_builds_as_released() {
    let lock_path = manifest("tests/content.lock");
    let text = std::fs::read_to_string(&lock_path).unwrap();
    let lock = parse(&text);
    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(manifest("tests/fixtures/content-lock"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    fixtures.sort();
    let on_disk: BTreeSet<String> = fixtures.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    let gone: Vec<&String> = lock.keys().filter(|s| !on_disk.contains(*s)).collect();
    assert!(gone.is_empty(), "released fixtures deleted: {gone:?}. A released fixture never leaves.");

    let update = std::env::var("CONTRACT_LOCK").as_deref() == Ok("update");
    let mut failures = Vec::new();
    let mut lines = Vec::new();
    for fixture in &fixtures {
        let shape = fixture.file_name().unwrap().to_string_lossy().into_owned();
        let out = match build(fixture) {
            Ok(out) => out,
            Err(e) => {
                failures.push(format!("{shape}: the plain build stopped: {e}"));
                continue;
            }
        };
        let recorded = lock.get(&shape);
        // A declared promise holds before the first release records the fixture.
        let why = broken(recorded.unwrap_or(&Recorded::default()), &out);
        failures.extend(why.into_iter().map(|why| format!("{shape}: {why}")));
        lines.extend(appended(&shape, recorded, &out));
    }
    assert!(failures.is_empty(), "the content lock is broken:\n  {}", failures.join("\n  "));
    if update && !lines.is_empty() {
        let body = text.trim_end_matches('\n').to_string() + "\n" + &lines.join("\n") + "\n";
        std::fs::write(&lock_path, body).unwrap();
    }
}

#[test]
fn content_lock_holds_a_fixture_to_its_lowest_released_total() {
    let lock = parse("shape problems 3\nshape problems 1\n");
    let out = |problems| Outcome { input: String::new(), problems, paths: BTreeSet::new(), unshown: Vec::new() };
    assert_eq!(broken(&lock["shape"], &out(2)), vec!["2 problems, over the released 1".to_string()]);
    assert!(broken(&lock["shape"], &out(1)).is_empty());
}

#[test]
fn content_lock_a_release_appends_only_a_lower_total_and_new_paths() {
    let lock = parse("shape input abc\nshape problems 2\nshape path index.html\n");
    let out = Outcome { input: "abc".into(), problems: 1, paths: ["index.html".to_string(), "rss.xml".to_string()].into(), unshown: Vec::new() };
    assert_eq!(appended("shape", lock.get("shape"), &out), vec!["shape problems 1", "shape path rss.xml"]);
    let same = Outcome { problems: 2, ..out };
    assert_eq!(appended("shape", lock.get("shape"), &same), vec!["shape path rss.xml"]);
}

#[test]
fn content_lock_names_a_changed_input_a_lost_page_and_a_lost_show() {
    let former = FORMER_PATHS[0].0.trim_start_matches('/');
    let lock = parse(&format!("shape input abc\nshape path index.html\nshape path gone.html\nshape path {former}\n"));
    let out = Outcome {
        input: "abd".into(),
        problems: 0,
        paths: ["index.html".to_string()].into(),
        unshown: vec!["index.html .moss-grid".to_string()],
    };
    assert_eq!(
        broken(&lock["shape"], &out),
        vec![
            "its input changed; a released fixture is frozen, so add a new one".to_string(),
            "no longer shows index.html .moss-grid".to_string(),
            "gone.html is no longer served".to_string(),
        ]
    );
}
