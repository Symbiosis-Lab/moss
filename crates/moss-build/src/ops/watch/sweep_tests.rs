use super::*;
use super::progress::*;
use super::tick::*;
use super::walk::*;

use crate::build::watch::{drift, scope};
use crate::system::folder_session::FolderSession;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Build `folder_path` once, synchronously from the test's point of view —
/// the engine has no app-side `build_sync` wrapper, so this drives
/// `run_pipeline` directly the way other moss-build tests do (see
/// `build/watch_tests.rs`).
async fn build_once(folder_path: &str) {
    use crate::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    let root = crate::vault::paths::VaultRoot::resolve(Path::new(folder_path));
    run_pipeline(PipelineConfig {
        root,
        progress: crate::build::null_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: crate::build::ports::host::test_host_ports(),
        trigger: BuildTrigger::Full,
        // A one-shot build sets this too: it is what
        // makes the seal tail (the `hashes.json` write these tests read)
        // finish before the call returns, rather than detaching the way a
        // long-lived watch process's rebuild does.
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    })
    .await
    .expect("build");
}

// ── The walk (ported from the cloud supervisor, whose loop this absorbed) ──

/// The walk is what feeds everything else, so it has to see the eviction
/// form the scan cannot: a pre-Sonoma placeholder, whose real filename is
/// absent from the directory entirely.
#[cfg(target_os = "macos")]
#[test]
fn the_walk_finds_a_placeholder_and_names_the_real_file() {
    // `TempDir::new()` names its directory `.tmpXXXXXX`, and that dot is
    // load-bearing HERE: it puts the vault under a dot-prefixed ancestor,
    // which is the shape of every synced vault reached through a
    // hidden-folder mount (`.hidden-ancestor/…`) and the shape that used to make
    // the predicates refuse every file in it. Naming the temp dir without a
    // dot turns this green while leaving that live — the test would stop
    // failing before the bug did. Keep the dot; it is the guard.
    let dir = tempfile::TempDir::new().unwrap();
    // A non-dot subdirectory, so the vault-RELATIVE path the predicates now
    // see is clean and only the ancestor is dotted.
    let root = &dir.path().join("vault");
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(root.join("index.md"), "# Home").unwrap();
    std::fs::write(root.join(".away.md.icloud"), b"").unwrap();
    // Excluded by the watcher's own filter — moss must not read it.
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/.HEAD.icloud"), b"").unwrap();

    assert_eq!(
        walk(root, None, None).dataless,
        vec![root.join("away.md")],
        "the real name, not the placeholder — and nothing from .git/"
    );
}

/// A vault with everything present yields nothing to wait for. Without this
/// moss would re-read files that are already on disk.
#[test]
fn a_fully_local_vault_has_nothing_dataless() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().join("vault");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("index.md"), "# Home").unwrap();
    std::fs::write(root.join("photo.jpg"), b"not really a jpeg").unwrap();
    assert!(walk(&root, None, None).dataless.is_empty());
}

#[test]
fn arrival_accounting_does_not_call_a_deleted_file_downloaded() {
    let dir = tempfile::tempdir().unwrap();
    let arrived = dir.path().join("arrived.md");
    let deleted = dir.path().join("deleted.md");
    std::fs::write(&arrived, "# Here").unwrap();
    let mut pending = HashSet::from([arrived, deleted]);
    assert_eq!(observe_pending(&mut pending), (1, 1));
    assert!(pending.is_empty());
}

#[test]
fn navigation_promotes_only_the_resolved_requested_source() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("posts/first.md");
    let second = dir.path().join("posts/second.md");
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::write(&first, "# First").unwrap();
    std::fs::write(&second, "# Second").unwrap();
    let _first = crate::build::icloud::pretend::evicted_until_requested(&first);
    let _second = crate::build::icloud::pretend::evicted_until_requested(&second);
    let session = FolderSession::new(dir.path().to_path_buf());
    let mut revision = 0;

    session.set_preview_requirement("/unresolved/".into(), crate::system::folder_session::PreviewSource::Unresolved).unwrap();
    promote_focused_source(&session, &mut revision);
    assert_eq!(crate::build::icloud::pretend::requests_for(&first), 0);
    assert_eq!(crate::build::icloud::pretend::requests_for(&second), 0);

    session.set_preview_requirement("/generated/".into(), crate::system::folder_session::PreviewSource::Generated).unwrap();
    promote_focused_source(&session, &mut revision);
    assert_eq!(crate::build::icloud::pretend::requests_for(&first), 0);

    session.set_preview_requirement("/first/".into(), crate::system::folder_session::PreviewSource::File(PathBuf::from("posts/first.md"))).unwrap();
    promote_focused_source(&session, &mut revision);
    promote_focused_source(&session, &mut revision);
    assert_eq!(crate::build::icloud::pretend::requests_for(&first), 1);
    assert_eq!(crate::build::icloud::pretend::requests_for(&second), 0);

    session.set_preview_requirement("/second/".into(), crate::system::folder_session::PreviewSource::File(PathBuf::from("posts/second.md"))).unwrap();
    promote_focused_source(&session, &mut revision);
    assert_eq!(crate::build::icloud::pretend::requests_for(&second), 1);
}

/// `.moss/` is dot-prefixed, so the obvious dir filter prunes it — and with
/// it every user-authored build input moss reads from there. The files that
/// hard-fail a build were the only ones nobody was downloading.
#[cfg(target_os = "macos")]
#[test]
fn the_walk_reaches_moss_inputs_but_not_moss_output() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().join("vault");
    std::fs::create_dir_all(root.join(".moss/theme")).unwrap();
    std::fs::create_dir_all(root.join(".moss/identity")).unwrap();
    std::fs::create_dir_all(root.join(".moss/build.nosync/generations/1")).unwrap();
    std::fs::write(root.join(".moss/.config.toml.icloud"), b"").unwrap();
    std::fs::write(root.join(".moss/theme/.style.css.icloud"), b"").unwrap();
    std::fs::write(root.join(".moss/identity/.secret-key.icloud"), b"").unwrap();
    std::fs::write(root.join(".moss/build.nosync/generations/1/.index.html.icloud"), b"").unwrap();

    let mut paths = walk(&root, None, None).dataless;
    paths.sort();
    assert_eq!(
        paths,
        vec![
            root.join(".moss/config.toml"),
            root.join(".moss/identity/secret-key"),
            root.join(".moss/theme/style.css"),
        ],
        "everything moss reads and cannot regenerate — never the generations it can"
    );
}

/// The regression: the identity key has no watcher rule
/// (nothing rebuilds when it changes) and no extension, so BOTH halves of
/// the old filter dropped it. It is also the one file publish cannot
/// proceed without, so it was never slow to arrive — it was never asked for.
#[test]
fn the_walk_asks_for_the_identity_key_publish_needs() {
    let folder = Path::new("/vault");
    assert!(should_descend(".moss/identity", "identity"));
    assert!(should_request(folder, &folder.join(".moss/identity/secret-key")));
    assert!(should_request(folder, &folder.join(".moss/identity/public.json")));
    assert!(should_request(folder, &folder.join(".moss/keys/plugin.key")));
    assert!(
        !scope::path_passes_filter(folder, &folder.join(".moss/identity/secret-key")),
        "and the watcher still does not watch it — these are different questions"
    );
}

#[test]
fn should_descend_walks_moss_inputs_and_prunes_everything_else() {
    assert!(should_descend("", "vault"), "the root, whatever the user named it");
    assert!(should_descend("", ".private-site"), "including a dot-prefixed vault");
    assert!(should_descend("notes", "notes"));
    assert!(!should_descend("node_modules", "node_modules"));
    assert!(!should_descend(".git", ".git"));
    assert!(should_descend(".moss", ".moss"), "or its children are unreachable");
    assert!(should_descend(".moss/theme", "theme"));
    assert!(should_descend(".moss/theme/fonts", "fonts"), "nested theme dirs too");
    assert!(should_descend(".moss/data/social", "social"));
    assert!(should_descend(".moss/identity", "identity"), "publish's own input");
    assert!(!should_descend(".moss/build.nosync", "build"), "moss's own output");
    assert!(!should_descend(".moss/build.nosync/staging", "staging"));
    assert!(!should_descend(".moss/cache", "cache"));
    assert!(
        !should_descend(".moss/whatever-comes-next", "whatever-comes-next"),
        "an undeclared .moss/ subdir must not silently be swept"
    );
}

/// Nested-vault boundary: a descendant owning its own `.moss/` is a
/// different site — the outer sweep must not spend the user's bandwidth
/// downloading it, and must not read its files as the outer site's drift.
/// It is handled when THAT site is opened.
#[test]
fn the_walk_stops_at_a_nested_moss_site() {
    let dir = tempfile::Builder::new().prefix("moss_sweep").tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("inner/.moss")).unwrap();
    std::fs::write(root.join("here.md"), "# Here").unwrap();
    std::fs::write(root.join("inner/there.md"), "# There").unwrap();

    assert!(
        should_request(root, &root.join("here.md")),
        "the outer vault's own file is still asked for"
    );
    assert!(
        !should_request(root, &root.join("inner/there.md")),
        "a file inside the nested site is that site's business"
    );
    let walked = walk(root, None, None);
    let rels: Vec<&str> = walked.files.iter().map(|f| f.rel.as_str()).collect();
    assert_eq!(rels, vec!["here.md"], "and its subtree is never walked into");
}

/// Outside `.moss/` the watcher's filter still decides — the sweep must not
/// start reading `.git/` objects because the `.moss/` half changed.
#[test]
fn outside_moss_the_watcher_filter_still_decides() {
    let folder = Path::new("/vault");
    assert!(should_request(folder, &folder.join("posts/hello.md")));
    assert!(should_request(folder, &folder.join("photos/cat.jpg")));
    assert!(!should_request(folder, &folder.join(".git/HEAD")));
    assert!(!should_request(folder, &folder.join("notes/scratch.xyz")));
}

/// `drift_eligible` used to defer its extension question entirely
/// to `scope::path_passes_filter`, a hand-maintained watcher-side list that
/// never included `.html`/`.htm` and still doesn't include word-processor
/// sources — both real scan buckets (`classify::ScanBucket::Html` and
/// `::Document`). Asking the scan's own classifier directly closes that gap
/// without a second extension list to remember, while an extension the
/// classifier does not recognize at all (`.xyz`) stays ineligible exactly as
/// before — `outside_moss_the_watcher_filter_still_decides` above pins that
/// half.
#[test]
fn scan_recognized_extensions_are_drift_eligible_even_when_the_watcher_filter_missed_them() {
    let folder = Path::new("/vault");
    for rel in ["page.html", "page.htm", "report.docx", "notes.pages", "manual.doc"] {
        assert!(
            drift_eligible(folder, &folder.join(rel)),
            "`{rel}` is a scan-recognized source and must be drift-eligible"
        );
    }
}

/// The relocation invariant, at the sweep's two predicates: **where the user
/// keeps the vault must not change the verdict on a file inside it.**
///
/// An earlier bug was the violation — a vault synced under a hidden folder lives under
/// `.hidden-ancestor`, `path_is_watchable` rejects a dotfile per path
/// COMPONENT, and handing it the absolute path therefore condemned every file
/// in the vault: the sweep came back empty, nothing reached the readers, and
/// the waiting screen counted down over a download nobody had asked for.
///
/// Crossed over `scope::VAULT_MOUNTS` rather than the one mount that was
/// reported. The previous version of this test named `.hidden-ancestor`
/// alone, and the coverage for every OTHER dotted mount was resting on
/// `TempDir::new()` happening to mint a `.tmpXXXXXX` prefix elsewhere in the
/// suite — a temp-dir implementation detail a cleanup could disarm silently.
///
/// A pure-function assertion on purpose: the only coverage this had was a
/// `#[cfg(target_os = "macos")]` test, and CI is Linux.
#[test]
fn a_relocated_vault_is_swept_the_same_way() {
    use crate::build::watch::scope::{mount_join, VAULT_MOUNTS};
    for (shape, mount) in VAULT_MOUNTS {
        let folder = Path::new(mount);
        for rel in ["index.md", "notes/post.md"] {
            let path = mount_join(mount, rel);
            assert!(should_request(folder, &path), "`{rel}` under {shape}");
            assert!(drift_eligible(folder, &path), "`{rel}` under {shape}");
        }
        // moss's own inputs are requested (they cannot be regenerated) but
        // never drift-compared (the manifest does not carry them).
        let config = mount_join(mount, ".moss/config.toml");
        assert!(should_request(folder, &config), "moss's own inputs, under {shape}");
        assert!(!drift_eligible(folder, &config), "under {shape}");
        // The dot rule still applies to what is INSIDE the vault — that is the
        // whole reason the predicate is consulted at all.
        for rel in [".git/HEAD", "node_modules/x/index.js", "notes/scratch.xyz"] {
            let path = mount_join(mount, rel);
            assert!(!should_request(folder, &path), "`{rel}` under {shape}");
            assert!(!drift_eligible(folder, &path), "`{rel}` under {shape}");
        }
    }
}

// ── "no baseline this pass" log gate (2026-09-15) ─────────────────────────

/// The whole point of the gate: a long wait for the first seal used to log
/// this line every tick (measured ~560 lines/session in a real upload).
#[test]
fn no_baseline_transition_logs_only_on_the_streak_first_pass() {
    assert!(should_log_no_baseline_transition(1), "first pass of a streak");
    for n in [2, 3, 10, 500] {
        assert!(!should_log_no_baseline_transition(n), "pass {n} is mid-streak");
    }
}

/// A fresh streak (the counter reset to 0 by a baseline appearing, then
/// incremented again) must log again — this is "once per streak", not
/// "once ever".
#[test]
fn a_new_streak_after_a_baseline_logs_again() {
    // Streak 1: passes 1, 2, 3. Streak 2 (after a baseline appeared and the
    // caller reset the counter to 0): passes 1, 2 again.
    let streak1: Vec<bool> = (1..=3).map(should_log_no_baseline_transition).collect();
    let streak2: Vec<bool> = (1..=2).map(should_log_no_baseline_transition).collect();
    assert_eq!(streak1, vec![true, false, false]);
    assert_eq!(streak2, vec![true, false]);
}

// ── Rebuild pacing (ported verbatim — semantics unchanged) ────────────────

/// The defect itself, pinned. The old condition was
/// `now - first_arrival_at >= 1500ms`, which during a bulk download — where
/// arrivals never pause — fired every 1.5s + build duration forever: 10
/// rebuilds in 95 seconds, 9 of them producing no output change.
#[test]
fn a_drain_does_not_rebuild_at_the_old_debounce() {
    let t0 = Instant::now();
    assert!(
        !should_rebuild(Some(t0), Some(t0), Duration::ZERO, t0 + Duration::from_millis(1500)),
        "1.5s after a batch opened is not a reason to rebuild the whole site again"
    );
}

/// Staleness has to eventually fire on its own, or pacing degenerates into
/// "never rebuild until the download finishes" and a user watching a
/// 20-minute drain sees nothing land.
#[test]
fn a_drain_rebuilds_once_the_floor_has_elapsed() {
    let t0 = Instant::now();
    assert!(should_rebuild(Some(t0), Some(t0), Duration::ZERO, t0 + MIN_REBUILD_INTERVAL));
}

/// The self-tuning half. A vault whose rebuild costs 9s must not be rebuilt
/// every 8s — that is a >100% duty cycle, which is the original bug at a
/// slightly slower tempo.
///
/// Live in production since phase 3: the worker records each completed
/// admission's finish time and wall duration (`WorkerHandle::last_completed`),
/// and the sweep's section (5) feeds them into this function whenever they
/// are newer than its own enqueue stamp — so `last_rebuild_duration` is the
/// real build cost, measured from finish.
#[test]
fn a_long_previous_rebuild_lengthens_the_interval_it_earns() {
    let t0 = Instant::now();
    let cost = Duration::from_secs(9);
    assert!(cost > MIN_REBUILD_INTERVAL, "the floor must not be doing the work here");

    assert!(
        !should_rebuild(Some(t0), Some(t0), cost, t0 + MIN_REBUILD_INTERVAL),
        "the floor does not override a measured cost above it"
    );
    assert!(should_rebuild(Some(t0), Some(t0), cost, t0 + cost));
}

/// A quiet gap must NOT be a shortcut past the floor. Arrivals are sampled
/// once per TICK, so a provider delivering in bursts looks quiet on every
/// tick between bursts — a "rebuild when quiet" condition would fire on all
/// of them and reproduce the storm at the burst cadence.
#[test]
fn a_lull_between_bursts_does_not_earn_an_early_rebuild() {
    let t0 = Instant::now();
    let now = t0 + TICK * 2;
    assert!(now.duration_since(t0) < MIN_REBUILD_INTERVAL, "premise: still inside the floor");
    assert!(!should_rebuild(Some(t0), Some(t0), Duration::ZERO, now));
}

/// Staleness is a permission, not a schedule. Without the `first_arrival_at`
/// check the sweep would rebuild an idle, fully-materialized vault every
/// MIN_REBUILD_INTERVAL for as long as the folder stayed open.
#[test]
fn nothing_arriving_never_rebuilds_however_stale() {
    let t0 = Instant::now();
    for elapsed in [MIN_REBUILD_INTERVAL, Duration::from_secs(600)] {
        assert!(
            !should_rebuild(None, Some(t0), Duration::ZERO, t0 + elapsed),
            "elapsed={elapsed:?}"
        );
    }
}

/// The first batch after a cold open is the one the waiting screen is
/// blocked on. Pacing exists to stop a repeating storm, and there is nothing
/// yet to repeat — holding this one for MIN_REBUILD_INTERVAL would add 8s to
/// every cold open of a partly-evicted vault.
#[test]
fn the_first_rebuild_of_an_episode_is_not_held_back() {
    let t0 = Instant::now();
    assert!(should_rebuild(
        Some(t0),
        None, // nothing rebuilt yet this episode
        Duration::ZERO,
        t0 + Duration::from_millis(200),
    ));
}

// ── Progress phases ────────────────────────────────────────────────────────

#[test]
fn a_long_wait_is_stalled_but_still_pending() {
    assert_eq!(phase_for(true), "stalled");
    assert_eq!(phase_for(false), "materializing");
}

// ── What the site is waiting for ──────────────────────────────

/// The count the full-window screen is keyed on is the render-relevant slice
/// of what moss is downloading, not all of it. Everything under `.moss/` is
/// requested, so `pending` routinely carries files no page renders
/// from — the reported incident was one of them holding the screen over a site
/// that had already built and was being served.
#[test]
fn only_render_relevant_files_are_counted_as_blocking() {
    let pending: HashSet<PathBuf> = [
        "/V/.moss/data/deployed-article-map.json", // bookkeeping — not the site's
        "/V/.moss/identity/secret-key",            // no extension at all
        "/V/photos/cover.jpg",                     // decoration
        "/V/config.toml",                          // genuinely render-blocking
        "/V/.moss/theme/style.css",                // as is the theme
        "/V/posts/hello.md",                       // and the page itself
    ]
    .iter()
    .map(PathBuf::from)
    .collect();

    assert_eq!(blocking_count(&pending), 3);
}

#[test]
fn a_long_wait_never_removes_a_structural_file_from_the_count() {
    let pending: HashSet<PathBuf> =
        ["/V/posts/hello.md", "/V/config.toml"].iter().map(PathBuf::from).collect();
    assert_eq!(blocking_count(&pending), 2);
}

/// The two counts and the phase, read together on the incident's own shape:
/// one non-structural file outstanding. The panel still honestly says moss is
/// fetching something (`materializing`, one remaining), the screen stays down,
/// and — because `stalled` is computed over the blocking count — the mandatory
/// 90-second "Downloads have stalled" window never opens for this class.
#[test]
fn a_pending_set_with_nothing_render_relevant_never_stalls() {
    let pending: HashSet<PathBuf> =
        ["/V/.moss/data/deployed-article-map.json"].iter().map(PathBuf::from).collect();

    let blocking = blocking_count(&pending);
    assert_eq!(blocking, 0);

    // What the sweep does with it: no stall however long the silence runs,
    // because the stall test is `blocking > 0`.
    let stalled = blocking > 0;
    assert!(!stalled);
    assert_eq!(phase_for(stalled), "materializing");
}

// ── The pass deadline and root health ─────────────────────────────────────

/// A pass whose deadline is already spent reports `deadline_blown` instead
/// of running unbounded — the sweep's "must not wedge silently" rule. On a
/// network filesystem a single stat can stall; the in-walk check is what
/// bounds the pass when the stats are merely slow rather than stuck.
#[test]
fn a_spent_deadline_blows_the_pass_instead_of_walking() {
    let dir = tempfile::Builder::new().prefix("moss_sweep").tempdir().unwrap();
    std::fs::write(dir.path().join("index.md"), "# Home").unwrap();
    let out = walk(dir.path(), Some(Instant::now() - Duration::from_secs(1)), None);
    assert!(out.deadline_blown);
    assert!(out.files.is_empty(), "no verdicts from a pass that could not finish");
}

/// A missing root is `root_unreadable`, never an empty result — an empty
/// result would read as "every file was deleted" and dispatch the rebuild
/// storm this flag exists to prevent.
#[test]
fn a_missing_root_is_unreadable_not_empty() {
    let out = walk(Path::new("/nonexistent/moss/vault"), None, None);
    assert!(out.root_unreadable);
    assert!(out.files.is_empty());
}

// ── Invariant 1: everything the walk judges, the scan consumes ────────────

/// Every drift-eligible file the walk reports must be one the build's scan
/// would consume into the manifest — otherwise the file can never appear in
/// `sources`, "not in baseline" reads as new-file drift, and the sweep
/// rebuilds forever (the sticky-drift guard caps that at one wasted rebuild,
/// but the invariant is what makes it not happen at all). Pinned over a
/// fixture holding one of each population the two filters disagree on.
#[test]
fn everything_the_walk_judges_the_scan_consumes() {
    let dir = tempfile::Builder::new().prefix("moss_sweep").tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("posts")).unwrap();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
    std::fs::create_dir_all(root.join("inner/.moss")).unwrap();
    std::fs::create_dir_all(root.join(".moss/theme")).unwrap();
    std::fs::write(root.join("index.md"), "# Home").unwrap();
    std::fs::write(root.join("posts/hello.md"), "# Hello").unwrap();
    std::fs::write(root.join("posts/cat.jpg"), b"jpg").unwrap();
    std::fs::write(root.join("page.html"), "<html></html>").unwrap();
    std::fs::write(root.join("AGENTS.md"), "moss-written").unwrap();
    std::fs::write(root.join(".git/HEAD"), "ref").unwrap();
    std::fs::write(root.join("node_modules/pkg/index.js"), "js").unwrap();
    std::fs::write(root.join("inner/there.md"), "# Nested site").unwrap();
    std::fs::write(root.join(".moss/config.toml"), "").unwrap();
    std::fs::write(root.join(".moss/theme/style.css"), "").unwrap();
    // The asset populations the walk and scan must agree on — each extension
    // routes through a different scan bucket (other_files, audio, fonts) and
    // a draft is a page the BUILD may skip rendering but the scan still
    // consumes.
    //
    // What this test does NOT establish, and once wrongly claimed it did: that
    // a scanned-but-unrendered file reaches the MANIFEST. Drift compares
    // against `sources`, not against the scan, and the only writer of page
    // entries runs per emitted output — so slot-only `footer.md` was scanned,
    // walked, and permanently absent from the baseline, which read as new-file
    // drift on every pass (see `slot_only_sources_appear_in_sealed_sources` in
    // `build/pipeline_tests.rs`, which asserts the walk-⊆-MANIFEST leg through
    // a real build because only a build exercises that wiring).
    std::fs::write(root.join("style.css"), "body{}").unwrap();
    std::fs::write(root.join("app.js"), "1;").unwrap();
    std::fs::write(root.join("paper.pdf"), b"%PDF-1.4").unwrap();
    std::fs::write(root.join("song.mp3"), b"ID3").unwrap();
    std::fs::write(root.join("type.woff2"), b"wOF2").unwrap();
    std::fs::write(root.join("posts/draft.md"), "---\ndraft: true\n---\n# WIP").unwrap();
    // Slot-only chrome: scanned, walked, and rendered into no page of its own.
    std::fs::write(root.join("footer.md"), "chrome").unwrap();

    let structure =
        crate::build::scan::scan::scan_folder(&root.to_string_lossy()).expect("scan fixture");
    let scanned: std::collections::HashSet<String> = structure
        .markdown_files
        .iter()
        .chain(&structure.html_files)
        .chain(&structure.notebook_files)
        .chain(&structure.other_files)
        .map(|f| f.path.clone())
        .chain(structure.image_files.iter().chain(&structure.video_files).map(|m| {
            m.path.clone()
        }))
        .collect();

    let walked = walk(root, None, None);
    assert!(!walked.files.is_empty(), "premise: the fixture is walkable");
    for f in &walked.files {
        assert!(
            scanned.contains(&f.rel),
            "walk judges '{}' but the scan never consumes it — it can never reach the \
             manifest, so it would drift forever (fix drift_eligible or the scan)",
            f.rel
        );
    }
}

// ── Invariant 2: one path normalizer ──────────────────────────────────────

/// The walk keys files exactly the way the manifest does — through the same
/// `path_to_relative_key` the event pump uses. Two normalizers is how a path
/// matches its own baseline entry on one side and not the other.
#[test]
fn the_walk_keys_files_the_manifest_way() {
    let dir = tempfile::Builder::new().prefix("moss_sweep").tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("图片/nested")).unwrap();
    std::fs::write(root.join("图片/nested/照 片.jpg"), b"jpg").unwrap();

    let walked = walk(root, None, None);
    assert_eq!(walked.files.len(), 1);
    assert_eq!(
        Some(walked.files[0].rel.clone()),
        crate::build::watch::path_to_relative_key(root, &root.join("图片/nested/照 片.jpg")),
        "same key the pump would derive for the same file"
    );
    assert_eq!(walked.files[0].rel, "图片/nested/照 片.jpg", "forward slashes, original case");
}

/// The counters behind the blind-folder warning, measured on a real tree.
///
/// The pair is what discriminates the two states the log could not tell
/// apart: a quiet vault (files seen, files watchable, nothing drifting) from
/// a vault moss cannot see into (files seen, none watchable). The fixture
/// mounts under `TempDir`'s own `.tmpXXXXXX` prefix, so a dot-prefixed
/// ancestor is in the measurement rather than beside it.
#[test]
fn the_walk_counts_what_it_can_see_against_what_it_may_watch() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(root.join("posts")).unwrap();
    std::fs::write(root.join(".git/HEAD"), b"ref").unwrap();
    std::fs::write(root.join("posts/hello.md"), b"# hi").unwrap();

    let walked = walk(root, None, None);
    assert!(walked.files_seen > 0, "the folder is not empty");
    assert!(
        walked.files_watchable > 0,
        "a vault under a dot-prefixed ancestor is still a vault moss can see into"
    );

    // The blind shape: files the walk reaches, none of which it may watch.
    // Dot-DIRECTORIES and `node_modules` are pruned before the walk reaches
    // a file at all, so what is left to be excluded per-file is a dotfile —
    // which is why the honest way to reach this state is the defect itself,
    // a dot-prefixed component ABOVE the vault voting on everything below it.
    std::fs::remove_file(root.join("posts/hello.md")).unwrap();
    std::fs::write(root.join("posts/.cover.jpg"), b"x").unwrap();
    std::fs::write(root.join(".DS_Store"), b"x").unwrap();
    let walked = walk(root, None, None);
    assert!(walked.files_seen > 0, "the walk still reaches them");
    assert_eq!(walked.files_watchable, 0, "and may watch none of them");
}

// ── Invariant 3: the baseline read fails SAFE ─────────────────────────────

/// No baseline means NO drift verdict — never an empty baseline. An empty
/// one makes every file on disk read as new-file drift, and when the cause
/// is `hashes.json` itself being evicted (it lives under an evictable
/// `.moss/build.nosync/`), that is a rebuild loop with no exit.
#[test]
fn a_missing_or_corrupt_manifest_yields_no_baseline_not_an_empty_one() {
    let dir = tempfile::Builder::new().prefix("moss_sweep").tempdir().unwrap();
    let root = dir.path();

    // Absent: nothing built yet.
    assert!(read_baseline_from_disk(root).is_none());

    // Corrupt: a write-crash or partial sync.
    let hashes = crate::moss_paths::MossPaths::new(root).hashes();
    std::fs::create_dir_all(hashes.parent().unwrap()).unwrap();
    std::fs::write(&hashes, "{ not json").unwrap();
    assert!(read_baseline_from_disk(root).is_none());

    // Valid: the one shape that produces a verdict.
    let valid = crate::types::content::SiteHashes::new();
    std::fs::write(&hashes, serde_json::to_string(&valid).unwrap()).unwrap();
    assert!(read_baseline_from_disk(root).is_some());
}

// ── Sticky drift: a rebuild that clears nothing is dispatched once ────────

/// The fingerprint is what separates "this file drifted again because the
/// user edited it again" (dispatch) from "this file survived its rebuild
/// untouched" (suppress): same set, same stats → equal; any edit — even a
/// SAME-SIZE re-edit landing inside the same second, the class of blindness
/// `mtime_nanos` exists for — must move it, or the re-edit is wrongly
/// suppressed and the preview stays stale until the file changes again.
/// The stamp reads the stat each pass's walk carried, so "a pass" here is a
/// fresh `WalkedFile` per write.
#[test]
fn the_drift_fingerprint_moves_when_the_file_does() {
    let dir = tempfile::Builder::new().prefix("moss_sweep").tempdir().unwrap();
    let abs = dir.path().join("a.md");
    let stamp_now = |abs: &PathBuf| drift::stamp_of(std::fs::metadata(abs).ok().as_ref());

    std::fs::write(&abs, "one").unwrap();
    let first = stamp_now(&abs);
    assert_eq!(first, stamp_now(&abs), "untouched → equal");

    // Same byte count, a few milliseconds later — same wall-clock second in
    // practice, and always the same second-granular stamp class the old
    // fingerprint keyed on. Nanos and ctime are what tell the passes apart.
    // (The kernel's coarse file-time clock ticks every few ms; two writes
    // inside ONE tick are indistinguishable by stat on any filesystem, so
    // the test steps past that floor rather than pretending it isn't there.)
    std::thread::sleep(Duration::from_millis(15));
    std::fs::write(&abs, "two").unwrap();
    assert_ne!(
        first,
        stamp_now(&abs),
        "a same-size same-second re-edit is a new drift, not a sticky one"
    );
}

// ── The blown-pass streak is its own unavailability verdict ───────────────

/// A chronically-stalling filesystem keeps its root readable — read_dir on
/// the top level answers fast while every full pass blows its deadline. A
/// shared streak that section (1) resets on each readable tick could never
/// reach the threshold, so the folder would ERROR forever without ever
/// going unavailable. Three consecutive blown passes with a readable root
/// must flip the verdict.
#[test]
fn three_blown_passes_with_a_readable_root_go_unavailable() {
    let session = FolderSession::new(PathBuf::from("/tmp/moss-sweep-blown-test"));
    let mut blown_streak: u32 = 0;
    for _ in 0..UNAVAILABLE_AFTER {
        assert!(!session.is_unavailable(), "not before the threshold");
        note_failed_pass(&session, "test", &mut blown_streak, "pass deadline blown");
    }
    assert!(session.is_unavailable(), "the streak is the verdict");
    // And a healthy pass ends the episode: the loop resets the streak, and
    // section (1)'s gate (`blown_streak < UNAVAILABLE_AFTER`) may clear it.
    blown_streak = 0;
    assert!(blown_streak < UNAVAILABLE_AFTER, "the gate opens once the streak resets");
    assert!(session.set_unavailable(false), "the clear sees the transition (returns previous)");
    assert!(!session.is_unavailable());
}

// ── The resume cursor: blown passes still cover the whole tree ────────────

/// A tree too big for one deadline must not be judged head-first forever.
/// The walk is deterministic (sorted), the cursor is the last processed
/// key, and a resumed pass starts strictly AFTER it — so consecutive
/// partial passes tile the tree instead of re-walking the same prefix.
#[test]
fn a_resumed_walk_starts_after_the_cursor_and_tiles_the_tree() {
    let dir = tempfile::Builder::new().prefix("moss_sweep").tempdir().unwrap();
    let root = dir.path();
    for name in ["a.md", "b.md", "c.md", "d.md"] {
        std::fs::write(root.join(name), "# x").unwrap();
    }

    let full: Vec<String> = walk(root, None, None).files.into_iter().map(|f| f.rel).collect();
    assert_eq!(full, vec!["a.md", "b.md", "c.md", "d.md"], "deterministic order");

    let resumed: Vec<String> =
        walk(root, None, Some("b.md")).files.into_iter().map(|f| f.rel).collect();
    assert_eq!(resumed, vec!["c.md", "d.md"], "strictly after the cursor, to the end");

    // First shard ∪ second shard = the tree: coverage across passes.
    let mut union: Vec<String> = full.iter().take(2).cloned().collect();
    union.extend(resumed);
    assert_eq!(union, full);
}

/// A pass whose budget is ALREADY spent judges nothing — and must hand the
/// caller's cursor back rather than dropping it, or one fully-starved pass
/// resets coverage to the top of the tree and the deep half is never
/// reached again.
#[test]
fn a_starved_walk_keeps_the_cursor_it_was_given() {
    let dir = tempfile::Builder::new().prefix("moss_sweep").tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.md"), "# x").unwrap();
    std::fs::write(root.join("b.md"), "# x").unwrap();

    let spent = Instant::now() - Duration::from_secs(1);
    let out = walk(root, Some(spent), Some("a.md"));
    assert!(out.deadline_blown);
    assert_eq!(out.resume.as_deref(), Some("a.md"), "the cursor survives a zero-progress pass");
}

// ── Sticky drift is a per-PATH verdict, not a per-SET one ─────────────────

fn stamp(n: u64) -> crate::build::watch::drift::DriftStamp {
    (n, n as i64, 0, None, None)
}

fn fp(entries: &[(&str, u64)]) -> std::collections::BTreeMap<String, crate::build::watch::drift::DriftStamp> {
    entries.iter().map(|(r, n)| ((*r).to_string(), stamp(*n))).collect()
}

/// The same entries as a drift report's own map — kind is irrelevant to
/// suppression, which keys on the stamp alone.
fn drifts(entries: &[(&str, u64)]) -> std::collections::BTreeMap<String, drift::Drift> {
    entries
        .iter()
        .map(|(r, n)| {
            ((*r).to_string(), drift::Drift { kind: drift::DriftKind::Edited, stamp: stamp(*n) })
        })
        .collect()
}

/// THE bug. Suppression was keyed on the whole drift set, so one unrelated
/// path joining or leaving re-dispatched every path in it — including the
/// ones a rebuild had already provably failed to clear. A set that
/// oscillates therefore never suppresses at all: on the vault that motivated
/// this, the sweep's set alternated 877 ↔ 1102 and the guard's WARN never
/// fired once across three minutes of continuous rebuilding.
#[test]
fn a_neighbour_joining_the_drift_set_does_not_re_dispatch_the_rest() {
    let dispatched = fp(&[("survived.md", 1)]);
    let now = drifts(&[("survived.md", 1), ("genuinely-new.md", 7)]);
    assert_eq!(
        undispatched_paths(&now, &dispatched),
        vec!["genuinely-new.md".to_string()],
        "only the path whose stamp the previous dispatch never saw"
    );
}

/// The suppression must not become permanent: a path that changes again is
/// a new incident, and the whole point of the sweep is to catch it.
#[test]
fn a_path_whose_stamp_moved_is_dispatched_again() {
    let dispatched = fp(&[("page.md", 1)]);
    let now = drifts(&[("page.md", 2)]);
    assert_eq!(undispatched_paths(&now, &dispatched), vec!["page.md".to_string()]);
}

/// The behaviour the set-keyed guard already had, kept: a drift set that
/// comes back byte-identical after its rebuild dispatches nothing.
#[test]
fn a_set_that_survived_its_rebuild_unchanged_dispatches_nothing() {
    let dispatched = fp(&[("a.md", 1), ("b.md", 2)]);
    let now = drifts(&[("a.md", 1), ("b.md", 2)]);
    assert!(undispatched_paths(&now, &dispatched).is_empty());
}

/// A sweep run straight after a build must find nothing.
///
/// The watcher's other half of an earlier bug class. The sweep walks the
/// vault every 2 s and compares what it finds against `hashes.json`'s
/// `sources`; a walked file with no baseline entry reads as NEW, which is
/// drift, which dispatches a rebuild that cannot clear it. Two derivations of
/// "every file this build knows about" — the walk's, and whatever the render
/// loops happened to register — and when they disagree the disagreement is
/// permanent.
///
/// `footer.md` is the fixture that makes them differ, and it is the file that
/// actually did it. A slot-only source renders into every page's chrome and
/// owns no output, so both manifest-registration loops — each keyed on an
/// emitted output — skipped it and its hash never entered `sources`. On a
/// synced-folder site that meant every 2 s pass judged both footers as drift,
/// dispatched a full rebuild that could not clear them, then blamed the
/// watcher and recreated it; after two strikes the folder degraded to
/// sweep-only and event-driven rebuilds stopped happening at all.
///
/// The existing invariant next door asserts walk ⊆ SCAN, which `footer.md`
/// satisfies — it is a markdown file the scan consumes happily. The
/// invariant that was missing is this one, walk ⊆ MANIFEST, and only a real
/// build can answer it.
///
/// **Includes a root `style.css`** . A vault asset moss neither
/// copies nor reads — `.moss/theme/` is the canonical stylesheet location,
/// and a root-level `style.css` is deliberately ignored, warned about by
/// `check_misplaced_theme_files` rather than auto-moved — used to reach
/// neither `sources` nor `files`, so the walk judged it present and the
/// manifest never could, drifting every pass exactly as `footer.md` did.
/// Fixed by giving the sweep's `drift_eligible` filter the same
/// `is_ignored_root_theme_file` predicate the asset pass uses to skip it,
/// so the walk's domain and the build's domain cannot disagree about a file
/// neither of them will ever act on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sweep_straight_after_a_build_finds_no_drift() {
    // Not `tempfile::tempdir()`: that mints a dot-prefixed `/tmp/.tmpXXXX`,
    // and a dot-prefixed ancestor is excluded by the vault path predicates —
    // the build then records no sources and every file "drifts".
    let tmp = tempfile::Builder::new().prefix("moss-sweep-test-").tempdir().unwrap();
    let test_dir = tmp.path().to_path_buf();
    let folder_path = test_dir.to_str().unwrap();

    std::fs::write(test_dir.join("index.md"), "# Home\n").unwrap();
    std::fs::write(test_dir.join("about.md"), "# About\n").unwrap();
    // Slot-only: reserved filename, renders into chrome, owns no output.
    std::fs::write(test_dir.join("footer.md"), "Made with moss.\n").unwrap();
    std::fs::create_dir_all(test_dir.join("posts")).unwrap();
    std::fs::write(test_dir.join("posts/one.md"), "# One\n").unwrap();
    // Deliberately ignored: canonical location is .moss/theme/style.css.
    std::fs::write(test_dir.join("style.css"), "body { margin: 0 }\n").unwrap();

    build_once(folder_path).await;

    let baseline = read_baseline_from_disk(&test_dir)
        .expect("a completed build writes a readable hashes.json");
    let walked = walk(&test_dir, None, None);
    assert!(!walked.files.is_empty(), "premise: the vault is walkable");

    let report = crate::build::watch::drift::detect_drift(
        &baseline,
        &walked.files,
        &[],
        &test_dir,
        crate::build::watch::drift::probe_missing,
        None,
        true,
    );

    assert!(
        report.is_quiet(),
        "the disk has not changed since the build, so the sweep must be quiet; \
         it reported drift on {:?} — a file the walk judges but the manifest \
         never recorded will drift on every pass, forever",
        report.drifted
    );
}

/// A rebuild dispatched for a genuine edit must not be
/// permanently sticky-suppressed just because an UNRELATED structural
/// source went briefly unreadable (cloud eviction) during that same
/// rebuild. Since a build always promotes and seals (carrying `other.md`
/// forward instead of withholding), `keeper.md`'s edit actually lands in
/// the new baseline, so the next pass's compare finds it quiet — the sticky
/// breaker's own assumption (an unchanged stamp means the dispatch it saw
/// already succeeded) holds.
#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_evicted_sibling_does_not_strand_a_genuine_edit_as_sticky() {
    let tmp = tempfile::Builder::new().prefix("moss-sweep-test-").tempdir().unwrap();
    let test_dir = tmp.path().to_path_buf();
    let folder_path = test_dir.to_str().unwrap();

    std::fs::write(test_dir.join("index.md"), "# Home\n").unwrap();
    std::fs::write(test_dir.join("keeper.md"), "# Keeper\n\nOriginal.\n").unwrap();
    std::fs::write(test_dir.join("other.md"), "# Other\n\nOriginal.\n").unwrap();

    build_once(folder_path).await;
    let baseline1 =
        read_baseline_from_disk(&test_dir).expect("first build writes a readable hashes.json");

    // A genuine edit, plus an unrelated structural sibling going unreadable
    // (pre-Sonoma cloud-eviction placeholder) in the same window — the
    // shape a synced vault actually produces when a rebuild races eviction.
    std::fs::write(test_dir.join("keeper.md"), "# Keeper\n\nEdited.\n").unwrap();
    std::fs::remove_file(test_dir.join("other.md")).unwrap();
    std::fs::write(test_dir.join(".other.md.icloud"), "").unwrap();

    let walked1 = walk(&test_dir, None, None);
    let report1 = crate::build::watch::drift::detect_drift(
        &baseline1,
        &walked1.files,
        &[],
        &test_dir,
        crate::build::watch::drift::probe_missing,
        None,
        true,
    );
    assert!(
        report1.drifted.contains_key("keeper.md"),
        "premise: the edit must be seen as drift: {:?}",
        report1.drifted
    );
    let mut dispatched_drift: std::collections::BTreeMap<String, crate::build::watch::drift::DriftStamp> =
        std::collections::BTreeMap::new();
    for (rel, d) in &report1.drifted {
        dispatched_drift.insert(rel.clone(), d.stamp);
    }

    // The dispatched rebuild: `other.md` is still unreadable throughout.
    build_once(folder_path).await;
    let baseline2 = read_baseline_from_disk(&test_dir)
        .expect("the dispatched rebuild must still write a readable hashes.json");

    // Nothing on disk changes further before the next pass.
    let walked2 = walk(&test_dir, None, None);
    let report2 = crate::build::watch::drift::detect_drift(
        &baseline2,
        &walked2.files,
        &[],
        &test_dir,
        crate::build::watch::drift::probe_missing,
        None,
        true,
    );
    assert!(
        !report2.drifted.contains_key("keeper.md"),
        "the dispatched rebuild must have actually captured keeper.md's edit \
         into the new baseline — a build withheld by other.md's eviction \
         would leave keeper.md drifting at its already-dispatched stamp: {:?}",
        report2.drifted
    );
    assert!(
        undispatched_paths(&report2.drifted, &dispatched_drift).is_empty(),
        "keeper.md's edit must not need re-dispatch — it already landed"
    );
}

// ── Folder health: degraded is an OR, not just the worker watchdog ─────────

/// `WatcherHealth::is_degraded()` folds suppressed watcher backoff into
/// `FolderHealthChanged.degraded` — previously that flag reflected only the
/// rebuild worker's overdue-build watchdog, so an extended backoff (rename
/// fidelity and `.moss/theme` / `.moss/config.toml` going stale) had no
/// user-visible signal at all. No worker is registered for this folder —
/// `folder_degraded(None, …)` — so a `true` result here can only come from
/// the watcher half of the OR.
#[test]
fn folder_degraded_reflects_a_suppressed_watcher_backoff_with_no_worker_registered() {
    let folder = format!("moss-sweep-test-watcher-degraded-{:?}", std::thread::current().id());
    let health = crate::ops::watch::supervision::register(&folder);

    assert!(!folder_degraded(None, &folder), "no strike has landed yet");

    // First strike: honored immediately (no prior recreation) — ordinary
    // self-heal, not degraded.
    assert!(health.strike(&folder), "first strike is always honored");
    assert!(
        !folder_degraded(None, &folder),
        "one honored recreation is the design's ordinary self-heal, not a degraded episode"
    );

    // Second strike lands inside the backoff window (RECREATE_BACKOFF_BASE =
    // 60s, elapsed here is microseconds) — suppressed, which is what
    // `is_degraded` means.
    assert!(!health.strike(&folder), "second strike must be suppressed by backoff");
    assert!(
        folder_degraded(None, &folder),
        "a suppressed strike must surface through folder_degraded even though no worker is registered"
    );

    crate::ops::watch::supervision::deregister(&folder, &health);
}

// ── Lifecycle: a sweep ends with its session ──────────────────────────────

/// A yielded process cancels the folder's session; the sweep must stop and
/// free its claim so a resume can start a fresh one. Without the claim being
/// freed, the resumed `start` would find the stale key and start nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_the_session_stops_the_sweep_and_frees_its_claim() {
    let tmp = tempfile::Builder::new().prefix("moss-sweep-life-").tempdir().unwrap();
    let folder = tmp.path().to_path_buf();
    let key = folder.to_string_lossy().to_string();
    let session = FolderSession::new(folder);
    let dispatch: crate::ops::watch::RebuildDispatch = std::sync::Arc::new(|_| Box::pin(async {}));
    start(session.clone(), host(dispatch)).await;
    assert!(sweeps().contains_key(&key), "the sweep claims its folder");

    session.cancel.cancel();
    let deadline = Instant::now() + Duration::from_secs(15);
    while sweeps().contains_key(&key) {
        assert!(Instant::now() < deadline, "the sweep outlived its cancelled session");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A reopen registers its new session before the old session's cancellation
/// has been polled, so the old claim is still in the map when the new start
/// arrives. The new start must replace it, and the old task's later exit must
/// not free the new claim.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_replaces_the_claim_of_a_cancelled_session() {
    let tmp = tempfile::Builder::new().prefix("moss-sweep-reopen-").tempdir().unwrap();
    let folder = tmp.path().to_path_buf();
    let key = folder.to_string_lossy().to_string();
    let dispatch: crate::ops::watch::RebuildDispatch = std::sync::Arc::new(|_| Box::pin(async {}));

    let old = FolderSession::new(folder.clone());
    start(old.clone(), host(dispatch.clone())).await;
    let old_gen = sweeps().get(&key).map(|c| c.gen).expect("the first sweep claims its folder");

    old.cancel.cancel();
    let new = FolderSession::new(folder);
    start(new.clone(), host(dispatch)).await;
    let new_gen = sweeps().get(&key).map(|c| c.gen).expect("the reopened folder is claimed");
    assert_ne!(new_gen, old_gen, "the reopened session must own the claim");

    // Let the old task drop; its claim must not remove the new one.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(sweeps().get(&key).map(|c| c.gen), Some(new_gen));

    new.cancel.cancel();
}

fn host(dispatch: crate::ops::watch::RebuildDispatch) -> SweepHost {
    SweepHost {
        dispatch,
        reporter: crate::build::ports::reporter::discard_owned(),
        emit: std::sync::Arc::new(|_| {}),
        cadence: tokio::sync::watch::channel(crate::ops::watch::cadence::Cadence::Live).1,
    }
}

/// The walk does not enter a nested site: its files are that site's to fetch
/// and compare.
#[test]
fn the_walk_does_not_enter_a_nested_site() {
    let dir = tempfile::Builder::new().prefix("moss_walk").tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("inner/.moss")).unwrap();
    std::fs::create_dir_all(root.join("posts")).unwrap();
    std::fs::write(root.join("inner/x.md"), b"# x").unwrap();
    std::fs::write(root.join("posts/a.md"), b"# a").unwrap();
    assert_eq!(walk(root, None, None).files_seen, 1);
}
