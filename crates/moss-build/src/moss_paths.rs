//! Canonical path helpers for the `.moss` directory structure.
//!
//! # The .moss Directory
//!
//! Every moss project has a `.moss` directory that stores everything that isn't
//! content. It follows the [Obsidian model]: the app UI is the primary interface
//! for configuration; the filesystem is the power-user escape hatch.
//!
//! ## Directory Layout
//!
//! ```text
//! .moss/
//! ├── config.toml           User settings
//! ├── state.toml            Machine-managed state
//! │
//! ├── theme/                Designer assets (→ copied to site root)
//! │   ├── leaves.mp4        Video overlays, textures, custom fonts, etc.
//! │   ├── grain.png
//! │   └── fonts/
//! │
//! ├── identity/             Cryptographic site identity
//! │   ├── public.json       Public key + email (shareable)
//! │   └── secret-key        Private signing key (never share)
//! │
//! ├── plugins/              Installed plugin bundles
//! │   └── github/
//! │       ├── manifest.json
//! │       └── main.bundle.js
//! │
//! ├── data/                 Runtime + imported data
//! │   ├── events.jsonl      Raw analytics event log (pulled from host)
//! │   ├── events-cursor.json  Sync cursor
//! │   ├── subscribers.csv   Newsletter subscriber list
//! │   └── social/           Imported social data
//! │       ├── matters.json
//! │       └── douban.json
//! │
//! ├── cache/                Content-addressed store — synced with the folder
//! │   ├── objects/          Hashed file blobs (2-char prefix dirs)
//! │   └── transforms/       Source→output mappings
//! │
//! └── build.nosync/         Generated artifacts, per machine (safe to delete)
//!     ├── cache/            Per-machine caches — NOT synced, NOT the store above
//!     │   ├── link-meta/    Cached og:tag metadata for links
//!     │   ├── hash-index.json
//!     │   └── tmp/
//!     ├── generations/      Immutable frozen build outputs (content-addressed)
//!     │   └── <gen-id>/     One frozen directory per sealed generation
//!     ├── current -> generations/<gen-id>/  Symlink to active generation
//!     ├── staging/          In-progress build (atomic swap target)
//!     ├── index/            Search-bundle holding area, OUTSIDE the swept tree
//!     │   ├── receipt.json  What the lane last published (ADR-045)
//!     │   └── <page-set-fp>/  One bundle per indexed page set
//!     ├── hashes.json       File hash manifest for change detection
//!     ├── article-map.json  Content index (all pages with metadata)
//!     └── profile.jsonl     Build performance metrics
//! ```
//!
//! ## Design Principles
//!
//! **Progressive disclosure by depth.** Content creators never open `.moss/`.
//! Designers find `config.toml` and `theme/` at the top level. Developers
//! dig into `build/` for cache and output diagnostics.
//!
//! **Single deletable build target.** `rm -rf .moss/build.nosync/` gives you a clean
//! rebuild without losing config, identity, plugins, or imported data.
//!
//! **Cloud-sync friendly, minus the regenerable parts.** Config, identity,
//! plugins and `data/` sync via iCloud, Dropbox, or Google Drive — that is the
//! portability mechanism across machines. `build/` is asked not to: on one
//! iCloud vault it was 12.3 GB of a 15.6 GB folder, produced ~1700 conflict
//! copies, and holds the bytes "optimize storage" zeroes by shared inode (the
//! 0-byte-stub class `hardlink_invariant_test` guards). The ask is the
//! `com.apple.fileprovider.ignore#P` xattr, set at build start by
//! `exclude_dirs_from_cloud_sync` — and it is only an ask. File Provider
//! extensions (Google Drive, Dropbox) read it as "do not sync". iCloud Drive
//! reads it as "stop syncing", which on a directory it already holds deletes
//! the cloud copy and leaves the local one under a name the cloud can still
//! deliver: it then renames the live `build/` aside as `build N` and puts its
//! own copy at the path, mid-build. So nothing may depend on the marker in
//! either direction. So the tree is split by what "download and wait" can
//! mean. `cache/` — the content-addressed store, `objects/` and
//! `transforms/` — is the same bytes on every machine and every blob carries
//! its checksum, so it syncs with the folder and is waited for (ADR-043's
//! shared-cache amendment; `ObjectStore::ready_blob`). Everything else lives
//! under `build.nosync/` — the suffix iCloud honours unconditionally — is
//! marked for File Provider as well, and is never relied on to be excluded:
//! the build holds that root by a directory handle
//! (`build::lifecycle::root_identity`), logs its identity as the `build.root`
//! line, and reads nothing there beyond what it wrote itself.
//! `build::lifecycle::tree_migration` moves an older tree into this shape
//! once, at build start.
//!
//! **Fully gitignored.** `.moss/` is not tracked by git. Cloud sync is the
//! portability mechanism for settings across machines.
//!
//! ## config.toml vs state.toml
//!
//! `config.toml` holds **user intent** — things deliberately chosen via the UI
//! or hand-edited. Each feature is a self-contained section:
//!
//! ```toml
//! [site]
//! lang = "en"
//!
//! [comments]
//! enabled = true
//!
//! [newsletter]
//! enabled = true
//!
//! [rss]
//! enabled = true
//!
//! [analytics]
//! enabled = true
//! provider = "goatcounter"
//! script = "https://example.goatcounter.com/count"
//! ```
//!
//! `state.toml` holds **machine-managed state** — deployment timestamps,
//! DNS records, commit metadata. Updated automatically by the app. Not
//! intended for hand-editing.
//!
//! ## theme/ Directory
//!
//! Files in `.moss/theme/` are served verbatim under `/_moss/theme/` during
//! build. This is where designers place assets referenced by
//! `style.css` or `script.js` — video overlays, textures, custom fonts.
//!
//! The render phase emits `/_moss/theme/style.css` and `/_moss/theme/script.js`
//! from the two canonical entry points; everything else in `theme/` mirrors
//! through unchanged. There is no longer a reserved-name skip list.
//!
//! ## Plugin Filesystem Access
//!
//! Plugins access `.moss/` exclusively through the moss-api SDK. They never
//! construct paths directly. The Rust backend resolves paths for each SDK
//! function:
//!
//! - `readSiteFile()` → `.moss/build.nosync/current/` (active frozen generation)
//! - `readPluginFile()` → `.moss/plugins/{plugin-name}/`
//!
//! Path traversal (`../`) and absolute paths are blocked at the Rust layer.

use std::path::{Path, PathBuf};

/// Path helper for the `.moss` directory structure.
///
/// Construct from a project root or an existing `.moss` directory path.
/// Methods return canonical paths for each part of the structure.
///
/// # Example
///
/// ```
/// use std::path::Path;
/// use moss_build::moss_paths::MossPaths;
///
/// let root = std::env::current_dir().unwrap().join("my-blog");
/// let paths = MossPaths::new(&root);
/// let moss = root.join(".moss");
///
/// // Build output (frozen generation)
/// assert_eq!(paths.generation_dir("abc123"), moss.join("build.nosync").join("generations").join("abc123"));
///
/// // Theme assets
/// assert_eq!(paths.theme_dir(), moss.join("theme"));
///
/// // Identity
/// assert_eq!(paths.identity_secret(), moss.join("identity").join("secret-key"));
/// ```
pub struct MossPaths {
    root: PathBuf,
}

impl MossPaths {
    /// Create from a project root directory.
    ///
    /// The `.moss` suffix is appended automatically.
    pub fn new(project_root: &Path) -> Self {
        Self {
            root: Self::absolutize(project_root.join(".moss")),
        }
    }

    /// Create from an existing `.moss` directory path.
    ///
    /// Use this when you already have the `.moss` path (e.g., from a config
    /// struct or command argument).
    pub fn from_moss_dir(moss_dir: PathBuf) -> Self {
        Self { root: Self::absolutize(moss_dir) }
    }

    /// Create from a project root path supplied as a `&str`.
    ///
    /// Convenience wrapper for callers (such as Tauri command handlers) that
    /// receive the project path as a string from the frontend.
    pub fn from_project_str(project_path: &str) -> Self {
        Self::new(std::path::Path::new(project_path))
    }

    /// Enforce the invariant that `root` is always absolute — the single
    /// chokepoint every constructor funnels through.
    ///
    /// A relative root makes the `current` generation symlink (and any other
    /// path baked to disk) resolve against the wrong base: a symlink target
    /// resolves relative to the link's own directory, not the CWD, so
    /// `moss build ./site` wrote a dangling `.moss/build.nosync/current` and the
    /// preview server fell back to a stale (or empty) `site/`. GUI callers
    /// already pass absolute project paths (`project_path` is documented and
    /// sent as absolute); this makes the CLI's relative paths equally safe.
    ///
    /// Absolute inputs pass through byte-for-byte unchanged; only a relative
    /// path is resolved against the current directory — lexically, without
    /// touching the filesystem or following symlinks.
    fn absolutize(root: PathBuf) -> PathBuf {
        if root.is_absolute() {
            root
        } else {
            std::path::absolute(&root).unwrap_or(root)
        }
    }

    /// The `.moss` directory itself.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The **site folder** — the directory `.moss` sits in, i.e. what was
    /// passed to [`MossPaths::new`].
    ///
    /// Exists because `root()` reads like "the project root" and is not: it is
    /// `<project>/.moss`. Feeding it to `domain::config::get_*`, which all join
    /// `.moss/config.toml` themselves, silently reads `<project>/.moss/.moss/`
    /// — a path that never exists, so every knob quietly took its default. That
    /// shipped twice (moss#976 Part A's `keep_generations`, and the generation
    /// GC on the interactive path, which ran against an empty directory).
    /// Reach for this whenever the callee expects a *folder path*.
    pub fn project_root(&self) -> &Path {
        // `new()` always appends `.moss`, so a parent always exists.
        self.root.parent().unwrap_or(&self.root)
    }

    // ── Config ──────────────────────────────────────────────

    /// `.moss/config.toml` — user settings (features, lang, services).
    ///
    /// Each feature is a self-contained TOML section with `enabled` + config.
    /// Default provider is "moss" (no config needed for subscribers).
    pub fn config(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    /// `.moss/state.toml` — machine-managed state (deployment, DNS).
    ///
    /// Updated automatically by the app. Not intended for hand-editing.
    pub fn state(&self) -> PathBuf {
        self.root.join("state.toml")
    }

    // ── Theme ───────────────────────────────────────────────

    /// `.moss/theme/` — designer assets copied to site output root.
    ///
    /// Place video overlays, textures, fonts, or other design assets here.
    /// They are copied to the root of the built site during build.
    pub fn theme_dir(&self) -> PathBuf {
        self.root.join("theme")
    }

    // ── Identity ────────────────────────────────────────────

    /// `.moss/identity/` — cryptographic site identity.
    pub fn identity_dir(&self) -> PathBuf {
        self.root.join("identity")
    }

    /// `.moss/identity/public.json` — public key + email (safe to share).
    pub fn identity_public(&self) -> PathBuf {
        self.root.join("identity").join("public.json")
    }

    /// `.moss/identity/secret-key` — private signing key (never share).
    pub fn identity_secret(&self) -> PathBuf {
        self.root.join("identity").join("secret-key")
    }

    // ── Plugins ─────────────────────────────────────────────

    /// `.moss/plugins/` — installed plugin bundles.
    ///
    /// Each plugin gets a subdirectory with its manifest and built JS.
    pub fn plugins_dir(&self) -> PathBuf {
        self.root.join("plugins")
    }

    // ── Data ────────────────────────────────────────────────

    /// `.moss/data/` — runtime + imported data.
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    /// `.moss/data/social/` — imported social data (Matters, Douban, etc.).
    ///
    /// JSON files from external platforms, fed into build.
    pub fn social_dir(&self) -> PathBuf {
        self.root.join("data").join("social")
    }

    /// `.moss/deploy/deployed-article-map.json` — the pre-moss#1079 record of
    /// what went live: a byte copy of the whole article map, 4.83 MB on the
    /// vault that reported the incident to carry the ~7 KB its readers used.
    /// Nothing writes it any more — the publish record carries the note IDs
    /// (`build::manifest::live_baseline`) — and every build imports and removes
    /// whichever copy of it is still here.
    pub fn deployed_article_map(&self) -> PathBuf {
        self.deploy_dir().join("deployed-article-map.json")
    }

    // ── Deploy ──────────────────────────────────────────────

    /// `.moss/deploy/` — durable record of what has actually gone live.
    ///
    /// Not under `build/`: nothing here is regenerable from the project. Delete
    /// it and moss cannot say what changed since the last publish until the
    /// next one lands.
    pub fn deploy_dir(&self) -> PathBuf {
        self.root.join("deploy")
    }

    /// `.moss/deploy/records/` — one publish record per target, holding the
    /// page hashes and output manifest of the last publish that target
    /// confirmed. Absent means the change set is reported unclassified rather
    /// than guessed. `build::manifest::published_record` app-side owns the file
    /// names and says why the key is the target.
    pub fn deploy_records_dir(&self) -> PathBuf {
        self.deploy_dir().join("records")
    }

    // ── Build ───────────────────────────────────────────────
    //
    // Everything under build/ is generated and safe to delete.
    // `rm -rf .moss/build.nosync/` gives a clean rebuild.

    /// `.moss/build.nosync/` — all generated artifacts.
    ///
    /// Safe to delete for a clean rebuild. This is the only large directory;
    /// everything else in `.moss/` is small.
    ///
    /// Answers from a held [`crate::build::lifecycle::open_build_root_handle`]
    /// handle when this folder's build has one open — resolved from the
    /// directory's file descriptor, not a fresh `stat` by this joined path —
    /// so a cloud sync client's rename-aside mid-build cannot silently move
    /// where every accessor built on this method reads or writes. Falls back
    /// to the joined path with no handle open (no build has started one yet,
    /// or the platform cannot).
    pub fn build_dir(&self) -> PathBuf {
        if let Some(held) = crate::build::lifecycle::held_build_root_path(&self.root) {
            return held;
        }
        self.root.join("build.nosync")
    }

    /// `.moss/build.nosync/cache/` — the PER-MACHINE caches: `hash-index.json`,
    /// `dep-cache.json`, `frontmatter-scan.json`, `folder-lang.json`,
    /// `link-meta/`, `manifest-hash-memo.json`, `tmp/`.
    ///
    /// NOT the content-addressed object store — that moved to [`store_dir`]
    /// (`.moss/cache/`, synced with the folder) when the tree split. Reading
    /// this doc as "the object store lives here" is exactly the mistake that
    /// left a warm scan reading an empty store after that split: use
    /// [`cache_objects`](Self::cache_objects) / [`cache_transforms`](Self::cache_transforms),
    /// or [`crate::build::cache::ObjectStore::for_site`] /
    /// [`crate::build::cache::TransformCache::for_site`], never this method,
    /// to reach the store.
    ///
    /// [`store_dir`]: Self::store_dir
    pub fn cache_dir(&self) -> PathBuf {
        self.build_dir().join("cache")
    }

    /// `.moss/cache/objects/` — hashed file blobs.
    /// The content-addressed store, `.moss/cache/`: shared with the folder
    /// and waited for, unlike everything under [`build_dir`](Self::build_dir).
    /// A plain join, not the held handle — the store is never marked, so a
    /// sync client has no reason to rename it aside.
    pub fn store_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn cache_objects(&self) -> PathBuf {
        self.store_dir().join("objects")
    }

    /// `.moss/cache/transforms/` — source→output mappings.
    pub fn cache_transforms(&self) -> PathBuf {
        self.store_dir().join("transforms")
    }

    /// `.moss/build.nosync/cache/tmp/` — temporary files during conversion.
    pub fn cache_tmp(&self) -> PathBuf {
        self.cache_dir().join("tmp")
    }

    /// `.moss/build.nosync/cache/hash-index.json` — fast-path cache validation index.
    pub fn cache_hash_index(&self) -> PathBuf {
        self.cache_dir().join("hash-index.json")
    }

    /// `.moss/build.nosync/cache/dep-cache.json` — per-page facade fingerprints from
    /// the previous build (moss#922 Stage 4), keyed by source path.
    pub fn cache_dep_graph(&self) -> PathBuf {
        self.cache_dir().join("dep-cache.json")
    }

    /// `.moss/build.nosync/cache/frontmatter-scan.json` — per-file `url:`/`external_url:`
    /// frontmatter extraction, keyed by source path, so an unchanged markdown
    /// file's page-map pre-scan is not re-read and re-parsed on every build.
    pub fn cache_frontmatter_scan(&self) -> PathBuf {
        self.cache_dir().join("frontmatter-scan.json")
    }

    /// `.moss/build.nosync/cache/folder-lang.json` — per-folder inferred language
    /// (ADR-065), keyed by folder path, alongside the file-set fingerprint it
    /// was inferred from. A folder whose file set is unchanged since the
    /// last build reuses its stored language rather than re-inferring from
    /// content — an edit to one file's body must never move the folder's
    /// language, only adding/removing a file may.
    pub fn cache_folder_lang(&self) -> PathBuf {
        self.cache_dir().join("folder-lang.json")
    }

    /// `.moss/build.nosync/cache/link-meta/` — cached og:tag metadata for external links.
    pub fn cache_link_meta(&self) -> PathBuf {
        self.cache_dir().join("link-meta")
    }

    /// `.moss/build.nosync/cache/videos/` — legacy video cache (pre-CAS migration).
    pub fn cache_videos_legacy(&self) -> PathBuf {
        self.cache_dir().join("videos")
    }

    /// `.moss/build.nosync/cache/manifest-hash-memo.json` — `oid -> xxh3` memo for
    /// the manifest hash of a CAS blob's bytes. A blob's bytes never change
    /// for a given oid, so this needs no staleness rule; it only grows (or
    /// gets crudely reset once it's large — see `ManifestHashMemo`). Lives
    /// under `cache/`, which is already `ExcludedRegenerable` and outside
    /// every staging/deploy sweep.
    pub fn cache_manifest_hash_memo(&self) -> PathBuf {
        self.cache_dir().join("manifest-hash-memo.json")
    }

    /// `.moss/build.nosync/index/` — holding area for the search bundle (ADR-045).
    ///
    /// Deliberately NOT under `staging/` or a generation: the search lane runs
    /// on its own schedule, so its output must survive `remove_stale_files`,
    /// which deletes every staging file the sealed manifest does not list. The
    /// bundle reaches staging (and the manifest) by *adoption* — every build
    /// copies the receipt's files in and registers them — never by the lane
    /// writing into the swept tree behind the manifest's back.
    pub fn index_dir(&self) -> PathBuf {
        self.build_dir().join("index")
    }

    /// `.moss/build.nosync/index/receipt.json` — the page-set fingerprint the lane
    /// last indexed plus the `(path, hash)` pairs of every file it produced.
    /// Written last, so a torn publish reads as "no receipt" rather than as a
    /// bundle whose files are half on disk.
    pub fn index_receipt(&self) -> PathBuf {
        self.index_dir().join("receipt.json")
    }

    /// `.moss/build.nosync/staging/` — in-progress build with preview annotations.
    ///
    /// Contains `data-source-line` attributes for scroll sync. The preview
    /// server points here during builds. After build completes, staging is
    /// strip-copied to `site/` for deployment.
    pub fn staging_dir(&self) -> PathBuf {
        self.build_dir().join("staging")
    }

    /// `.moss/build.nosync/current.generation` — text marker naming the current generation id.
    pub fn current_generation_marker(&self) -> PathBuf {
        self.build_dir().join("current.generation")
    }

    /// `.moss/build.nosync/generations/` — content-addressed generation store.
    ///
    /// Each completed build is sealed as an immutable generation directory here.
    /// The `current` symlink points to the active generation.
    pub fn generations_dir(&self) -> PathBuf {
        self.build_dir().join("generations")
    }

    /// `.moss/build.nosync/generations/<gen_id>/` — a single sealed generation.
    pub fn generation_dir(&self, gen_id: &str) -> PathBuf {
        self.generations_dir().join(gen_id)
    }

    /// `.moss/build.nosync/current` — symlink to the active generation directory.
    ///
    /// Always points to an absolute path inside `generations/`. Replaced
    /// atomically via `set_current_ptr`.
    pub fn current_ptr(&self) -> PathBuf {
        self.build_dir().join("current")
    }

    /// Atomically replace `.moss/build.nosync/current` → `generations/<gen_id>/`.
    ///
    /// Uses write-then-rename so no reader ever sees an absent `current` pointer.
    /// Symlink target is absolute to avoid cwd ambiguity.
    ///
    /// **Unordered.** Production seal tails must go through
    /// `build::lifecycle::promote`, which refuses a promotion from a build older
    /// than the one already on `current` (moss#968 §5d).
    pub fn set_current_ptr(&self, gen_id: &str) -> std::io::Result<()> {
        let gen_dir = self.generation_dir(gen_id);
        let current = self.current_ptr();

        #[cfg(unix)]
        {
            // Atomic repoint: write-then-rename so no reader ever sees an absent
            // `current` pointer. Symlink target is absolute to avoid cwd ambiguity.
            let tmp = current.with_extension("tmp");
            // allow:unlink the pointer's own temp; the rename below swaps `current` atomically
            let _ = std::fs::remove_file(&tmp);
            std::os::unix::fs::symlink(&gen_dir, &tmp)?;
            // allow:unlink the pointer's own temp; the rename below swaps `current` atomically
            std::fs::rename(&tmp, &current)?;
        }
        #[cfg(windows)]
        {
            // Windows directory symlinks require elevation
            // (SeCreateSymbolicLinkPrivilege), so — like `ship_phase` — copy the
            // generation into `current` for output parity instead. Not atomic, but
            // Windows is not the primary deploy target and a partial read self-heals
            // on the next rebuild.
            // The removal's result is NOT discarded, and that is the whole
            // difference between this and a torn read. `current` is what the
            // preview server serves on Windows (a real directory there, not a
            // symlink), so if something holds a handle open — a scanner, an
            // indexer, a preview pane — and the removal fails, copying on top
            // truncates files a browser is fetching AND leaves the previous
            // generation's files behind, making `current` a union of two
            // builds. Failing the seal is recoverable; a silently blended site
            // is not.
            // allow:unlink Windows only: `current` is a copied directory there and is the served tree, so a promote can show a reader a torn tree (a named residual; the fix is a junction swap)
            match crate::build::io_utils::remove_output_dir_all(&current) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            // Receipts discarded, nothing here reads them (same as `build/ship.rs`); unlike the
            // local copy this replaced, this helper follows directory symlinks, which nothing
            // under `generations/` creates today.
            crate::build::media::pipeline::copy_dir_recursive(&gen_dir, &current)
                .map_err(std::io::Error::other)?;
        }
        // The marker is the answer to "which generation is current" on every
        // platform; on unix `current` remains the served pointer. Under
        // `.moss/build.nosync/`, so through io_utils, not a raw write (ADR-043).
        crate::build::io_utils::write_output(&self.current_generation_marker(), gen_id.as_bytes())?;
        Ok(())
    }

    /// What the preview server should serve at cold start, before the rebuild
    /// has produced annotated `staging/` output.
    ///
    /// Prefers the last frozen generation (the `current` symlink) so a
    /// previously-built site renders instantly with zero flicker; falls back
    /// to `staging/` (e.g. an interrupted prior session, or — on a first-ever
    /// build — an empty directory that is still a valid `ServeDir` root).
    ///
    /// Returns the `current` SYMLINK path itself (not its resolved target) —
    /// `ServeDir` follows symlinks, so this resolves to the generation's files
    /// while staying valid across a later `set_current_ptr` swap.
    ///
    /// The third fallback, `build/site/`, is gone: nothing populates it under
    /// the generations model, so resting on it 404'd the whole rebuild window.
    /// [`retire_legacy_roots`] deletes it on every build.
    pub fn initial_serve_dir(&self) -> PathBuf {
        let current = self.current_ptr();
        // `exists()` follows the symlink: true only if both the link AND its
        // target generation are present, so a dangling pointer falls through.
        if current.exists() {
            return current;
        }
        self.staging_dir()
    }

    /// `.moss/build.nosync/hashes.json` — file hash manifest for change detection.
    ///
    /// Tracks content hashes of output files, source files, plugin fingerprints,
    /// and builder fingerprints. Enables incremental builds and smart preview
    /// refresh.
    pub fn hashes(&self) -> PathBuf {
        self.build_dir().join("hashes.json")
    }

    /// `.moss/build.nosync/article-map.json` — content index.
    ///
    /// Maps URL paths to article metadata (title, content, frontmatter, tags).
    /// Used by syndication plugins, RSS generation, and search.
    pub fn article_map(&self) -> PathBuf {
        self.build_dir().join("article-map.json")
    }

    /// `.moss/build.nosync/profile.jsonl` — build performance metrics.
    ///
    /// One JSON line per build with step-level timing data.
    pub fn profile(&self) -> PathBuf {
        self.build_dir().join("profile.jsonl")
    }

    /// Read the id of the current promoted generation from the marker file.
    pub fn current_generation_id(&self) -> std::io::Result<String> {
        std::fs::read_to_string(self.current_generation_marker()).map(|s| s.trim().to_string())
    }

    // ── Initialization ──────────────────────────────────────

    /// Create all directories in the `.moss` structure. **Test fixture only.**
    ///
    /// Safe to call multiple times — uses `create_dir_all` internally.
    ///
    /// Deliberately `cfg(test)`: production never called it. The build creates
    /// what it needs where it needs it, and the owner of site-output writes is
    /// `build::io_utils` (atomic temp+rename). A per-build path-claim registry
    /// (`StageWriter`, M6b) was proposed alongside it in ADR-052 but was never
    /// built; the correctness gap it was later invoked for (a concurrent
    /// build racing the shared staging tree between seal and ship) shipped
    /// instead as content-addressed manifest entries — see ADR-052's
    /// 2026-09-17 update for what a registry would and would not still cover.
    /// Wiring this up instead would install a second, weaker writer for a
    /// concern that already has a designated owner — and it failed the
    /// three-question gate outright at zero production callers.
    ///
    /// Ten tests across four modules use it to stand up a `.moss` skeleton
    /// before creating generations, so it earns its keep as a fixture; it just
    /// should not pretend to be production API.
    ///
    /// Seven of those ten now live in the app crate, on the far side of the
    /// ADR-057 move, and a plain `#[cfg(test)]` is invisible to them — it arms
    /// only while *this* crate's own tests compile. The `test-fixtures` feature
    /// is what carries the gate across the crate line: `src-tauri` enables it
    /// from `[dev-dependencies]` only, so a production build still cannot see
    /// this method and the argument above is unchanged.
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        let dirs = [
            self.root.clone(),
            self.identity_dir(),
            self.theme_dir(),
            self.plugins_dir(),
            self.data_dir(),
            self.social_dir(),
            self.build_dir(),
            self.cache_dir(),
            self.cache_objects(),
            self.cache_transforms(),
            self.cache_tmp(),
            self.staging_dir(),
            self.generations_dir(),
        ];
        for dir in &dirs {
            // allow:raw_write a test fixture; no production build compiles it
            std::fs::create_dir_all(dir)?;
        }
        // No cloud-sync marking here: this is a fixture, and marking temp dirs
        // proves nothing. Production applies it via `exclude_dirs_from_cloud_sync`
        // at the real `.moss` creation, covered end-to-end in
        // `pipeline_correctness::build_excludes_regenerable_dirs_from_cloud_sync`.
        // Remove any stale current.tmp left by a killed process.
        // allow:unlink a test fixture; no production build compiles it
        let _ = std::fs::remove_file(self.current_ptr().with_extension("tmp"));
        Ok(())
    }
}

// ─── The moss-written path registry (issue #960) ─────────────────────────────
//
// One enumeration of every path moss itself writes, plus the handful inside
// `.moss/` the user edits. Everything that needs to answer "is this moss's own
// output?" reads it: `.moss/.gitignore`, cloud-sync exclusion, and — the reason
// it exists — the file watcher.
//
// It exists because that question had five independent answers. Five times a
// build's own output reached moss's own watcher, and five times the fix was an
// exclusion at one more filter site (#960 tabulates them). The fifth could not
// be fixed that way at all: the triggering event carries no path on Linux.
//
// Adding a path moss writes? Add it HERE, with its three attributes, and the
// watcher excludes it for free.

/// One path moss knows about, relative to the **project root** (not `.moss/`).
///
/// Root-relative because `AGENTS.md`/`CLAUDE.md`/`GEMINI.md` are moss-written
/// and live at the root — a `.moss`-relative table cannot express them, which
/// is how instance 3 of this bug class (#955) got in.
///
/// Four independent flags, not one `is_moss_written` boolean. The concerns are
/// genuinely different sets and collapsing them would be a new bug:
/// `data/social/` is moss-written but **must** stay watched, while `identity/`
/// and `keys/` matter for gitignore and are irrelevant to cloud sync.
///
/// `watched` and `materialized` are the pair most easily confused, and moss#986
/// is what happens when they are treated as one. "Should an edit here rebuild
/// the site?" and "must moss be able to READ these bytes?" are different
/// questions with different answers: nothing rebuilds when the identity key
/// changes, and publish cannot start without it.
/// What a cloud provider may do with a moss-written path.
///
/// An enum rather than a bool, and the reason is moss#1079: a path whose
/// exclusion is a real decision must state it, and a bool is a decision you can
/// make by not thinking. Every entry names its regime or the crate does not
/// compile — and the one entry that keeps a provider's hands on something moss
/// cannot rebuild has to say why that is right (ADR-062).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudPolicy {
    /// The provider is welcome to it. Either the user's own content, or state a
    /// second machine genuinely wants: the identity key is what lets another
    /// machine publish the same site at all, and `.moss/deploy/` is the record
    /// of which URLs are live — a fact about the SITE, so a second machine that
    /// publishes after a rename can still leave a forwarding link.
    Synced,
    /// Regenerable output — ADR-043's regime. Nothing here is authoritative:
    /// moss holds the replacement bytes at every build start, so a dataless
    /// file here **is** absent and writes go through `build::io_utils`.
    ExcludedRegenerable,
    /// Regenerable AND worth waiting for: the content-addressed store. Same
    /// key ⇒ same bytes on every machine, and every blob carries its own
    /// checksum, so a dataless one is a download to wait for — bounded, then
    /// a miss — rather than an absence (ADR-043's shared-cache amendment).
    /// Never marked: on iCloud Drive the marker un-syncs a directory the
    /// cloud already holds, which for a shared store is a delete.
    SyncedRegenerable,
}

impl CloudPolicy {
    /// Whether this path gets the "do not sync" marker.
    pub fn is_excluded(self) -> bool {
        matches!(self, CloudPolicy::ExcludedRegenerable)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MossPathRule {
    /// Root-relative, forward slashes, trailing `/` iff it is a directory.
    pub rel: &'static str,
    /// Must never reach a commit — contributes a line to `.moss/.gitignore`.
    pub gitignore: bool,
    /// What a cloud provider may do with this path.
    ///
    /// Not a bool, and the reason is moss#1079: the entry that held the only
    /// record of which URLs are live carried a `false` nobody had decided —
    /// `false` is what a new entry gets by not thinking about it — under a
    /// comment claiming the opposite. An enum has no default, and the test
    /// below makes an authoritative synced path write down why sharing it is
    /// right.
    pub cloud: CloudPolicy,
    /// May reach the file watcher. False for everything moss writes.
    pub watched: bool,
    /// moss READS these bytes and cannot regenerate them, so the cloud
    /// supervisor asks the provider for them when they are evicted
    /// (`build_shell::watch::sweep`).
    ///
    /// Strictly wider than `watched`: `.moss/identity/` is the file publish
    /// cannot proceed without and the file no edit ever rebuilds on — which is
    /// how the one file a publish dies on became the one nobody asked the
    /// provider for (moss#986). Not the same as "important": `.moss/build.nosync/` is
    /// read back but regenerable, and ADR-043 says a dataless file there is
    /// *absent*, so it stays false and stays pruned.
    pub materialized: bool,
    /// The user may legitimately want to commit ONE file from here, so the
    /// emitted line is `dir/*` — the only spelling git can re-include through —
    /// and a `!dir/one-file` beneath it is honored.
    ///
    /// False for secrets on purpose: nothing in `keys/` should ever be
    /// committed, so its line stays the bare `keys/` git cannot negate through.
    pub gitignore_negatable: bool,
}

/// Ordered so that the `gitignore` entries generate [`moss_gitignore`] in the
/// exact order the hand-written constant used to have. Lookup is
/// longest-prefix, so declaration order does not affect matching.
pub const MOSS_PATH_RULES: &[MossPathRule] = &[
    // — gitignored, in .moss/.gitignore order —
    // Not watched (no edit here rebuilds anything) but materialized: these are
    // the site's cryptographic identity, and publish signs with them.
    r(".moss/identity/", true, CloudPolicy::Synced, false, true),
    r(".moss/keys/", true, CloudPolicy::Synced, false, true),
    // `data/` holds non-regenerable user state moss reads and WRITES BACK —
    // reading an evicted one as empty turns a merge into an erase
    // (`feeds::redirects`).
    //
    // Emitted as `data/*`: `redirects.json` records the old URLs a site still
    // answers, which is site content, and a site that keeps it in git wants
    // every checkout to serve the same redirects. The one directory here with a
    // file worth committing, so the one rule spelled so the user can.
    r_negatable(".moss/data/", true, CloudPolicy::Synced, false, true),
    // What the last publish put live: one record per target, holding the page
    // hashes the change set is computed from and the note ID that was live at
    // each URL.
    //
    // NOT gitignored, since v6 (moss#993). The record is a fact about the
    // SITE, not this computer — the same reasoning that keeps it in cloud
    // sync (reconsidered from moss#1079) keeps it in git: a fresh clone
    // without it thinks nothing was ever published — blank composition ring,
    // no forwarding link after a rename. It was ignored to spare a
    // collaborator a stale "up to date"; the CPHS re-clone (2026-08-31)
    // showed the blank costs more, and every reader already treats the record
    // as advisory (ADR-062: unreadable is never "nothing published"). The v6
    // gitignore migration prunes the old `deploy/` line from existing projects.
    r(".moss/deploy/", false, CloudPolicy::Synced, false, true),
    // Declared under `.moss/build.nosync/` anyway: a row is what creates and marks
    // the directory the moment moss first writes there.
    r(".moss/build.nosync/cache/", false, CloudPolicy::ExcludedRegenerable, false, false),
    r(".moss/build.nosync/staging/", false, CloudPolicy::ExcludedRegenerable, false, false),
    // Born 2026-07-22 with no row: on the CPHS vault its 6 GB carried
    // `com.dropbox.attrs` while the marked `staging/` and `cache/` beside it
    // stayed clean. `gitignore: false` — a line would rewrite every existing
    // project's `.moss/.gitignore` as a side effect (#960).
    r(".moss/build.nosync/generations/", false, CloudPolicy::ExcludedRegenerable, false, false),
    r(".moss/agents/", true, CloudPolicy::Synced, false, false),
    // Gitignored: a git user already has git's own history, and moss must never
    // turn a version store into commits nobody made. `CloudPolicy::Synced`
    // because traveling with the vault via whatever cloud provider it already
    // uses is the whole point. Not watched: a write here must never trigger a
    // rebuild. Not materialized: an evicted blob already reads as "not kept" —
    // the store's existing honest-about-missing-bytes posture — so nothing here
    // needs to be force-downloaded.
    r(".moss/history/", true, CloudPolicy::Synced, false, false),
    // — regenerable output, kept out of cloud sync —
    r(".moss/build.nosync/", true, CloudPolicy::ExcludedRegenerable, false, false),
    // The store is the one path that is both regenerable and shared: see the
    // variant's doc. Gitignored like the rest of the output — blobs in a
    // site's repository are never wanted.
    r(".moss/cache/", true, CloudPolicy::SyncedRegenerable, false, false),
    // — moss-written, neither gitignored nor cloud-excluded —
    // Plugin bundles are moss-written but not regenerable without a re-install,
    // and a plugin whose bundle is evicted simply fails to load.
    r(".moss/plugins/", false, CloudPolicy::Synced, false, true),
    // The record of where this site is published. Read as empty, a deployed
    // site looks undeployed and the build bakes localhost into every page.
    r(".moss/state.toml", false, CloudPolicy::Synced, false, true),
    // Root agent-instruction files — tooling, not content, whoever wrote them.
    // Kept in step with the app crate's `build::scan::classify::ROOT_AGENT_CONFIG_FILES`
    // by `moss::infra::moss_paths::tests::root_agent_files_match_the_classifier`,
    // which stays on that side of the crate line because the list it compares
    // against is still there (ADR-057).
    r("AGENTS.md", false, CloudPolicy::Synced, false, false),
    r("CLAUDE.md", false, CloudPolicy::Synced, false, false),
    r("GEMINI.md", false, CloudPolicy::Synced, false, false),
    // — inside `.moss/`, but the user's to edit: these MUST stay watched —
    r(".moss/config.toml", false, CloudPolicy::Synced, true, true),
    r(".moss/theme/", false, CloudPolicy::Synced, true, true),
    r(".moss/assets/", false, CloudPolicy::Synced, true, true),
    // moss writes this one (background comment/social sync) and it is still
    // watched on purpose: the sync is change-gated, and the preview has to
    // refresh when it lands. Pinned by the social-sync rebuild regression test.
    r(".moss/data/social/", false, CloudPolicy::Synced, true, true),
];

const fn r(
    rel: &'static str,
    gitignore: bool,
    cloud: CloudPolicy,
    watched: bool,
    materialized: bool,
) -> MossPathRule {
    MossPathRule { rel, gitignore, cloud, watched, materialized, gitignore_negatable: false }
}

/// Same, for the one directory whose gitignore line must permit re-including a
/// single file. A separate constructor so `r` stays the default — the safe
/// spelling is what you get by not thinking about it.
const fn r_negatable(
    rel: &'static str,
    gitignore: bool,
    cloud: CloudPolicy,
    watched: bool,
    materialized: bool,
) -> MossPathRule {
    MossPathRule { rel, gitignore, cloud, watched, materialized, gitignore_negatable: true }
}

/// The rule governing a project-root-relative path, longest prefix wins.
///
/// Longest-prefix is what lets `.moss/data/` (not watched) and
/// `.moss/data/social/` (watched) coexist in one table.
pub fn rule_for(rel: &str) -> Option<&'static MossPathRule> {
    let rel = rel.trim_start_matches("./").trim_end_matches('/');
    let mut best: Option<&'static MossPathRule> = None;
    for rule in MOSS_PATH_RULES {
        let matches = match rule.rel.strip_suffix('/') {
            // Directory: itself or anything beneath it.
            Some(dir) => rel == dir || rel.starts_with(&format!("{dir}/")),
            // File: exact match only.
            None => rel == rule.rel,
        };
        // `best.is_none_or(…)` says this more directly but is stable only from
        // 1.82, and this crate declares the workspace MSRV (1.80) where the app
        // crate declares none — so the same line that was silent in `src-tauri`
        // is a clippy::incompatible_msrv warning here. "No best yet" is length 0.
        if matches && rule.rel.len() > best.map_or(0, |b| b.rel.len()) {
            best = Some(rule);
        }
    }
    best
}

/// Whether a project-root-relative path may reach the file watcher.
///
/// Unknown paths under `.moss/` are **not** watchable — `.moss/` is moss's
/// namespace, so a new subdirectory a future version writes is excluded before
/// anyone remembers to add it. Unknown paths elsewhere are the user's content
/// and are watchable (the dotfile/`node_modules` rules are applied separately
/// by the watcher).
pub fn is_watchable_rel(rel: &str) -> bool {
    match rule_for(rel) {
        Some(rule) => rule.watched,
        None => {
            let rel = rel.trim_start_matches("./");
            rel != ".moss" && !rel.starts_with(".moss/")
        }
    }
}

/// Whether moss must be able to READ this path's bytes — i.e. whether the cloud
/// supervisor should ask the provider to hand an evicted copy back.
///
/// Outside `.moss/` everything is the user's content and the answer is yes (the
/// extension filter is the caller's). Inside `.moss/` the table decides, and an
/// undeclared subdirectory is **not** materialized: a future version's scratch
/// directory must not silently spend the provider's request budget.
pub fn is_materialized_rel(rel: &str) -> bool {
    match rule_for(rel) {
        Some(rule) => rule.materialized,
        None => {
            let rel = rel.trim_start_matches("./");
            rel != ".moss" && !rel.starts_with(".moss/")
        }
    }
}

/// Delete the output roots older moss versions wrote, and unblock `current`.
/// `.moss/site/` and `build/site/` (now under `build.nosync/`, where the
/// tree migration moves the old root) lost their last reader when generations
/// replaced in-place output. `.moss/cache/` was retired here too, until it
/// became the shared store's home. A directory-shaped `build.nosync/current` is the
/// pre-generations form of the pointer, and on unix `set_current_ptr` renames
/// a symlink into place, so while that directory sits there the build seals
/// generations it can never promote — an old site served forever with no error
/// a user could act on. Idempotent — three `remove_dir_all` calls returning
/// `NotFound` once migrated — and unix-gated only around `current`, which on
/// Windows is a real directory by design.
///
/// Takes `&MossPaths`, not a bare path: `build/site` and `build/current` sit
/// under the live build root, so they route through `build_dir()` like every
/// other build-owned path — a hand-joined `moss_root.join("build/…")` here
/// would keep targeting the decoy after a cloud sync client's rename-aside,
/// same as the accessors above before this fix.
pub fn retire_legacy_roots(mp: &MossPaths) {
    let moss_root = mp.root();
    for legacy in [moss_root.join("site"), mp.build_dir().join("site")] {
        // allow:unlink retired output roots that no build writes or serves
        let _ = crate::build::io_utils::remove_output_dir_all(&legacy);
    }
    #[cfg(unix)]
    {
        let current = mp.current_ptr();
        // `symlink_metadata`, not `is_dir`: the live form is a symlink TO a
        // directory, and following it would delete the generation.
        if matches!(std::fs::symlink_metadata(&current), Ok(m) if m.is_dir()) {
            // Logged, unlike the three above: a failure here reproduces the
            // exact undiagnosable state the pass exists to end.
            // allow:unlink retired output roots that no build writes or serves
            if let Err(e) = crate::build::io_utils::remove_output_dir_all(&current) {
                log::warn!("[retire-legacy] {} is a directory and could not be removed ({e}); \
                            promotion will keep failing until it is deleted by hand", current.display());
            }
        }
    }
}

/// Create every directory moss keeps out of cloud sync, and mark it.
///
/// Named for the question rather than for the answer: what a path's
/// [`CloudPolicy`] is, is the table's business, and moss#1079 was a case of
/// reading the sweep's old name (`exclude_regenerable_dirs`) as if it were
/// evidence about a path's regime.
///
/// Called from the build entry point rather than from [`MossPaths::ensure_dirs`]:
/// every caller of `ensure_dirs` sits inside `#[cfg(test)]`, so hooking there
/// would mark the directories in tests and never in a real build. Creating them
/// here is also what gives the marker something to attach to — it must land
/// before either directory fills, since it cannot un-sync bytes already
/// uploaded.
pub fn exclude_dirs_from_cloud_sync(moss_root: &std::path::Path) {
    for rule in MOSS_PATH_RULES.iter().filter(|r| r.cloud.is_excluded()) {
        let Some(sub) = rule.rel.strip_prefix(".moss/") else { continue };
        let dir = moss_root.join(sub.trim_end_matches('/'));
        // The create is what gives the marker something to attach to, so a
        // failure here means the directory goes UNMARKED and syncs — one of the
        // three candidate causes #965 is trying to tell apart. It was silent
        // before; a cloud provider refusing to materialize the entry
        // (`EDEADLK`) looks identical to a permissions error without this line.
        match crate::build::io_utils::create_output_dir_all(&dir) {
            Ok(()) => exclude_from_cloud_sync(&dir),
            // Regenerable, so the cost of syncing it is noise and conflict
            // copies rather than anything lost — hence `debug`. A path whose
            // exclusion protected something moss could not rebuild would earn
            // a louder line, and would first have to answer the test below.
            Err(e) => log::debug!(
                "[cloud-exclude] could not create {} — it will sync unmarked: {}",
                dir.display(),
                e
            ),
        }
    }
}

/// Everything under `.moss/` that must never reach a commit.
///
/// moss owns this file, not the user's root `.gitignore` — a folder of
/// markdown may not even be a repo, and if it is, its `.gitignore` is the
/// author's. `keys/` is load-bearing: a keystore in a pushed repo is a leaked
/// permanent site identity (pinned by `build_tests::keystore_dir_is_gitignored`).
///
/// Derived from [`MOSS_PATH_RULES`]; the exact output is pinned by
/// `moss_paths_tests::gitignore_matches_the_shipped_contents`. Adding a line
/// here appends it to every existing project's `.moss/.gitignore` on its next
/// build, which is why `build/generations/` never was; `build.nosync/` and
/// `cache/` were, deliberately, when the tree split — the old `build/cache/`
/// and `build/staging/` lines they supersede stay where a project has them,
/// matching nothing.
pub fn moss_gitignore() -> String {
    gitignore_rules().map(|(_, line)| format!("{line}\n")).collect()
}

/// Each gitignored rule paired with the line it contributes.
///
/// Public because the writer that consumes it — `ensure_moss_gitignore` — stays
/// in the app crate until the cloud-readiness cluster crosses (ADR-057).
pub fn gitignore_rules() -> impl Iterator<Item = (&'static MossPathRule, String)> {
    MOSS_PATH_RULES.iter().filter(|r| r.gitignore).filter_map(|rule| {
        let rel = rule.rel.strip_prefix(".moss/")?;
        // `data/` → `data/*`, the only form git can re-include through.
        let line = if rule.gitignore_negatable { format!("{rel}*") } else { rel.to_string() };
        Some((rule, line))
    })
}

/// The directory an existing `.gitignore` line excludes in full — `data`,
/// `data/`, `/data/`, `data/*` and `data/**` all name `data`. They differ only
/// in whether git can re-include *through* them, which is a question about the
/// user's OTHER lines, not this one.
///
/// A wildcard surviving the strip comes back `None`: moss implements no glob
/// matcher, and the safe answer to "does `**/tmp*` cover `keys/`?" is "assume
/// not" — that appends a redundant line instead of trusting one it misread.
pub fn excluded_dir(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
        return None;
    }
    let body = line
        .strip_suffix("/**")
        .or_else(|| line.strip_suffix("/*"))
        .unwrap_or(line)
        .trim_end_matches('/');
    let body = body.strip_prefix('/').unwrap_or(body);
    if body.is_empty() || body.contains(['*', '?', '[']) {
        return None;
    }
    Some(body)
}

/// Whether `existing` already excludes everything under `dir`.
///
/// String equality was the bug. A user who rewrote `data/` as `data/*` so they
/// could add `!data/redirects.json` had satisfied moss's rule more precisely;
/// moss failed to recognize its own rule and appended a bare `data/` **after**
/// the negation, and git cannot re-include a file whose parent directory is
/// excluded, so 128 committed redirect stubs went silently untracked.
///
/// `negatable` is what keeps `keys/` intact: for a secret, a `!` beneath the
/// rule is not intent to respect but the path by which a permanent site
/// identity reaches a pushed repo, so it counts as NOT excluded and moss
/// appends the bare `keys/` to override it.
pub fn already_excluded(existing: &str, dir: &str, negatable: bool) -> bool {
    let covered = existing
        .lines()
        .filter_map(excluded_dir)
        // A broader line counts: a user's `build/` excludes `build/cache/`.
        .any(|d| dir == d || dir.starts_with(&format!("{d}/")));
    if !covered {
        return false;
    }
    if negatable {
        return true;
    }
    !existing.lines().any(|l| {
        let Some(pat) = l.trim().strip_prefix('!') else { return false };
        let pat = pat.trim().trim_start_matches('/').trim_end_matches('/');
        pat == dir || pat.starts_with(&format!("{dir}/"))
    })
}

/// Mark `dir` as excluded from cloud file-provider sync (iCloud Drive, and the
/// other providers that adopted File Provider). Best-effort and idempotent.
///
/// Applied to every [`MOSS_PATH_RULES`] row that `exclude_dirs_from_cloud_sync`
/// marks `ExcludedRegenerable`: `.moss/build.nosync/` plus `cache/`, `staging/` and
/// `generations/` beneath it — there is no top-level `.moss/cache` since the
/// CAS migration in 752fe9e0e5. All regenerable, and syncing it is actively
/// harmful: on one iCloud vault it was 12.3 GB of 15.6 GB, ~1700 conflict
/// copies (`rss 2.xml`, `photo 3.jpg`), the same bytes "optimize storage"
/// eviction zeroes by shared inode (`hardlink_invariant_test` guards this).
/// `.moss/data`/`config.toml` deliberately keep syncing: small, and wanted.
///
/// The attribute goes on the DIRECTORY — the provider skips the whole subtree
/// from that one marker, it is not copied onto each child. The `#P` suffix is
/// Apple's flag-encoding (see `<sys/xattr_flags.h>`) and keeps the marker
/// attached if the directory itself is copied.
/// Directories already warned about a failed `setxattr` this process. Set
/// once per session — "session" meaning process lifetime, the same meaning
/// used everywhere else in this design — so a vault that re-triggers the
/// same failure on every rebuild doesn't re-warn on each save.
#[cfg(target_os = "macos")]
fn warned_dirs() -> &'static std::sync::Mutex<std::collections::HashSet<std::path::PathBuf>> {
    static WARNED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<std::path::PathBuf>>> =
        std::sync::OnceLock::new();
    WARNED.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// Pure gate: `true` the first time `dir` is seen, `false` on every later
/// call with the same `dir`. Takes `seen` explicitly rather than reading
/// [`warned_dirs`] directly so a test exercises a local set — parallel-safe,
/// independent of every other test in the binary.
#[cfg(target_os = "macos")]
pub(crate) fn should_warn_once(
    dir: &std::path::Path,
    seen: &std::sync::Mutex<std::collections::HashSet<std::path::PathBuf>>,
) -> bool {
    seen.lock().unwrap_or_else(|e| e.into_inner()).insert(dir.to_path_buf())
}

#[cfg(target_os = "macos")]
fn exclude_from_cloud_sync(dir: &std::path::Path) {
    use std::os::unix::ffi::OsStrExt;
    let (Ok(path), Ok(name)) = (
        std::ffi::CString::new(dir.as_os_str().as_bytes()),
        std::ffi::CString::new("com.apple.fileprovider.ignore#P"),
    ) else {
        return;
    };
    // SAFETY: both pointers are NUL-terminated and outlive the call; the value
    // pointer and length describe the same 1-byte literal.
    let rc = unsafe {
        libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            b"1".as_ptr() as *const libc::c_void,
            1,
            0,
            0,
        )
    };
    // The failure is still non-fatal — failing to set this costs sync hygiene,
    // never a build, and a read-only or non-xattr filesystem is fine. But the
    // return value is no longer *silent*: on one live iCloud vault the marker
    // was measured ABSENT on `.moss/build.nosync` while present on `.moss/cache`, with
    // 24 cross-machine conflict copies of the `current` symlink to show for it
    // (moss#964 §4). Whether the call fails, the marker is stripped on a
    // cross-machine round trip, or it was applied only after the directory had
    // already synced is unresolved, and this log line is what the next
    // investigation starts from.
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        if should_warn_once(dir, warned_dirs()) {
            log::warn!(
                "[cloud-exclude] setxattr(com.apple.fileprovider.ignore#P) failed on {}: {}",
                dir.display(),
                err
            );
        }
        return;
    }
    // Verify after set: a call that returned 0 is not the same as a marker on
    // the directory, and which of the two failed is the question moss#965
    // could not answer from the return code alone.
    if !has_cloud_sync_marker(dir) && should_warn_once(dir, warned_dirs()) {
        log::warn!(
            "[cloud-exclude] com.apple.fileprovider.ignore#P is absent on {} right after it was set — \
             the provider may still sync it",
            dir.display()
        );
    }
}

/// Whether `dir` carries the File Provider exclusion marker.
#[cfg(target_os = "macos")]
pub(crate) fn has_cloud_sync_marker(dir: &std::path::Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let (Ok(path), Ok(name)) = (
        std::ffi::CString::new(dir.as_os_str().as_bytes()),
        std::ffi::CString::new("com.apple.fileprovider.ignore#P"),
    ) else {
        return false;
    };
    // SAFETY: both pointers are NUL-terminated and outlive the call; a null
    // value buffer of size 0 asks only for the attribute's length.
    unsafe { libc::getxattr(path.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0, 0, 0) >= 0 }
}

/// No marker off macOS — File Provider is an Apple subsystem, so nothing
/// here can be excluded from it. Widened alongside the macOS definition
/// (rather than left `#[cfg(target_os = "macos")]`-only) so a caller outside
/// this file can ask the question uniformly.
#[cfg(not(target_os = "macos"))]
pub(crate) fn has_cloud_sync_marker(_dir: &std::path::Path) -> bool {
    false
}

/// No-op off macOS — File Provider is an Apple subsystem.
#[cfg(not(target_os = "macos"))]
fn exclude_from_cloud_sync(_dir: &std::path::Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_paths(root: &std::path::Path) -> MossPaths {
        MossPaths::new(root)
    }

    // ─── The moss-written path registry (#960) ──────────────────────────────

    /// `.moss/.gitignore` is now generated. Its contents are a promise to every
    /// project already on disk — `ensure_moss_gitignore` only ever ADDS lines,
    /// so a line that appears here appears in every user's file on their next
    /// build. Pinned byte-for-byte against what shipped.
    #[test]
    fn gitignore_matches_the_shipped_contents() {
        assert_eq!(
            moss_gitignore(),
            "identity/\nkeys/\ndata/*\nagents/\nhistory/\nbuild.nosync/\ncache/\n",
            "the generated .moss/.gitignore changed — if that is intended, note \
             that it lands in every existing project on its next build"
        );
    }
    // ─── Which cloud regime each path is in (moss#1079, ADR-062) ────────────

    /// The trap moss#1079 fell into, stated as a test.
    ///
    /// Two questions decide it. Is this path **authoritative** — is moss the
    /// only thing that knows what is in it, so nothing can rebuild it? And can
    /// a **provider withhold** it — is it in territory a sync client governs?
    /// Both yes is the trap: `.moss/deploy/` held the only record of which
    /// URLs were live, and a provider decided moss could not read it.
    ///
    /// `materialized` is moss's own name for the first question ("moss READS
    /// these bytes and cannot regenerate them"), and `CloudPolicy::Synced` is
    /// the second. Every entry in that intersection has to be listed here with
    /// the reason it is genuinely fine — and a new one fails this test until
    /// someone writes that reason down.
    ///
    /// Being in the intersection is not by itself wrong: most of these MUST
    /// travel. What is wrong is being in it by accident, and then reading a
    /// withheld file as an empty one.
    ///
    /// Listing an entry is not a rubber stamp: each of these is synced because
    /// a second machine legitimately needs it, and each therefore also owes its
    /// reader the "unreadable is not absent" handling (icloud-awareness.md).
    #[test]
    fn every_authoritative_synced_path_says_why_it_is_safe() {
        // rel => why sharing it is right, despite the provider owning it.
        const DELIBERATELY_SHARED: &[(&str, &str)] = &[
            (".moss/identity/", "the site's signing identity: a second machine cannot publish the same site without it, so it MUST travel"),
            (".moss/keys/", "same — the keystore is the site's identity, and the alternative is a site only one computer can ever publish"),
            (".moss/data/", "site state the site itself owns: redirects.json is a rename history every checkout should serve. Its reader already distinguishes unreadable from absent (feeds::redirects)"),
            (".moss/plugins/", "installed plugin bundles — re-installable, and an evicted bundle simply fails to load"),
            (".moss/state.toml", "where this site is published: wanted on a second machine, which would otherwise bake localhost into pages"),
            (".moss/config.toml", "the user's own configuration — content, not machine state"),
            (".moss/theme/", "the user's own theme — content"),
            (".moss/assets/", "the user's own assets — content"),
            (".moss/data/social/", "background comment/social sync results, re-fetchable from the server that produced them"),
            (".moss/deploy/", "what is live at each target, which is a fact about the SITE: a second machine needs it to leave a forwarding link for a page this one renamed. Read through `manifest::live_baseline`, whose tri-state keeps unreadable apart from absent (moss#1079)"),
        ];

        let unlisted: Vec<&str> = MOSS_PATH_RULES
            .iter()
            .filter(|r| r.materialized && !r.cloud.is_excluded())
            .map(|r| r.rel)
            .filter(|rel| !DELIBERATELY_SHARED.iter().any(|(listed, _)| listed == rel))
            .collect();

        assert!(
            unlisted.is_empty(),
            "these paths are authoritative (moss reads them and cannot rebuild them) AND \
             synced (a provider can decline to hand them back) — the moss#1079 trap. \
             Either exclude it from sync, or add it to DELIBERATELY_SHARED with the \
             reason a second machine needs it — AND give its reader the tri-state that \
             keeps unreadable apart from absent: {unlisted:?}"
        );
    }

    /// The publish record's regime, pinned where it can be read.
    ///
    /// Synced AND tracked, both for the same reason: the record is a fact
    /// about the site, and a machine without it thinks nothing was ever
    /// published — blank ring, no forwarding link after a rename. Git carried
    /// the opposite decision until v6 (a committed record can tell a
    /// collaborator "up to date" when their checkout is not), and the CPHS
    /// re-clone (moss#993) is why it flipped: every reader already treats the
    /// record as advisory, and the blank cost more than the staleness.
    /// Materialized — moss reads it back and cannot rebuild it, so it stays
    /// in the set the sweep asks the provider for.
    #[test]
    fn the_publish_record_travels_by_git_and_by_the_provider() {
        let rule = rule_for(".moss/deploy/records/moss-abc.json").expect("rule");
        assert_eq!(rule.cloud, CloudPolicy::Synced);
        assert!(
            !rule.gitignore,
            "an ignored publish record leaves a fresh clone with no ring and \
             no rename forwarding (moss#993) — flipping this back also needs a \
             gitignore migration to re-add the line to existing projects"
        );
        assert!(
            rule.materialized,
            "unlike regenerable output, this one is read back — it must stay in the \
             set the cloud supervisor asks the provider for (moss#986)"
        );
        assert!(!rule.watched, "a deploy writing it must not wake the watcher");
    }

    /// The directory-shaped `current` is the one that mattered: `set_current_ptr`
    /// renames a symlink into place, which fails while a directory sits there,
    /// so a project built by a pre-generations moss sealed a generation it
    /// could never promote — the old site served forever, no error to act on.
    #[cfg(unix)]
    #[test]
    fn retiring_legacy_roots_clears_what_blocks_a_promotion_and_keeps_what_serves_one() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
        std::fs::create_dir_all(&base).unwrap();
        let proj = tempfile::TempDir::new_in(&base).unwrap();
        let mp = MossPaths::new(proj.path());
        let root = mp.root();

        for legacy in ["site", "build.nosync/site"] {
            std::fs::create_dir_all(root.join(legacy)).unwrap();
            std::fs::write(root.join(legacy).join("stale"), b"old").unwrap();
        }
        std::fs::create_dir_all(root.join("build.nosync/current")).unwrap();
        std::fs::write(root.join("build.nosync/current/index.html"), b"<h1>five weeks old</h1>").unwrap();

        retire_legacy_roots(&mp);

        for legacy in ["site", "build.nosync/site", "build.nosync/current"] {
            assert!(!root.join(legacy).exists(), "{legacy} must be gone");
        }
        retire_legacy_roots(&mp); // idempotent: the next build finds nothing

        // A live `current` is a symlink TO a directory. Following it would
        // delete the generation this very preview is serving.
        let gen = mp.generation_dir("abc");
        std::fs::create_dir_all(&gen).unwrap();
        std::fs::write(gen.join("index.html"), b"<h1>live</h1>").unwrap();
        mp.set_current_ptr("abc").unwrap();
        retire_legacy_roots(&mp);
        assert!(
            mp.current_ptr().join("index.html").exists(),
            "the served generation must survive the pass that removes the legacy form"
        );
    }

    /// Every path the marker is applied to.
    ///
    /// Pinned as a set rather than asserted one at a time: the loop that marks
    /// them iterates the table, so the interesting question is what the WHOLE
    /// table asks for. Everything in it is regenerable — that is the only
    /// reason moss has to take a path away from a provider, and a path that
    /// needs a second reason needs an ADR first (ADR-062).
    #[test]
    fn the_marker_covers_the_regenerable_tree_and_nothing_else() {
        let mut excluded: Vec<&str> =
            MOSS_PATH_RULES.iter().filter(|r| r.cloud.is_excluded()).map(|r| r.rel).collect();
        excluded.sort_unstable();
        assert_eq!(
            excluded,
            [
                ".moss/build.nosync/",
                ".moss/build.nosync/cache/",
                ".moss/build.nosync/generations/",
                ".moss/build.nosync/staging/",
            ]
        );
        assert!(
            MOSS_PATH_RULES
                .iter()
                .filter(|r| r.cloud.is_excluded())
                .all(|r| !r.materialized),
            "an excluded path moss reads back and cannot rebuild is the moss#1079 trap"
        );
    }

    /// Spellings of "exclude everything under this directory" that git treats
    /// as equivalent. Recognizing them is the fix; over-recognizing a glob is
    /// how the fix would become a leak.
    #[test]
    fn equivalent_directory_spellings_are_recognized() {
        for line in ["data", "data/", "/data", "/data/", "data/*", "data/**"] {
            assert_eq!(excluded_dir(line), Some("data"), "{line:?}");
        }
        for line in ["", "  ", "# data/", "!data/redirects.json", "dat*/", "data/*.json", "/"] {
            assert_eq!(excluded_dir(line), None, "{line:?}");
        }
        // A broader rule the user wrote covers a narrower one moss wants.
        assert!(already_excluded("build/\n", "build/cache", false));
        // …but a sibling with a shared prefix does not.
        assert!(!already_excluded("database/\n", "data", true));
    }

    /// The three attributes are separate because the three sets are. Collapsing
    /// them to one `is_moss_written` boolean would unwatch `data/social/`, whose
    /// arrival must refresh the preview.
    #[test]
    fn social_data_is_moss_written_and_still_watched() {
        assert!(is_watchable_rel(".moss/data/social/matters.json"));
        assert!(is_watchable_rel(".moss/data/social"));
        // …while the rest of data/ is not: a deploy rewriting its own
        // bookkeeping used to wake the watcher.
        assert!(!is_watchable_rel(".moss/data/deployed-article-map.json"));
        assert!(!is_watchable_rel(".moss/data"));
    }

    #[test]
    fn build_output_is_never_watchable() {
        for rel in [
            ".moss",
            ".moss/build.nosync",
            ".moss/build.nosync/staging/index.html",
            ".moss/build.nosync/generations/abc/index.html",
            ".moss/cache/objects/ab/cd",
            ".moss/build.nosync/hashes.json",
            ".moss/site/index.html",
            ".moss/identity/secret-key",
            ".moss/plugins/github/main.bundle.js",
            // Not in the table at all: `.moss/` is moss's namespace, so an
            // unknown entry is excluded before anyone remembers to add it.
            ".moss/something-a-future-version-writes/state.json",
        ] {
            assert!(!is_watchable_rel(rel), "{rel} must never reach the watcher");
        }
    }

    #[test]
    fn user_editable_moss_files_stay_watchable() {
        for rel in [
            ".moss/config.toml",
            ".moss/theme/style.css",
            ".moss/theme/fonts/IM-Fell.woff2",
            ".moss/assets/logo.png",
        ] {
            assert!(is_watchable_rel(rel), "{rel} is the user's to edit");
        }
    }

    #[test]
    fn content_outside_moss_is_watchable() {
        assert!(is_watchable_rel("index.md"));
        assert!(is_watchable_rel("posts/hello.md"));
        // …except root agent-instruction files, which are tooling, not
        // content, whoever wrote them (#955).
        assert!(!is_watchable_rel("AGENTS.md"));
        assert!(!is_watchable_rel("CLAUDE.md"));
    }

    /// Longest-prefix matching is what lets one table hold both `.moss/data/`
    /// (not watched) and `.moss/data/social/` (watched). A `.moss/dataset/`
    /// must not be captured by the `.moss/data/` rule.
    #[test]
    fn prefix_matching_respects_path_boundaries() {
        assert_eq!(rule_for(".moss/data/social/x.json").map(|r| r.rel), Some(".moss/data/social/"));
        assert_eq!(rule_for(".moss/data/x.json").map(|r| r.rel), Some(".moss/data/"));
        assert!(rule_for(".moss/database.json").is_none());
        // File rules match exactly, never as a prefix.
        assert!(rule_for("AGENTS.md.bak").is_none());
    }

    /// Create a TempDir inside the repo's `target/test-tmp/` (gitignored, per
    /// project convention). `TempDir::new_in` does NOT create the parent, so we
    /// call `create_dir_all` first to avoid a flaky NotFound panic when the dir
    /// does not yet exist (e.g. fresh checkout or after `rm -rf target/test-tmp`).
    fn make_tmp() -> TempDir {
        // `CARGO_MANIFEST_DIR` is `crates/moss-build/`, so the repo's
        // `target/test-tmp` is two levels up — not one, as it was when this
        // file lived in `src-tauri/` (ADR-057 move).
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp");
        std::fs::create_dir_all(&base).expect("create target/test-tmp");
        TempDir::new_in(&base).expect("create test tempdir")
    }

    #[test]
    fn set_current_ptr_points_current_at_the_generation() {
        let tmp = make_tmp();
        let mp = make_paths(tmp.path());
        // Create the generation dir (with a marker) so `current` has content to
        // resolve to on both platforms.
        let gen_dir = mp.generation_dir("gen001");
        std::fs::create_dir_all(&gen_dir).unwrap();
        std::fs::write(gen_dir.join("marker.txt"), b"gen001").unwrap();
        // Ensure build dir exists for current_ptr
        std::fs::create_dir_all(mp.generations_dir()).unwrap();

        mp.set_current_ptr("gen001").unwrap();

        let link = mp.current_ptr();
        // Behavioral contract (both platforms): `current` resolves to the
        // generation's content.
        assert_eq!(
            std::fs::read(link.join("marker.txt")).unwrap(),
            b"gen001",
            "current must resolve to the generation's content"
        );
        // Unix repoints with an absolute symlink; Windows copies the generation in
        // (dir symlinks need elevation), so the mechanism differs per platform.
        #[cfg(unix)]
        {
            assert!(link.is_symlink(), "current should be a symlink");
            let target = std::fs::read_link(&link).unwrap();
            assert!(target.is_absolute(), "symlink target must be absolute");
            assert_eq!(target, gen_dir, "symlink must point to the generation dir");
        }
        #[cfg(windows)]
        {
            assert!(
                link.is_dir() && !link.is_symlink(),
                "current should be a real directory copy on Windows"
            );
        }
    }

    #[test]
    fn new_absolutizes_a_relative_project_root() {
        // Invariant: a relative project root is made absolute at construction.
        let mp = MossPaths::new(std::path::Path::new("some/relative/project"));
        assert!(mp.root().is_absolute(), "new() must absolutize a relative root");
        assert!(mp.generation_dir("g").is_absolute());
        // Absolute inputs are passed through byte-for-byte (no normalization).
        // Env-derived: "/abs/project" is not absolute on Windows.
        let abs = std::env::current_dir().unwrap();
        assert_eq!(MossPaths::new(&abs).root(), abs.join(".moss"));
    }

    #[test]
    fn project_root_is_the_folder_moss_lives_in_not_the_moss_dir() {
        // Regression (moss#976): `root()` reads like "the project root" but is
        // `<project>/.moss`. Passing it to `domain::config::get_*` (which join
        // `.moss/config.toml` themselves) or back into `MossPaths::new` builds
        // `<project>/.moss/.moss/...` — a path that never exists, so knobs
        // silently took their defaults and generation GC swept an empty tree.
        let abs = std::env::current_dir().unwrap();
        let mp = MossPaths::new(&abs);
        assert_eq!(mp.project_root(), abs);
        // Round-trip: reconstructing from `project_root()` is the identity.
        assert_eq!(MossPaths::new(mp.project_root()).root(), mp.root());
    }

    #[test]
    fn set_current_ptr_resolves_for_relative_project_root() {
        // Regression (2026-06-22): `moss build <relative-path>` must still produce
        // a `current` symlink that RESOLVES. Before the absolute-root invariant,
        // the relative gen_dir target dangled (a symlink resolves against its own
        // dir, .moss/build.nosync/, not the CWD), so the preview fell back to a stale or
        // empty site/ — the coldstart-blank class, re-exposed on the relative path.
        let tmp = make_tmp(); // absolute, under CARGO_MANIFEST_DIR/../target/test-tmp
        // Unit-test CWD is CARGO_MANIFEST_DIR (src-tauri); express the same dir as
        // a RELATIVE root so we exercise the path that used to dangle.
        let rel_root = std::path::Path::new("..")
            .join("target")
            .join("test-tmp")
            .join(tmp.path().file_name().unwrap());
        assert!(rel_root.is_relative(), "precondition: root must be relative");

        let mp = MossPaths::new(&rel_root);
        std::fs::create_dir_all(mp.generation_dir("gen001")).unwrap();
        std::fs::create_dir_all(mp.generations_dir()).unwrap();
        mp.set_current_ptr("gen001").unwrap();

        let link = mp.current_ptr();
        assert!(
            link.exists(),
            "current must resolve (not dangle) for a relative project root"
        );
        // The dangling-relative-target regression is unix-symlink-specific; on
        // Windows `current` is a copy, so there is no target to absolutize.
        #[cfg(unix)]
        assert!(
            std::fs::read_link(&link).unwrap().is_absolute(),
            "current symlink target must be absolute"
        );
    }

    #[test]
    fn set_current_ptr_second_call_replaces_first() {
        let tmp = make_tmp();
        let mp = make_paths(tmp.path());
        std::fs::create_dir_all(mp.generation_dir("gen001")).unwrap();
        std::fs::create_dir_all(mp.generation_dir("gen002")).unwrap();
        std::fs::create_dir_all(mp.generations_dir()).unwrap();

        mp.set_current_ptr("gen001").unwrap();
        mp.set_current_ptr("gen002").unwrap();

        // Behavioral contract (both platforms): current now resolves to gen002.
        assert_eq!(
            mp.current_generation_id().unwrap(),
            "gen002",
            "second call must repoint current at the new generation"
        );
        #[cfg(unix)]
        assert_eq!(
            std::fs::read_link(mp.current_ptr()).unwrap(),
            mp.generation_dir("gen002")
        );
        // No stale current.tmp
        assert!(!mp.current_ptr().with_extension("tmp").exists());
    }

    #[test]
    fn initial_serve_dir_prefers_current_when_present() {
        // A previously-built site: current -> generations/<id>. Cold start must
        // serve that frozen generation immediately (the regression: it served
        // the now-empty site/ and 404'd). We return the `current` SYMLINK path
        // itself so it stays valid across a later set_current_ptr swap.
        let tmp = make_tmp();
        let mp = make_paths(tmp.path());
        std::fs::create_dir_all(mp.generation_dir("gen001")).unwrap();
        std::fs::create_dir_all(mp.generations_dir()).unwrap();
        mp.set_current_ptr("gen001").unwrap();

        assert_eq!(mp.initial_serve_dir(), mp.current_ptr());
    }

    #[test]
    fn initial_serve_dir_falls_back_to_staging_when_no_current() {
        // No current pointer (e.g. an interrupted prior session) but staging/
        // has content: serve staging rather than the empty site/.
        let tmp = make_tmp();
        let mp = make_paths(tmp.path());
        std::fs::create_dir_all(mp.staging_dir()).unwrap();
        std::fs::write(mp.staging_dir().join("index.html"), b"<html></html>").unwrap();
        assert!(!mp.current_ptr().exists(), "precondition: no current pointer");

        assert_eq!(mp.initial_serve_dir(), mp.staging_dir());

        // Whether staging has anything in it no longer changes the answer:
        // the third fallback, `build/site`, is retired, and an empty staging
        // is still a valid `ServeDir` root.
        std::fs::remove_file(mp.staging_dir().join("index.html")).unwrap();
        assert_eq!(mp.initial_serve_dir(), mp.staging_dir());
    }

    #[test]
    fn ensure_dirs_removes_stale_current_tmp() {
        let tmp = make_tmp();
        let mp = make_paths(tmp.path());
        // Create the build dir and a stale current.tmp (simulating a killed process)
        std::fs::create_dir_all(mp.generations_dir()).unwrap();
        let stale_tmp = mp.current_ptr().with_extension("tmp");
        std::fs::write(&stale_tmp, b"stale").unwrap();
        assert!(stale_tmp.exists(), "precondition: current.tmp exists before ensure_dirs");

        mp.ensure_dirs().unwrap();

        assert!(!stale_tmp.exists(), "ensure_dirs must remove stale current.tmp");
    }

    #[test]
    fn test_paths() {
        // A unix-style literal ("/project") is not absolute on Windows and would be
        // absolutized against the CWD drive, so the base is env-derived and the
        // layout contract is pinned by the joined component literals below.
        let base = std::env::current_dir().unwrap().join("project");
        let paths = MossPaths::new(&base);
        let moss = base.join(".moss");
        assert_eq!(paths.root(), moss);
        assert_eq!(paths.config(), moss.join("config.toml"));
        assert_eq!(paths.state(), moss.join("state.toml"));
        assert_eq!(paths.theme_dir(), moss.join("theme"));
        assert_eq!(paths.identity_public(), moss.join("identity").join("public.json"));
        assert_eq!(paths.identity_secret(), moss.join("identity").join("secret-key"));
        assert_eq!(
            paths.generation_dir("abc123"),
            moss.join("build.nosync").join("generations").join("abc123")
        );
        assert_eq!(paths.current_ptr(), moss.join("build.nosync").join("current"));
        assert_eq!(paths.staging_dir(), moss.join("build.nosync").join("staging"));
        assert_eq!(paths.cache_dir(), moss.join("build.nosync").join("cache"));
        assert_eq!(paths.hashes(), moss.join("build.nosync").join("hashes.json"));
        assert_eq!(paths.article_map(), moss.join("build.nosync").join("article-map.json"));
        assert_eq!(paths.social_dir(), moss.join("data").join("social"));
        assert_eq!(
            paths.deployed_article_map(),
            moss.join("deploy").join("deployed-article-map.json")
        );
        // The &str constructor resolves identically (was from_project_str_resolves).
        let from_str = MossPaths::from_project_str(base.to_str().unwrap());
        assert_eq!(from_str.data_dir(), moss.join("data"));
    }

    #[test]
    fn test_ensure_dirs() {
        let temp = tempfile::tempdir().unwrap();
        let paths = MossPaths::new(temp.path());
        paths.ensure_dirs().unwrap();

        assert!(paths.root().exists());
        assert!(paths.identity_dir().exists());
        assert!(paths.theme_dir().exists());
        assert!(paths.plugins_dir().exists());
        assert!(paths.data_dir().exists());
        assert!(paths.social_dir().exists());
        assert!(paths.build_dir().exists());
        assert!(paths.cache_objects().exists());
        assert!(paths.cache_transforms().exists());
        assert!(paths.cache_tmp().exists());
        // site/ is no longer created by ensure_dirs — gen dirs are created on-demand
        // by materialize_and_promote. Verify staging and generations root exist:
        assert!(paths.staging_dir().exists());
        assert!(paths.generations_dir().exists());
    }

    // ─── setxattr-failure warn-once gate (#964 §4 visibility) ───────────────

    /// What the verify-after-set warning reads: absent before the marker is
    /// set, present after.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_cloud_sync_marker_reads_back_once_set() {
        let tmp = make_tmp();
        let dir = tmp.path().join("build.nosync");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!has_cloud_sync_marker(&dir), "an unmarked directory reads back unmarked");
        exclude_from_cloud_sync(&dir);
        assert!(has_cloud_sync_marker(&dir), "and a marked one reads back marked");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn should_warn_once_fires_only_on_first_sighting_of_a_dir() {
        let seen = std::sync::Mutex::new(std::collections::HashSet::new());
        let a = std::path::PathBuf::from("/tmp/a/.moss/build.nosync");
        let b = std::path::PathBuf::from("/tmp/b/.moss/build.nosync");

        assert!(should_warn_once(&a, &seen), "first sighting of a must warn");
        assert!(!should_warn_once(&a, &seen), "second sighting of a must stay quiet");
        assert!(should_warn_once(&b, &seen), "a different dir must still warn");
        assert!(!should_warn_once(&b, &seen), "and then go quiet in turn");
    }
}

/// Write `.moss/.gitignore`, adding any lines a prior moss version didn't know
/// about.
///
/// The append branch is why this isn't a one-line `write_if_absent`: a project
/// built before an entry existed would otherwise never get it. `agents/` is
/// exactly that case — generated guidance, versioned with the binary that
/// wrote it, not the author's to commit.
///
/// Only ever adds. A line the user deleted on purpose comes back, which is the
/// right trade for secrets; a line they *added* is preserved.
///
/// "Missing" is decided per DIRECTORY, not per line of text — that is the part
/// that used to be wrong. A user who spelled the rule as `data/*` plus
/// `!data/redirects.json`, the only way to keep one file under it in git, is
/// recognized as having satisfied it, so moss never appends a line that shadows
/// a negation. The exception is deliberate: for a rule that is not
/// [`MossPathRule::gitignore_negatable`] the negation is overridden instead.
pub(crate) fn ensure_moss_gitignore(moss_root: &std::path::Path) -> Result<(), String> {
    let path = moss_root.join(".gitignore");
    // `read_input_if_present`, not a raw read: this file lives in the synced
    // vault, and an evicted `.gitignore` reports NotFound on pre-Sonoma macOS
    // (the bytes move to a hidden `.gitignore.icloud` sibling). Treating that as
    // "absent" takes the first branch and REPLACES the user's file with the
    // stock one, dropping every line they added — the erase-vs-merge failure
    // moss#986 found in `redirects.json`, one directory over.
    let existing = match crate::build::cloud_readiness::read_input_if_present(&path) {
        Ok(Some(existing)) => existing,
        // Genuinely absent — the ordinary first-build case.
        Ok(None) => {
            // allow:raw_write .moss/.gitignore is user-visible state in the vault, not build output
            return std::fs::write(&path, moss_gitignore())
                .map_err(|e| format!("Failed to write .moss/.gitignore: {e}"));
        }
        // Present but unreadable: a directory in its place, bad permissions,
        // invalid UTF-8, or still in the cloud after the wait. Writing here
        // would destroy whatever is there, and a read failure is exactly the
        // case where moss cannot know what that is — so "only ever adds" would
        // become "replaced it wholesale". Nor is a build the place to litigate
        // it: the user asked for their site.
        Err(e) => {
            log::warn!(target: "build", ".moss/.gitignore unreadable, left alone: {e}");
            return Ok(());
        }
    };
    let missing: String = gitignore_rules()
        .filter(|(rule, _)| {
            let dir = rule.rel.strip_prefix(".moss/").unwrap_or(rule.rel).trim_end_matches('/');
            !already_excluded(&existing, dir, rule.gitignore_negatable)
        })
        .map(|(_, line)| format!("{line}\n"))
        .collect();
    if !missing.is_empty() {
        let sep = if existing.ends_with('\n') { "" } else { "\n" };
        // allow:raw_write .moss/.gitignore is user-visible state in the vault, not build output
        std::fs::write(&path, format!("{existing}{sep}{missing}"))
            .map_err(|e| format!("Failed to update .moss/.gitignore: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod gitignore_writer_tests {
    use super::*;
    use tempfile::TempDir;

    /// Create a TempDir inside the repo's `target/test-tmp/` (gitignored, per
    /// project convention). `TempDir::new_in` does NOT create the parent, so we
    /// call `create_dir_all` first to avoid a flaky NotFound panic when the dir
    /// does not yet exist (e.g. fresh checkout or after `rm -rf target/test-tmp`).
    fn make_tmp() -> TempDir {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp");
        std::fs::create_dir_all(&base).expect("create target/test-tmp");
        TempDir::new_in(&base).expect("create test tempdir")
    }

    // ─── `.moss/.gitignore` upkeep (moss#1001) ──────────────────────────────

    /// The exact file a real site ships so it can keep `redirects.json` — the
    /// list of old URLs it still answers — in git. `data/*` plus the negation is
    /// the ONLY spelling that works: git will not re-include a file whose parent
    /// directory is excluded, so a bare `data/` makes the `!` line inert.
    ///
    /// moss used to append `data/` here, below the negation, on every build.
    const SITE_THAT_COMMITS_ITS_REDIRECTS: &str = "\
identity/
data/*
!data/redirects.json
build/cache/
build/staging/
keys/
agents/
history/
";

    /// The regression. A build must not touch the user's rule, and must not
    /// append a line that shadows it.
    #[test]
    fn a_users_more_specific_rule_is_left_alone() {
        let tmp = make_tmp();
        let moss_root = tmp.path().join(".moss");
        std::fs::create_dir_all(&moss_root).unwrap();
        let path = moss_root.join(".gitignore");
        std::fs::write(&path, SITE_THAT_COMMITS_ITS_REDIRECTS).unwrap();

        ensure_moss_gitignore(&moss_root).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();

        assert!(
            !after.lines().any(|l| l.trim() == "data/"),
            "appending a bare `data/` below the negation silently untracks \
             redirects.json — that is moss#1001:\n{after}"
        );
        assert!(
            after.starts_with(SITE_THAT_COMMITS_ITS_REDIRECTS),
            "the user's lines must survive byte-for-byte and in order:\n{after}"
        );
        // Every rule this file owes is satisfied (deploy/ stopped being one at
        // v6), so a build must not add a single byte — and a second run is the
        // same no-op. The bug was only visible because it recurred on every
        // single build.
        assert_eq!(
            after,
            format!("{SITE_THAT_COMMITS_ITS_REDIRECTS}build.nosync/\ncache/\n"),
            "only a rule this file is actually missing may be added"
        );
        ensure_moss_gitignore(&moss_root).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), after, "not idempotent");
    }

    /// A file with every rule moss knows about must come back BYTE-IDENTICAL.
    /// Covers the shape actually on disk at the site that hit this: having had
    /// `data/` forced on them, the author appended `!data/` to undo it, so git
    /// descends into the directory again and the `data/*` + `!data/redirects.json`
    /// pair above recovers its meaning.
    ///
    /// That workaround is scar tissue from this bug, and it is still the user's
    /// line. A *build* does not tidy it away just because the fix makes it
    /// redundant — unasked-for normalization is the very thing being fixed
    /// here. The one-time v4→v5 migration
    /// (`infra::config_migrations::migrate_gitignore`) does clean it up, once,
    /// after writing a backup.
    #[test]
    fn the_workaround_a_site_wrote_to_survive_this_bug_is_untouched() {
        let workaround = "\
identity/
data/*
!data/redirects.json
build/cache/
build/staging/
keys/
agents/
deploy/
data/
!data/
history/
";
        let tmp = make_tmp();
        let moss_root = tmp.path().join(".moss");
        std::fs::create_dir_all(&moss_root).unwrap();
        let path = moss_root.join(".gitignore");
        std::fs::write(&path, workaround).unwrap();

        ensure_moss_gitignore(&moss_root).unwrap();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("{workaround}build.nosync/\ncache/\n"),
            "a live site's pushed .moss/.gitignore must survive a build byte-for-byte, \
             gaining only the two lines the split tree needs"
        );
    }

    /// A project built by an older moss has the bare `data/`. A build must
    /// converge on ONE rule — ending up with both `data/` and `data/*` would be
    /// harmless today and a trap the moment the user added a negation. Turning
    /// the old spelling into the new one is a one-time, backed-up migration
    /// (`infra::config_migrations::migrate_gitignore`), not something every
    /// build does.
    #[test]
    fn an_old_projects_bare_data_rule_is_not_duplicated() {
        let tmp = make_tmp();
        let moss_root = tmp.path().join(".moss");
        std::fs::create_dir_all(&moss_root).unwrap();
        let path = moss_root.join(".gitignore");
        let old = "identity/\nkeys/\ndata/\ndeploy/\nbuild/cache/\nbuild/staging/\nagents/\nhistory/\n";
        std::fs::write(&path, old).unwrap();

        ensure_moss_gitignore(&moss_root).unwrap();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("{old}build.nosync/\ncache/\n"),
            "`data/` already excludes everything `data/*` does; rewriting or \
             duplicating it would edit a file moss promised only to add to"
        );
    }

    /// The `keys/` guarantee, which the equivalence check must not soften. A
    /// keystore in a pushed repo is a leaked permanent site identity, so a `!`
    /// under it is overridden — moss re-appends the bare `keys/`, the form git
    /// cannot re-include through.
    #[test]
    fn a_negation_cannot_uncover_the_keystore() {
        let tmp = make_tmp();
        let moss_root = tmp.path().join(".moss");
        std::fs::create_dir_all(&moss_root).unwrap();
        let path = moss_root.join(".gitignore");
        std::fs::write(&path, "keys/*\n!keys/site.key\n").unwrap();

        ensure_moss_gitignore(&moss_root).unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        let keys_line = after.lines().position(|l| l.trim() == "keys/");
        let negation = after.lines().position(|l| l.trim() == "!keys/site.key");
        assert!(keys_line.is_some(), "moss must restore a bare `keys/`:\n{after}");
        assert!(keys_line > negation, "`keys/` must come AFTER the negation to override it");
    }

    /// Two lists of the same three filenames, kept in step by this test rather
    /// than by memory — the drift between exactly this pair is what #955 was.
    #[test]
    fn root_agent_files_match_the_classifier() {
        for name in crate::build::scan::classify::ROOT_AGENT_CONFIG_FILES {
            assert!(
                rule_for(name).is_some_and(|r| !r.watched),
                "{name} is written by moss and classified as an agent config, so it \
                 must be in MOSS_PATH_RULES and unwatched"
            );
        }
    }
}
