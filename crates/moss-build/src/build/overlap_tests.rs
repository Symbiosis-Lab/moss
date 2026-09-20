//! The real overlapping-build sequence, end to end.
//!
//! `stage_dir` is one persistent directory per open folder. A build renders under
//! the stage-write lock, drops it, and its DETACHED seal tail later re-acquires the
//! lock and copies files out of `stage_dir` into an immutable generation directory
//! (`ship_phase`). A second build can render in between, so the first build's
//! generation may receive the second build's bytes under the first build's frozen
//! manifest hash — deploy then refuses it ("file bytes do not match sealed
//! manifest"). The race was closed class by class (deferred assets, image variants,
//! rendered HTML, then the derived files) by giving a manifest entry an immutable
//! source: a CAS object id, or for the derived files the bytes themselves
//! (`ShipSource::Held`), so `ship_phase` never reads the mutable stage. Every unit
//! test for that overwrites one stage file once; this runs the whole sequence, so a
//! class that regresses, or a producer added without protection, shows up here.
//!
//! The sequence, all through the real `run_pipeline` and the real tail:
//!
//! 1. build 0, run to its end with an inline tail, so `hashes.json` exists and the
//!    builds below are warm (slot-cache hits, and carried pages in the markdown-only
//!    edits);
//! 2. build A, whose tail is HELD BACK ([`CapturingSpawner`]: the `NoSpawner` in
//!    `test_host_ports()` drops the tail unpolled, so no other test host can run one);
//! 3. a real content edit to the vault;
//! 4. build B, run to its render, its tail held back too;
//! 5. release A's tail: `advertise_sealed` → `materialize_and_promote` →
//!    `ship_phase`, reading a stage directory that now holds B's bytes;
//! 6. compare every file in A's generation with the hash A's sealed manifest
//!    recorded for it.
//!
//! B's tail stays held until A's has shipped. Releasing it first would supersede A
//! (`Promotion::Superseded`), and a superseded tail never hands its manifest to
//! deploy — the state the bug needs is A promoted and adopted while B's bytes are on
//! the stage.
//!
//! Builds run without a `FolderSession` of their own. A session makes every worker
//! park a task on its cancel token for the life of the session, which would keep the
//! runtime from ever reading quiet, and this harness needs exactly that reading: it
//! is how it knows A's background workers have finished reading the vault before the
//! edit lands. The tail still takes the registered session's stage-write lock, as in
//! production; nothing here runs two writers at once, so the lock has nothing to
//! order.
//!
//! **Not covered**, so nobody assumes otherwise: video (needs FFmpeg, which the build
//! downloads when absent — not hermetic), notebooks (`resolve_asset_directory` reads
//! the real `~/.moss/assets`), the search lane, and config-change overlaps. The two
//! `nb/analysis.*` keys an earlier scratch harness saw diverge on the `site/` docs
//! are unexplained by this harness and out of its scope.

use super::*;
use crate::build::manifest::SealedManifest;
use crate::build::ports::host::HostPorts;
use crate::build::ports::spawner::{CapturingSpawner, Task};
use crate::deploy::one_shot::capture_seal;
use crate::system::folder_session::FolderSession;
use crate::types::services::BuildServices;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// What may ship from the stage at all
// ---------------------------------------------------------------------------

/// The derived files, registered with `emit_held`: the generation ships the bytes
/// the manifest kept, not whatever the stage holds when the tail runs.
///
/// The manifest the tail hands to deploy cannot show that, because `release_held`
/// drops the bytes first, on purpose. What proves each of these protected is that
/// nothing diverges under the edits below, each calibrated to move the files it
/// names. Listing them is what lets
/// [`assert_every_stage_shipped_entry_is_classified`] fail on any OTHER entry that
/// has no immutable source.
const DERIVED: [&str; 4] = [
    // Hover-preview index: one entry per public page, from its title, description
    // and the first 300 characters of its body.
    "_moss/previews.json",
    // The whole site's markdown, concatenated: any edit to any listed page moves it.
    "llms.txt",
    // Full HTML of every dated, listed article: moves when one of those changes.
    "rss.xml",
    // One <loc> per listed page, <lastmod> from its `date`: moves on a page added,
    // removed, renamed, or re-dated — never on a body edit.
    "sitemap.xml",
];

/// Whether an entry with no CAS object id is safe to ship from the stage by design.
///
/// The edit-independent half of the guard: a fingerprint-only producer that the
/// fixture's edit happens not to vary would pass the divergence check unnoticed, so
/// every such entry in A's manifest must be one of [`DERIVED`] or match here. A new
/// producer trips it the first time it appears.
///
/// Producers the fixture does not run — the search bundle, notebooks, math PNGs, a
/// custom theme — are absent on purpose: adding one to the fixture is exactly when
/// somebody should have to classify it.
fn ships_from_stage_by_design(key: &str) -> bool {
    let file_name = key.rsplit('/').next().unwrap_or(key);
    // Content-named: 16 hex digits between dots (`theme.<h>.js`) or as the whole stem
    // (`og/<h>.png`). The name IS the content, so a rewrite lands at another path.
    let content_named = file_name
        .split('.')
        .any(|part| part.len() == 16 && part.bytes().all(|b| b.is_ascii_hexdigit()));
    (key.starts_with("_moss/") && content_named)
        // Config-derived (site URL, `ai_policy`): moves only on a config edit.
        || key == "robots.txt"
        || (key.starts_with("qr/") && key.ends_with(".svg"))
        // Moves only when the favicon source does.
        || key.starts_with("assets/favicon")
}

// ---------------------------------------------------------------------------
// The vault
// ---------------------------------------------------------------------------

/// 16 x 16 solid PNG. Small on purpose: a `.png`/`.jpg` is converted to a `.webp`
/// however small it is (`raster_with_picture` in `media/image.rs`), and encode time
/// is the one thing here that grows with pixels.
fn write_png(path: &std::path::Path, rgb: [u8; 3]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    ::image::RgbImage::from_pixel(16, 16, ::image::Rgb(rgb)).save(path).unwrap();
}

const HOME: &str = "---\ntitle: Home\n---\n\nWelcome. [notes](notes) [data](assets/data.txt)\n";

/// Long enough that the first 300 characters — all a hover preview keeps — are
/// filled by the opening alone, so an edit at the end leaves `previews.json` alone.
const NOTES_LEAD: &str = "Notes body paragraph. It is long enough that the first three hundred characters of \
plain text are filled by this opening alone, so that an edit made at the very end of the page stays out of the \
hover preview: the quick brown fox jumps over the lazy dog, again and again, until the count of characters is \
comfortably past three hundred, and then it keeps going a little further still.";

fn notes(head: &str, tail: &str) -> String {
    format!("---\ntitle: Notes\n---\n\n{head}{NOTES_LEAD}\n\n{tail}\n")
}

const POST_A: &str = "---\ntitle: First Post\ndate: 2026-01-10\n---\n\n![pic](images/pic.png)\n\nFirst post body.\n";
const POST_B: &str = "---\ntitle: Second Post\ndate: 2026-02-01\n---\n\nSecond post body.\n";

struct Overlap {
    _tmp: tempfile::TempDir,
    folder: std::path::PathBuf,
    mp: crate::moss_paths::MossPaths,
    spawner: Arc<CapturingSpawner>,
    served: Arc<std::sync::RwLock<std::path::PathBuf>>,
    _record: Arc<crate::build::lifecycle::LifecycleCell>,
}

impl Drop for Overlap {
    fn drop(&mut self) {
        crate::system::folder_session::registry().remove(&self.folder.to_string_lossy());
    }
}

/// A build that has rendered, whose tail has not run yet.
struct Held {
    tail: Task,
    sealed: Arc<Mutex<Option<SealedManifest>>>,
}

impl Held {
    /// Run the tail to its end and hand back the manifest it gave deploy. Panics when
    /// the tail was superseded or withheld and gave deploy nothing.
    async fn release(self) -> SealedManifest {
        tokio::time::timeout(std::time::Duration::from_secs(60), self.tail)
            .await
            .expect("the seal tail must finish");
        let sealed = self.sealed.lock().unwrap().take();
        sealed.expect("the tail must have promoted its generation and adopted the manifest")
    }
}

impl Overlap {
    /// A vault in a portable temp dir under `target/`: the system tempdir can itself
    /// sit under a moss-managed folder, which would make this pass or fail for the
    /// wrong reason. Canonical, so this test's `MossPaths` names the same lifecycle
    /// record the build's resolved `VaultRoot` does.
    fn new() -> Self {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-tmp");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::Builder::new().prefix("moss-overlap").tempdir_in(&base).unwrap();
        let folder = tmp.path().canonicalize().unwrap();
        // What the tail takes its lock from: `run_pipeline` reads the registry.
        let session = FolderSession::new(folder.clone());
        crate::system::folder_session::registry().insert(folder.to_string_lossy().to_string(), session);
        let mp = crate::moss_paths::MossPaths::new(&folder);
        // The tail holds this record in production; held here for the whole run so a
        // parallel test's folder lookup cannot evict it between builds.
        let record = crate::build::lifecycle::lock_for(&mp);
        let vault = Overlap {
            _tmp: tmp,
            folder,
            mp,
            spawner: Arc::default(),
            served: Arc::new(std::sync::RwLock::new(std::path::PathBuf::new())),
            _record: record,
        };
        vault.write("index.md", HOME);
        vault.write("notes.md", &notes("", ""));
        vault.write("posts/a.md", POST_A);
        vault.write("posts/b.md", POST_B);
        vault.write("assets/data.txt", "alpha\n");
        write_png(&vault.folder.join("images/pic.png"), [200, 30, 30]);
        vault
    }

    fn write(&self, rel: &str, body: &str) {
        let path = self.folder.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn generations(&self) -> BTreeSet<String> {
        crate::build::store_gc::list_generations(&self.mp.generations_dir()).into_iter().collect()
    }

    fn staged(&self, rel: &str) -> String {
        std::fs::read_to_string(self.mp.staging_dir().join(rel)).unwrap_or_default()
    }

    fn generated(&self, sealed: &SealedManifest, rel: &str) -> String {
        std::fs::read_to_string(self.mp.generation_dir(sealed.generation_id()).join(rel)).unwrap_or_default()
    }

    /// One `run_pipeline`, configured as a watch rebuild is (`SlotsOnly`, an
    /// admission epoch minted in build order) and with a fresh host so each build has
    /// its own manifest slot.
    async fn run(&self, trigger: BuildTrigger, exits_after_build: bool) -> Arc<Mutex<Option<SealedManifest>>> {
        let mut host = HostPorts {
            site_dir: Some(self.served.clone()),
            spawner: self.spawner.clone(),
            services: BuildServices::headless(),
            ..crate::build::ports::host::test_host_ports()
        };
        let sealed = capture_seal(&mut host);
        // Bounded: a build that waited on a tail this test has not released would
        // otherwise hang the suite instead of failing it.
        let build = run_pipeline(PipelineConfig {
            root: VaultRoot::resolve(&self.folder),
            progress: null_sink(),
            plugins: PluginMode::SlotsOnly,
            watch: false,
            start_server: false,
            host,
            trigger,
            exits_after_build,
            site_url_override: Some("https://example.com".to_string()),
            server_port: None,
            admission_epoch: Some(crate::build::ship::next_promotion_epoch()),
            live_port: None,
        });
        tokio::time::timeout(std::time::Duration::from_secs(60), build)
            .await
            .expect("the build must finish without waiting for a tail")
            .expect("build");
        sealed
    }

    /// Build 0: rendered, sealed and shipped inline. Returns its generation id.
    async fn warm_up(&self) -> String {
        let sealed = self.run(BuildTrigger::Full, true).await;
        let sealed = sealed.lock().unwrap().take().expect("build 0 must promote");
        sealed.generation_id().to_string()
    }

    /// A build whose tail is kept back until [`Held::release`].
    async fn build_holding_its_tail(&self, trigger: BuildTrigger) -> Held {
        let sealed = self.run(trigger, false).await;
        Held { tail: self.spawner.take_seal_tail(), sealed }
    }
}

/// Wait until the runtime has no task left: the build's asset walk, its image
/// worker and its manifest coordinator are all `tokio` tasks, so quiet means the
/// build has finished READING THE VAULT. Without this the edit below could land
/// under a worker still to read the file, and that worker would register the edited
/// bytes as its own — consistent, and invisible to the harness.
async fn until_background_work_is_done() {
    let metrics = tokio::runtime::Handle::current().metrics();
    for _ in 0..4000 {
        if metrics.num_alive_tasks() == 0 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("a build's background work never finished: {} tasks alive", metrics.num_alive_tasks());
}

// ---------------------------------------------------------------------------
// The sequence
// ---------------------------------------------------------------------------

/// Every manifest entry whose bytes in `generation_dir` are not the bytes the
/// manifest hashed — the set deploy refuses with "file bytes do not match sealed
/// manifest" (`deploy::upload::verify_bytes`), taken over a whole generation. A file
/// the manifest names but the generation lacks is in the set too: the same broken
/// promise.
///
/// Symlink entries (`120000:`) are skipped: their hash is of the link target's name,
/// which `ship_phase` recreates rather than copies.
fn files_differing_from_manifest(generation_dir: &std::path::Path, sealed: &SealedManifest) -> BTreeSet<String> {
    let mut differing = BTreeSet::new();
    for (rel, entry) in sealed.files() {
        let (mode, expected) = crate::types::content::parse_entry(entry);
        if mode == crate::types::content::MODE_SYMLINK {
            continue;
        }
        let matches = std::fs::read(generation_dir.join(rel))
            .map(|bytes| crate::build::assets::paths::compute_binary_hash(&bytes) == expected)
            .unwrap_or(false);
        if !matches {
            differing.insert(rel.clone());
        }
    }
    differing
}

/// A real content edit, and the trigger a watcher would classify it as.
struct Edit {
    apply: fn(&Overlap),
    trigger: fn() -> BuildTrigger,
    /// A page and a string in it that only B's edit puts there: proof B's render is
    /// on the stage before A's tail runs, so the edit cannot have been a no-op.
    on_stage: Option<(&'static str, &'static str)>,
}

fn md_only(path: &'static str) -> BuildTrigger {
    BuildTrigger::ContentOnly(vec![path.into()])
}

/// Everything the sequence produced.
struct Outcome {
    vault: Overlap,
    a: SealedManifest,
    /// Keys whose bytes in A's generation are not the bytes A's manifest hashed.
    diverging: BTreeSet<String>,
    /// The same for B's generation, shipped last from B's own stage: nothing may be in it.
    b_diverging: BTreeSet<String>,
    /// Keys whose manifest entry differs between A and B, or that only one has: what
    /// B's edit actually changed, whatever shipped.
    drift: BTreeSet<String>,
}

async fn overlap(edit: &Edit) -> Outcome {
    let vault = Overlap::new();
    let gen_0 = vault.warm_up().await;

    // A: its own small edit, so its generation is not build 0's.
    vault.write("notes.md", &notes("", "A-EDIT"));
    let a = vault.build_holding_its_tail(md_only("notes.md")).await;
    until_background_work_is_done().await;

    // The edit under test, then B.
    (edit.apply)(&vault);
    let b = vault.build_holding_its_tail((edit.trigger)()).await;
    until_background_work_is_done().await;

    // Ordering: both builds have rendered and neither tail has run. Nothing reached
    // `generations/` since build 0, `current` has not moved, and the stage already
    // holds B's render. A harness that serialised the builds (or lost B's edit)
    // fails here rather than passing on a sequence that never overlapped.
    assert_eq!(vault.generations(), BTreeSet::from([gen_0.clone()]), "no tail may have shipped yet");
    assert_eq!(vault.mp.current_generation_id().unwrap(), gen_0, "`current` must not have moved");
    if let Some((page, marker)) = edit.on_stage {
        assert!(vault.staged(page).contains(marker), "B's edit is not on the stage ({page} lacks {marker:?})");
    }

    // A ships, from a stage that is now B's.
    let a_sealed = a.release().await;
    assert_eq!(
        vault.generations(),
        BTreeSet::from([gen_0.clone(), a_sealed.generation_id().to_string()]),
        "A's tail must have shipped exactly A's generation, and B's has not run"
    );
    assert_eq!(vault.mp.current_generation_id().unwrap(), a_sealed.generation_id());
    let diverging = files_differing_from_manifest(&vault.mp.generation_dir(a_sealed.generation_id()), &a_sealed);

    // B's own tail, after A's.
    let b_sealed = b.release().await;
    let b_diverging = files_differing_from_manifest(&vault.mp.generation_dir(b_sealed.generation_id()), &b_sealed);
    assert_eq!(vault.mp.current_generation_id().unwrap(), b_sealed.generation_id());

    let drift: BTreeSet<String> = a_sealed
        .files()
        .iter()
        .filter(|(key, entry)| b_sealed.files().get(*key) != Some(*entry))
        .map(|(key, _)| key.clone())
        .chain(b_sealed.files().keys().filter(|key| !a_sealed.files().contains_key(*key)).cloned())
        .collect();
    Outcome { vault, a: a_sealed, diverging, b_diverging, drift }
}

fn set(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

/// The derived files B's edit moved. A file B did not change cannot diverge, so an
/// edit that stopped moving one would stop testing it: each test below pins what its
/// edit moves, from the generators (`feeds/{sitemap,rss,llms_txt}.rs`,
/// `render/blocking.rs`).
fn derived_moved(drift: &BTreeSet<String>) -> BTreeSet<String> {
    drift.iter().filter(|k| DERIVED.contains(&k.as_str())).cloned().collect()
}

/// The ratchet, at its floor: nothing in A's generation may differ from the manifest
/// A sealed. A divergence means a protected class stopped being protected, or a
/// producer was added without protection.
fn assert_nothing_diverges(outcome: &Outcome) {
    let pages: Vec<&String> = outcome.diverging.iter().filter(|k| k.ends_with(".html")).collect();
    assert!(
        outcome.diverging.is_empty(),
        "generation files whose bytes do not match the sealed manifest: {:?}\n\
         (a protected class regressed, or a producer was added unprotected)\n\
         of which rendered HTML pages: {pages:?}",
        outcome.diverging
    );
    // The far end of the chain: B's own tail, after A's, ships B faithfully.
    assert!(
        outcome.b_diverging.is_empty(),
        "B's generation, shipped last from B's own stage, disagrees with B's manifest: {:?}",
        outcome.b_diverging
    );
}

/// Every entry that ships from the mutable stage is either a derived file or named
/// as safe by design — whatever the edit varied.
fn assert_every_stage_shipped_entry_is_classified(sealed: &SealedManifest) {
    let unclassified: Vec<&String> = sealed
        .files()
        .iter()
        .filter(|(_, entry)| crate::types::content::parse_entry(entry).0 == crate::types::content::MODE_FILE)
        .map(|(key, _)| key)
        .filter(|key| sealed.staged_oid(key).is_none())
        .filter(|key| !DERIVED.contains(&key.as_str()) && !ships_from_stage_by_design(key))
        .collect();
    assert!(
        unclassified.is_empty(),
        "these entries have no CAS object id, so they ship from whatever the stage holds when the tail runs, \
         and nothing classifies them. Give the producer an immutable source (a CAS object, or `emit_held` for a \
         derived file, then add it to DERIVED), or add it to ships_from_stage_by_design if it cannot diverge: \
         {unclassified:?}"
    );
}

// ---------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------

/// A rewritten dated article, a new page, a re-encoded image and an overwritten
/// asset, all between A's render and A's ship. Every class of output changes bytes
/// under a held-back tail; none of it may reach A's generation.
const EVERYTHING: Edit = Edit {
    apply: |vault| {
        vault.write(
            "posts/a.md",
            "---\ntitle: First Post, rewritten\ndate: 2026-01-10\n---\n\n![pic](images/pic.png)\n\nB-BODY rewritten.\n",
        );
        vault.write("extra.md", "---\ntitle: Extra\n---\n\nExtra page.\n");
        vault.write("assets/data.txt", "beta, quite different\n");
        write_png(&vault.folder.join("images/pic.png"), [20, 60, 220]);
    },
    // A page was created, so a watcher classifies the batch `Structural`; the asset
    // and the image are not markdown either. This build renders every page.
    trigger: || BuildTrigger::Structural(vec!["extra.md".into(), "assets/data.txt".into(), "images/pic.png".into()]),
    on_stage: Some(("posts/a/index.html", "B-BODY")),
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_overlap_ships_every_generation_file_under_its_own_manifest_hash() {
    let o = overlap(&EVERYTHING).await;

    // The edit changed every class, so a class absent from `diverging` below is
    // protected, not untouched.
    for probe in ["posts/a/index.html", "extra/index.html", "assets/data.txt", "images/pic.png", "images/pic.webp"] {
        assert!(o.drift.contains(probe), "the edit must change {probe}, or its absence below proves nothing: {:?}", o.drift);
    }
    // ...and every derived file, or one of them would quietly stop being tested.
    assert_eq!(derived_moved(&o.drift), set(&DERIVED), "the edit must move every derived file");

    // The ratchet first, so a regression is reported as the set of files it broke.
    assert_nothing_diverges(&o);

    // A's page still says what A rendered, though the stage now says B's.
    let page = "posts/a/index.html";
    assert!(o.vault.generated(&o.a, page).contains("First post body."), "A's generation lost A's text");
    assert!(!o.vault.generated(&o.a, page).contains("B-BODY"), "A's generation received B's text");
    assert!(o.vault.staged(page).contains("B-BODY"), "the stage must hold B's text");

    assert_every_stage_shipped_entry_is_classified(&o.a);
}

/// The control: nothing edited between the two builds, so nothing may diverge. It is
/// what proves the harness is not simply always red, and that no generator writes
/// something (a timestamp) that would be mistaken for a race.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unedited_overlap_diverges_nowhere() {
    let o = overlap(&Edit { apply: |_| {}, trigger: || md_only("notes.md"), on_stage: None }).await;

    assert!(o.drift.is_empty(), "two builds of one vault must produce one set of bytes: {:?}", o.drift);
    assert_nothing_diverges(&o);
    assert_every_stage_shipped_entry_is_classified(&o.a);
}

/// Which edit moves which derived file — the fixture's calibration; see
/// [`derived_moved`].
async fn an_md_edit_moves(edit: Edit, moves: &[&str]) {
    let o = overlap(&edit).await;
    assert_eq!(derived_moved(&o.drift), set(moves), "the edit moves a different set of derived files than expected");
    assert_nothing_diverges(&o);
}

/// Body text of an undated page, past the hover preview's 300 characters: only the
/// concatenation of all markdown changes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_edit_of_an_undated_page_moves_llms_txt_alone() {
    an_md_edit_moves(
        Edit {
            apply: |v| v.write("notes.md", &notes("", "A-EDIT B-TAIL")),
            trigger: || md_only("notes.md"),
            on_stage: Some(("notes/index.html", "B-TAIL")),
        },
        &["llms.txt"],
    )
    .await;
}

/// The same page, edited inside the first 300 characters: the hover preview moves too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_edit_near_the_top_of_an_undated_page_also_moves_previews() {
    an_md_edit_moves(
        Edit {
            apply: |v| v.write("notes.md", &notes("B-HEAD ", "A-EDIT")),
            trigger: || md_only("notes.md"),
            on_stage: Some(("notes/index.html", "B-HEAD")),
        },
        &["_moss/previews.json", "llms.txt"],
    )
    .await;
}

/// A dated, listed article: the feed carries its full HTML, so RSS moves as well.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_edit_of_a_dated_article_moves_the_feed_too() {
    an_md_edit_moves(
        Edit {
            apply: |v| v.write("posts/b.md", "---\ntitle: Second Post\ndate: 2026-02-01\n---\n\nB-BODY of the second post.\n"),
            trigger: || md_only("posts/b.md"),
            on_stage: Some(("posts/b/index.html", "B-BODY")),
        },
        &["_moss/previews.json", "llms.txt", "rss.xml"],
    )
    .await;
}

/// Re-dating an article moves the sitemap's `<lastmod>` and the feed's `<pubDate>`;
/// no body text changed, so neither llms.txt nor the hover previews move.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn re_dating_an_article_moves_the_sitemap_and_the_feed_only() {
    an_md_edit_moves(
        Edit {
            apply: |v| v.write("posts/b.md", "---\ntitle: Second Post\ndate: 2026-03-05\n---\n\nSecond post body.\n"),
            trigger: || md_only("posts/b.md"),
            on_stage: Some(("posts/b/index.html", "2026-03-05")),
        },
        &["rss.xml", "sitemap.xml"],
    )
    .await;
}

/// A page added: the page set moves, so the sitemap does, and a new listed page
/// enters the previews and llms.txt — but nothing is dated, so the feed stays put.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adding_an_undated_page_moves_the_sitemap_but_not_the_feed() {
    an_md_edit_moves(
        Edit {
            apply: |v| v.write("extra.md", "---\ntitle: Extra\n---\n\nB-BODY extra page.\n"),
            trigger: || BuildTrigger::Structural(vec!["extra.md".into()]),
            on_stage: Some(("extra/index.html", "B-BODY")),
        },
        &["_moss/previews.json", "llms.txt", "sitemap.xml"],
    )
    .await;
}
