//! Full-build-equivalence tests for the surface-dependents narrowing
//! (`build::render::incremental::dependents`).
//!
//! The claim under test: for every one of the six non-graph channels a
//! page's surface is load-bearing for (site nav, breadcrumbs, series
//! siblings, homepage title, folder-embed listings, and the classified/
//! unclassified boundary itself), an INCREMENTAL rebuild after a single-field
//! edit must serve byte-identical output to a FROM-SCRATCH full build of the
//! same edited vault. `verdict_tests.rs` pins the render-SET math in
//! isolation; this file is the end-to-end check that the real pipeline
//! (scan -> parse -> render -> emit -> slot injection) agrees with it on
//! actual bytes, not just on which paths a `RenderVerdict` names.
//!
//! Every page in the output tree is compared, not just the edited one — a
//! narrowing bug's symptom is exactly a page NOBODY thought to check staying
//! stale.

use std::path::{Path, PathBuf};

#[path = "support/copy_dir.rs"]
mod copy_dir;

/// A fixed instant for every fixture file's mtime, so two independently
/// written copies of the same content hash identically end to end — a
/// source mtime can fold into a content hash downstream (`copy_dir.rs`'s own
/// doc comment, re: `og_card.rs`'s cover-image hash), and this suite writes
/// each tree with `std::fs::write` rather than a single `copy_dir_recursive`
/// call, so nothing else pins that timestamp for it.
fn fixed_mtime() -> std::time::SystemTime {
    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)
}

fn write(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, contents).unwrap();
    std::fs::File::open(&path).unwrap().set_modified(fixed_mtime()).unwrap();
}

/// A rich fixture covering every channel under test: nav items in two
/// languages, breadcrumbs on, nested folders with index pages (two levels),
/// a series folder, a tag page and a place page (both term-derived), a
/// translated pair (`about.md` / `zh-hans/about.md`, matched by stem across
/// language trees), an inline folder-embed listing on the homepage, and a
/// homepage with a title.
fn write_vault(root: &Path) {
    write(
        root,
        ".moss/config.toml",
        r#"schema_version = 6

[site]
lang = "en"

[terms.tags]
fields = ["tags"]
title = "Tags"

[terms.places]
type = "place"
fields = ["location"]
title = "Places"
"#,
    );
    write(
        root,
        ".moss/places.toml",
        r#"["Riverside"]
lat = 1.0
lng = 2.0
precision = "city"
"#,
    );

    write(
        root,
        "index.md",
        r#"---
title: Riverside Notes
uid: home-en
lang: en
breadcrumb: true
---

# Riverside Notes

The homepage, with an inline folder-embed listing of the writings folder.

![[/writings/]]
"#,
    );
    write(
        root,
        "about.md",
        r#"---
title: About
uid: about-en
lang: en
nav: true
weight: 10
---

About this site.
"#,
    );
    write(
        root,
        "contact.md",
        r#"---
title: Contact
uid: contact-en
lang: en
nav: true
weight: 20
---

Contact us.
"#,
    );

    write(
        root,
        "writings/index.md",
        r#"---
title: Writings
uid: writings-index
lang: en
---

# Writings

A nested folder index.
"#,
    );
    write(
        root,
        "writings/alpha.md",
        r#"---
title: Alpha
uid: writings-alpha
lang: en
tags:
  - craft
location:
  - Riverside
---

Alpha's own body.
"#,
    );
    write(
        root,
        "writings/beta.md",
        r#"---
title: Beta
uid: writings-beta
lang: en
---

Beta's own body.
"#,
    );
    write(
        root,
        "writings/deep/index.md",
        r#"---
title: Deep
uid: writings-deep-index
lang: en
---

# Deep

A second level of nesting, so a breadcrumb trail has more than one middle
segment to carry.
"#,
    );
    write(
        root,
        "writings/deep/leaf.md",
        r#"---
title: Leaf
uid: writings-deep-leaf
lang: en
---

The deepest page.
"#,
    );

    write(
        root,
        "series/index.md",
        r#"---
title: Series
uid: series-index
lang: en
series: true
---

# Series

A series folder — its direct children form a reading-order chain.
"#,
    );
    write(
        root,
        "series/part-1.md",
        r#"---
title: Part One
uid: series-part-1
lang: en
weight: 1
---

The first step.
"#,
    );
    write(
        root,
        "series/part-2.md",
        r#"---
title: Part Two
uid: series-part-2
lang: en
weight: 2
---

The second step.
"#,
    );
    write(
        root,
        "series/part-3.md",
        r#"---
title: Part Three
uid: series-part-3
lang: en
weight: 3
---

The third step.
"#,
    );

    write(
        root,
        "zh-hans/index.md",
        r#"---
title: 河岸笔记
uid: home-zh
lang: zh-hans
---

欢迎来到这个网站。这是中文主页。
"#,
    );
    write(
        root,
        "zh-hans/about.md",
        r#"---
title: 关于
uid: about-zh
lang: zh-hans
---

关于这个网站。
"#,
    );

    // Two footer-only pages (not nav items) so a weight edit has a second
    // entry to reorder past — with only one, the footer's rendered HTML
    // would be identical either way and the scenario below would prove
    // nothing.
    write(
        root,
        "support.md",
        r#"---
title: Support Us
uid: footer-support
lang: en
nav: false
footer: true
weight: 5
---

A footer-only link, not in the nav.
"#,
    );
    write(
        root,
        "donate.md",
        r#"---
title: Donate
uid: footer-donate
lang: en
nav: false
footer: true
weight: 15
---

A second footer-only link, so an order change is observable.
"#,
    );

    // A `writings/` child carrying only untyped custom frontmatter
    // (`external_url:`/`publisher:`, `child_list.rs`'s `external_url`/
    // `publisher` helpers) — shown on both `writings/index.html`'s own
    // listing and the homepage's `![[/writings/]]` folder embed.
    write(
        root,
        "writings/gamma.md",
        r#"---
title: Gamma
uid: writings-gamma
lang: en
external_url: https://example.com/gamma
publisher: Example Press
---

Gamma is syndicated from elsewhere.
"#,
    );
}

/// Reconstruction of the desktop app's `moss::build_sync` from moss-build's
/// own public API — same shape `snapshot_tests.rs` uses, parameterized by
/// `trigger` so a second call against the same folder can pass
/// `BuildTrigger::ContentOnly` and actually engage the incremental skip
/// (`IncrementalGates::render_skip`, gated on the trigger alone — see
/// `build/incremental_gates.rs` — not on watch mode).
fn build(folder_path: &str, trigger: moss_build::build::BuildTrigger) -> Result<String, String> {
    use moss_build::build::{run_pipeline, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let source = PathBuf::from(folder_path);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(&source),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(folder_path),
        trigger,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }))
}

fn staging_dir(root: &Path) -> PathBuf {
    root.join(".moss/build.nosync/staging")
}

/// Every file under `dir`, keyed by its path relative to `dir`.
fn list_files(dir: &Path) -> std::collections::BTreeMap<String, PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            let rel = e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
            (rel, e.path().to_path_buf())
        })
        .collect()
}

/// Byte-compare every file in two build output trees. Panics with a message
/// naming every divergence, missing file, and extra file — not just the
/// first one — so a single run pins the whole shape of a failure.
#[track_caller]
fn assert_trees_byte_identical(scenario: &str, incremental_root: &Path, full_root: &Path) {
    // `_moss/og/<hash>.png` is content-addressed and, confirmed by direct
    // comparison against a two-FULL-build sequence (which produces exactly
    // the same 14 files a single full build does), reused/kept rather than
    // swept when a build is incremental — the stale-file sweep for image
    // outputs runs on a Full build, not a `ContentOnly` one. That is a
    // property of the pre-existing carry-forward machinery
    // (`render/blocking.rs`'s `to_carry` loop registers a carried page's
    // HTML manifest entry but not its card's `ImageOutputs` entry), present
    // for any incremental rebuild regardless of this narrowing, and not
    // something this file's scenarios fix. A leftover card is unreferenced
    // by any page's HTML (checked directly), so it is inert, not stale
    // content — exclude the directory from the extra/missing check; every
    // OTHER path, including
    // every page's own HTML, still has to match exactly.
    let is_content_addressed_cache = |rel: &str| rel.starts_with("_moss/og/");

    let incremental = list_files(&staging_dir(incremental_root));
    let full = list_files(&staging_dir(full_root));

    let mut problems: Vec<String> = Vec::new();
    for (rel, full_path) in &full {
        match incremental.get(rel) {
            None => problems.push(format!("missing from the incremental build: {rel}")),
            Some(inc_path) => {
                let a = std::fs::read(inc_path).unwrap();
                let b = std::fs::read(full_path).unwrap();
                if a != b {
                    problems.push(format!(
                        "DIVERGES: {rel} ({} bytes incremental vs {} bytes full)",
                        a.len(),
                        b.len()
                    ));
                }
            }
        }
    }
    for rel in incremental.keys() {
        if !full.contains_key(rel) && !is_content_addressed_cache(rel) {
            problems.push(format!("extra in the incremental build, absent from full: {rel}"));
        }
    }

    assert!(
        problems.is_empty(),
        "[{scenario}] incremental vs from-scratch full build diverged:\n{}",
        problems.join("\n")
    );
}

struct Scenario<'a> {
    name: &'a str,
    /// Applied to BOTH the incremental tree (after its first full build) and
    /// the fresh tree (before its only, full build).
    edit: fn(&Path),
    /// The source path the edit touched — the trigger a real autosave would
    /// carry.
    changed: &'a str,
}

/// Every test in this file that calls `build()` holds this for its whole
/// scenario. Only `nav_weight_edit_verdict_log_shows_a_narrowed_incremental_render`
/// actually needs exclusivity (it reads a GLOBAL, process-wide log capture,
/// which every other test's builds would otherwise also write into) — but
/// gating every scenario on the same lock is what makes that guarantee hold
/// regardless of run order or `--test-threads`, rather than depending on
/// this file's current test list never growing another log-reading test.
static SEQUENTIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn run_scenario(s: &Scenario<'_>) {
    let _guard = SEQUENTIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let tmp = tempfile::tempdir().unwrap();
    let incremental_root = tmp.path().join("incremental");
    let full_root = tmp.path().join("full");

    write_vault(&incremental_root);
    let baseline = build(&incremental_root.to_string_lossy(), moss_build::build::BuildTrigger::Full);
    assert!(baseline.is_ok(), "[{}] baseline full build failed: {:?}", s.name, baseline);

    (s.edit)(&incremental_root);
    let changed_path = incremental_root.join(s.changed);
    let incremental_result = build(
        &incremental_root.to_string_lossy(),
        moss_build::build::BuildTrigger::ContentOnly(vec![changed_path]),
    );
    assert!(incremental_result.is_ok(), "[{}] incremental rebuild failed: {:?}", s.name, incremental_result);

    write_vault(&full_root);
    (s.edit)(&full_root);
    let full_result = build(&full_root.to_string_lossy(), moss_build::build::BuildTrigger::Full);
    assert!(full_result.is_ok(), "[{}] from-scratch full build failed: {:?}", s.name, full_result);

    assert_trees_byte_identical(s.name, &incremental_root, &full_root);
}

fn edit_frontmatter(root: &Path, rel: &str, from: &str, to: &str) {
    let path = root.join(rel);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains(from), "fixture drift: {rel} no longer contains {from:?}");
    let edited = content.replacen(from, to, 1);
    std::fs::write(&path, edited).unwrap();
    // Advance past `fixed_mtime()` so a parse-cache/facade path that happens
    // to read mtime (none currently do for markdown; belt and suspenders)
    // sees a real change, and so this write can never collide with the
    // timestamp every unedited sibling file still carries.
    std::fs::File::open(&path)
        .unwrap()
        .set_modified(fixed_mtime() + std::time::Duration::from_secs(1))
        .unwrap();
}

// ---- title edits on every role the design's dependency table names --------

#[test]
fn title_edit_on_a_leaf_article() {
    run_scenario(&Scenario {
        name: "title: leaf article",
        edit: |root| edit_frontmatter(root, "writings/beta.md", "title: Beta", "title: Beta Renamed"),
        changed: "writings/beta.md",
    });
}

#[test]
fn title_edit_on_a_folder_index_page() {
    run_scenario(&Scenario {
        name: "title: folder-index page",
        edit: |root| edit_frontmatter(root, "writings/index.md", "title: Writings", "title: Writings Renamed"),
        changed: "writings/index.md",
    });
}

#[test]
fn title_edit_on_a_nav_item() {
    run_scenario(&Scenario {
        name: "title: nav item",
        edit: |root| edit_frontmatter(root, "about.md", "title: About", "title: About Renamed"),
        changed: "about.md",
    });
}

#[test]
fn title_edit_on_a_series_member() {
    run_scenario(&Scenario {
        name: "title: series member",
        edit: |root| edit_frontmatter(root, "series/part-1.md", "title: Part One", "title: Part One Renamed"),
        changed: "series/part-1.md",
    });
}

#[test]
fn title_edit_on_the_homepage() {
    run_scenario(&Scenario {
        name: "title: homepage",
        edit: |root| edit_frontmatter(root, "index.md", "title: Riverside Notes", "title: Riverside Notes Renamed"),
        changed: "index.md",
    });
}

#[test]
fn title_edit_on_a_translated_page() {
    run_scenario(&Scenario {
        name: "title: translated page",
        edit: |root| edit_frontmatter(root, "zh-hans/about.md", "title: 关于", "title: 关于我们"),
        changed: "zh-hans/about.md",
    });
}

#[test]
fn title_edit_on_a_page_listed_in_a_folder_embed() {
    // `writings/alpha.md` is both an inline `![[/writings/]]` embed member
    // (homepage) and an auto-listed child of `writings/index.md`'s own
    // folder-embed — the same page satisfies both readings of "listed in a
    // folder embed."
    run_scenario(&Scenario {
        name: "title: folder-embed member",
        edit: |root| edit_frontmatter(root, "writings/alpha.md", "title: Alpha", "title: Alpha Renamed"),
        changed: "writings/alpha.md",
    });
}

// ---- the other classified fields ------------------------------------------

#[test]
fn label_edit_on_a_folder_index() {
    // moss has no independently-authored `label:` frontmatter key — `doc.label`
    // (the chrome text nav/breadcrumbs/listings show) is always the "chrome
    // label cascade" computed from `title:` (else the filename), never a
    // separate field an author sets on its own
    // (`markdown/pipeline.rs`'s `label`/`heading::compute` — confirmed by
    // grepping every `.label =`/`label:` assignment in production code: none
    // read a `label:` key). So on a REAL page the only way to move `label`
    // is the same edit as `title_edit_on_a_folder_index_page` — this
    // scenario is kept separate anyway: on THIS folder (not nav-eligible,
    // not a home) it isolates the breadcrumb-ancestor channel from the nav/
    // homepage-title channels a title edit could otherwise also touch.
    run_scenario(&Scenario {
        name: "label: folder index (breadcrumb ancestor)",
        edit: |root| edit_frontmatter(root, "writings/index.md", "title: Writings", "title: Writings Renamed"),
        changed: "writings/index.md",
    });
}

#[test]
fn lang_change_on_a_nav_item() {
    run_scenario(&Scenario {
        name: "lang: nav item",
        edit: |root| edit_frontmatter(root, "contact.md", "lang: en", "lang: zh-hans"),
        changed: "contact.md",
    });
}

#[test]
fn weight_change_on_a_nav_item() {
    // Crosses `contact.md`'s weight (20), not just moves within it — a
    // same-side reweigh leaves the nav bar's rendered ORDER unchanged on
    // every other page, which would make this scenario pass even with the
    // nav-globals channel ablated away. The point of this edit is that the
    // link sequence in every OTHER "en" page's nav bar visibly flips.
    run_scenario(&Scenario {
        name: "weight: nav item",
        edit: |root| edit_frontmatter(root, "about.md", "weight: 10", "weight: 25"),
        changed: "about.md",
    });
}

// ---- the fail-safe: an unclassified field must still be provably safe -----

#[test]
fn unclassified_field_edit_stays_safe_via_the_full_render_fallback() {
    // `sidebar:` is not in the classified set (`dependents::field_is_classified`),
    // so this build takes the OLD path — `FullCause::SurfaceChanged`, render
    // everything — pinned directly at `verdict_tests.rs`'s
    // `an_unclassified_field_edit_still_forces_full`. What this integration
    // test adds is the same byte-equivalence guarantee for that path: a
    // full-render bypass is trivially safe (every page is re-rendered, none
    // carried), and this proves the fixture's plumbing doesn't quietly
    // disagree.
    run_scenario(&Scenario {
        name: "unclassified: sidebar",
        edit: |root| edit_frontmatter(root, "writings/deep/index.md", "title: Deep", "title: Deep\nsidebar: writings"),
        changed: "writings/deep/index.md",
    });
}

#[test]
fn weight_edit_on_a_footer_page() {
    // `weight` is classified only when `doc.footer != Some(true)`
    // (`dependents::field_is_classified`) — a footer page's own weight
    // feeds `generate_footer`'s sort order on EVERY other page's rendered
    // footer, a whole-corpus channel nothing here narrows. Reordering
    // `support.md` past `donate.md` (weight 5 -> 20, `donate.md` is 15) must
    // take the Full fallback so every other page's footer actually reorders.
    run_scenario(&Scenario {
        name: "footer: weight reorders past a sibling",
        edit: |root| edit_frontmatter(root, "support.md", "weight: 5", "weight: 20"),
        changed: "support.md",
    });
}

#[test]
fn custom_key_edit_on_a_page_shown_on_anothers_listing_card() {
    // `publisher:` has no typed struct field — it is read straight out of
    // `raw_frontmatter` by `child_list.rs`'s `publisher()` helper, on
    // whichever page hosts a listing that shows this child's card
    // (`writings/index.html`'s own listing, and the homepage's
    // `![[/writings/]]` embed). Proves the fix for the raw_frontmatter
    // confound — stripping the eight TYPED keys before the surface hash —
    // left an UNTYPED key's own value still reaching the pre-existing
    // listing-group digest (`listing::project_child`, which hashes the
    // whole stripped `ParsedDocument`, `raw_frontmatter` included) that
    // updates a card on its host.
    run_scenario(&Scenario {
        name: "custom key: publisher on a listed child",
        edit: |root| edit_frontmatter(root, "writings/gamma.md", "publisher: Example Press", "publisher: New Press"),
        changed: "writings/gamma.md",
    });
}

// ---- proof the narrowing actually engages, not just that it is safe -------
//
// Every scenario above proves SAFETY: incremental and from-scratch-full
// output match. None of them by itself proves the narrowing ever fired
// rather than every build quietly taking the old blanket `Full` path (which
// would ALSO pass every byte-comparison above, trivially). This is not a
// hypothetical: the fixture's own `raw_frontmatter` catch-all field
// initially reproduced exactly that trap — every frontmatter edit moved it
// alongside the named field being tested, so `FullCause::SurfaceChanged`
// fired every time regardless of classification, and the whole ablation
// pass below was silently testing nothing until `facade::surface_debug`
// started stripping the typed keys back out of it
// (`dependents::strip_typed_frontmatter_keys`). This test is what would have
// caught that: it reads the verdict's own log line rather than trusting the
// output comparison alone.

struct CapturingLogger {
    lines: std::sync::Mutex<Vec<String>>,
}

static CAPTURE: std::sync::OnceLock<CapturingLogger> = std::sync::OnceLock::new();
static INSTALL: std::sync::Once = std::sync::Once::new();

impl log::Log for CapturingLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) && record.target() == "incremental" {
            self.lines.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(record.args().to_string());
        }
    }
    fn flush(&self) {}
}

/// Install the capturing logger once per process and return its handle.
/// `log::set_boxed_logger` is a single global assignment, so this is the
/// only logger any test in this binary gets — fine here, since this file's
/// other tests don't depend on log output at all.
fn capture() -> &'static CapturingLogger {
    let logger = CAPTURE.get_or_init(|| CapturingLogger { lines: std::sync::Mutex::new(Vec::new()) });
    INSTALL.call_once(|| {
        log::set_max_level(log::LevelFilter::Info);
        let _ = log::set_logger(logger);
    });
    logger
}

/// A nav-eligible page's `weight` edit must produce an `Incremental` verdict
/// that actually skips pages — not merely "no divergence," which a
/// permanently-`Full` build would also produce.
#[test]
fn nav_weight_edit_verdict_log_shows_a_narrowed_incremental_render() {
    let _guard = SEQUENTIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let logger = capture();
    logger.lines.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("incremental");
    write_vault(&root);
    let baseline = build(&root.to_string_lossy(), moss_build::build::BuildTrigger::Full);
    assert!(baseline.is_ok(), "{baseline:?}");
    // The cold-cache first build legitimately logs "full render: skip
    // disabled" — clear it so only the SECOND build's verdict is under test.
    logger.lines.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();

    edit_frontmatter(&root, "about.md", "weight: 10", "weight: 25");
    let changed = root.join("about.md");
    let result = build(&root.to_string_lossy(), moss_build::build::BuildTrigger::ContentOnly(vec![changed]));
    assert!(result.is_ok(), "{result:?}");

    let lines = logger.lines.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let verdict_line = lines.iter().find(|l| l.contains("tracked pages"));
    assert!(
        verdict_line.is_some_and(|l| l.contains("by surface dependents") && !l.contains("skipping 0")),
        "expected a narrowed incremental verdict that actually skips pages, got: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("full render:")),
        "a classified weight edit must not take the Full fallback, got: {lines:?}"
    );
}
