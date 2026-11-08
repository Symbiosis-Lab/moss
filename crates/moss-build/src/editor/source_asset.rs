//! Project-scoped SOURCE file bytes for the editor — the carrier-free core.
//!
//! The editor renders the source the user typed, not the build's output. The
//! one exception: an admitted raster source the build ladders is answered with
//! a built rung (`image/webp`), found from the source through the generation's
//! receipt and never from request text, while the build's stat record still
//! vouches for the source. `w=<ladder width>` asks for the widest built rung up
//! to that width, or the narrowest when every built rung is wider (another
//! value is 400); without `w`, the same up to [`EDITOR_DEFAULT_WIDTH`]. The
//! original is served only when
//! no fresh rung exists, or when the query carries the valueless flag
//! `original` (a view that needs the file itself, such as zoom); `original`
//! together with `w` is 400.
//!
//! [`serve_source_asset`] is the whole behaviour. Two carriers wrap it:
//!
//! - the `moss-source://` Tauri scheme (`source_asset_protocol::register`),
//!   used by the desktop app;
//! - `GET /__moss/source/*path` (`ops::serve::router::source_asset_route`),
//!   mounted by the preview server and by `moss edit --serve` — used by any
//!   host with no custom-scheme support, i.e. a browser or an Obsidian pane.
//!
//! They are adapters, not implementations: each resolves a project root its own
//! way and dresses the shared answer, with the headers it names
//! ([`SourceAssetResponse::headers`]), in its own response type. Anything that
//! decides WHAT to serve belongs here, so the two carriers cannot drift.
//! [`resolve_scoped`] is the containment boundary for both — no caller may
//! widen it. Nothing in this file names a `tauri::` type; the scheme mount
//! stays in the `source_asset_protocol` sibling.

use crate::build::manifest::preview_originals::Originals;
use crate::build::stat::FileStat;
use crate::types::content::SiteHashes;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Resolve a request path (the URL path of `moss-source://localhost/<p>`,
/// percent-DECODED, leading '/' allowed) to an absolute source file path,
/// rejecting anything that escapes `project_root`. Returns None on escape or
/// when the resolved path is not an existing file.
pub fn resolve_scoped(project_root: &Path, request_path: &str) -> Option<PathBuf> {
    let rel = request_path.trim_start_matches('/');
    if rel.is_empty() {
        return None;
    }
    let canonical_root = std::fs::canonicalize(project_root).ok()?;
    let joined = canonical_root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    let canonical = std::fs::canonicalize(&joined).ok()?;
    if !canonical.starts_with(&canonical_root) {
        return None; // escaped the project root
    }
    if !canonical.is_file() {
        return None;
    }
    // Staying inside the vault is not enough: the vault CONTAINS secrets.
    // `.moss/identity-key` is the user's private signing key and `.moss/config.toml`
    // can hold an api_key, so "scoped to the project" would still serve them. That
    // was survivable while the only caller was moss's own webview; it is not once
    // the same resolver answers on a TCP port every local process can reach.
    //
    // Deny any dot-prefixed component, the rule every general-purpose web server
    // uses. It covers `.moss/`, and also `.git/`, `.env` and `.ssh` if a vault ever
    // contains them. One carve-out: `.moss/theme/` is the site's design assets,
    // served verbatim on the published site at `/_moss/theme/`, so there is no
    // secret to keep and the editor's viewer needs them (a theme image otherwise
    // listed in the tree but 404 in the viewer). The dot rule still applies to
    // components below `theme/`.
    let rel = canonical.strip_prefix(&canonical_root).ok()?;
    // On a miss `rel` still starts with `.moss`, which the dot rule then denies.
    let inside = rel.strip_prefix(".moss/theme").unwrap_or(rel);
    let denied_dotfile = inside.components().any(|c| {
        matches!(c, std::path::Component::Normal(name)
            if name.to_string_lossy().starts_with('.'))
    });
    if denied_dotfile {
        return None;
    }
    Some(canonical)
}

/// What a source-asset request resolved to, independent of how it arrived.
///
/// Deliberately not a `tauri::http::Response` or an `axum::Response`: this is
/// the answer, and each mount dresses it in its own carrier's type. Keeping it
/// carrier-free is what lets the two mounts share one implementation instead of
/// being two implementations held in sync by a test.
pub struct SourceAssetResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl SourceAssetResponse {
    pub fn not_found() -> Self {
        Self {
            status: 404,
            content_type: "text/plain; charset=utf-8".to_string(),
            body: b"Not Found".to_vec(),
        }
    }

    pub fn bad_request(why: &str) -> Self {
        Self {
            status: 400,
            content_type: "text/plain; charset=utf-8".to_string(),
            body: why.as_bytes().to_vec(),
        }
    }

    /// Every header a carrier sends with this answer. Source bytes change on
    /// every save (the editor busts with `?_t=`), so nothing may cache them;
    /// `nosniff` holds the browser to the declared type, and the sandbox keeps
    /// a file in the folder from running script on the origin that shows it.
    /// The browser's PDF viewer still draws a PDF under it.
    pub fn headers(&self) -> [(&'static str, &str); 4] {
        [
            ("Content-Type", self.content_type.as_str()),
            ("Cache-Control", "no-cache"),
            ("X-Content-Type-Options", "nosniff"),
            ("Content-Security-Policy", SANDBOXED),
        ]
    }
}

const SANDBOXED: &str = "sandbox; default-src 'none'; img-src 'self' data:; media-src 'self'; style-src 'unsafe-inline'";

/// Resolve and read a project-scoped SOURCE file, or the built rung that stands
/// in for it (see the module header) — the whole behaviour behind both mounts
/// (the `moss-source://` Tauri scheme and the preview server's
/// `/__moss/source/` route).
///
/// `request_path` must already be percent-DECODED; `query` is the request's
/// raw query string, which only `w` and `original` are read from. Escapes outside
/// `project_root` and missing files are 404 by way of [`resolve_scoped`], which
/// is the security boundary; nothing here may widen it.
pub async fn serve_source_asset(project_root: &Path, request_path: &str, query: Option<&str>) -> SourceAssetResponse {
    let Ok(wanted) = wanted(query) else {
        return SourceAssetResponse::bad_request("w must be one ladder width, and cannot be combined with original");
    };
    let Some(file) = resolve_scoped(project_root, request_path) else {
        return SourceAssetResponse::not_found();
    };
    let (root, source) = (project_root.to_path_buf(), file.clone());
    let rung = match wanted {
        Wanted::Original => None,
        Wanted::AtMost(w) => tokio::task::spawn_blocking(move || widest_up_to(built_rungs(&root, &source), w))
            .await.ok().flatten().map(|(_, rung)| rung),
    };
    if let Some(rung) = rung {
        // An unreadable rung (evicted, swept mid-read) falls through to the original.
        if let Ok(body) = read_source_asset(rung).await {
            return SourceAssetResponse { status: 200, content_type: "image/webp".to_string(), body };
        }
    }
    match read_source_asset(file.clone()).await {
        Ok(body) => SourceAssetResponse {
            status: 200,
            content_type: crate::editor::resolve::asset_resolver::mime_for_path(&file).to_string(),
            body,
        },
        Err(_) => SourceAssetResponse::not_found(),
    }
}

/// The width a request without `w` asks for: the editor's text column drawn
/// on a 2x display.
const EDITOR_DEFAULT_WIDTH: u32 = 800;

#[derive(Clone, Copy, Debug)]
enum Wanted {
    AtMost(u32),
    Original,
}

/// The widest of `rungs` (narrowest first) no wider than `w`, or the narrowest
/// when every one is wider.
fn widest_up_to<T>(rungs: Vec<(u32, T)>, w: u32) -> Option<(u32, T)> {
    let mut rungs = rungs.into_iter();
    let narrowest = rungs.next()?;
    Some(rungs.take_while(|(rung, _)| *rung <= w).last().unwrap_or(narrowest))
}

/// What a raw query string asks for. `Err` when `w` is not exactly one width
/// of the ladder, when `original` carries a value or repeats, or when both
/// are given.
fn wanted(query: Option<&str>) -> Result<Wanted, ()> {
    let (mut width, mut original) = (None, false);
    for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        match &*key {
            "w" => {
                let w = value.parse::<u32>().ok().filter(|w| moss_core::asset_paths::LADDER.contains(w)).ok_or(())?;
                if width.replace(w).is_some() {
                    return Err(());
                }
            }
            "original" if value.is_empty() && !original => original = true,
            "original" => return Err(()),
            _ => {}
        }
    }
    match (width, original) {
        (Some(_), true) => Err(()),
        (Some(w), false) => Ok(Wanted::AtMost(w)),
        (None, true) => Ok(Wanted::Original),
        (None, false) => Ok(Wanted::AtMost(EDITOR_DEFAULT_WIDTH)),
    }
}

/// The current generation's rungs for `file`, narrowest first, or none
/// unless the build's own record proves the source unchanged since: each rung
/// was made from bytes with the oid the build's source record holds, and that
/// record's stat (size, mtime to the nanosecond, ctime, inode) still vouches
/// for the file.
pub(crate) fn built_rungs(project_root: &Path, file: &Path) -> Vec<(u32, PathBuf)> {
    static RECORDS: std::sync::LazyLock<BuildRecords> = std::sync::LazyLock::new(BuildRecords::default);
    built_rungs_from(&RECORDS, project_root, file).unwrap_or_default()
}

/// The receipt and the source records, kept between requests. A generation
/// never changes, so its id names its receipt; `hashes.json` is rewritten by
/// every build, so its own stat is part of its key.
#[derive(Default)]
struct BuildRecords {
    receipts: crate::build::manifest::backfill::Memo<(PathBuf, String), Arc<Originals>>,
    hashes: crate::build::manifest::backfill::Memo<(PathBuf, FileStat), Arc<SiteHashes>>,
}

impl BuildRecords {
    fn receipt(&self, mp: &crate::moss_paths::MossPaths, generation: &str) -> Option<Arc<Originals>> {
        let key = (mp.generations_dir(), generation.to_string());
        if let Some(hit) = self.receipts.get(&key) {
            return hit;
        }
        let receipt = Originals::read(&key.0, generation).ok().flatten().map(Arc::new);
        self.receipts.put(&key, &receipt);
        receipt
    }

    fn hashes(&self, mp: &crate::moss_paths::MossPaths) -> Option<Arc<SiteHashes>> {
        let key = (mp.hashes(), FileStat::of(&std::fs::metadata(mp.hashes()).ok()?));
        if let Some(hit) = self.hashes.get(&key) {
            return hit;
        }
        let hashes = SiteHashes::read(&key.0).map(Arc::new);
        self.hashes.put(&key, &hashes);
        hashes
    }
}

fn built_rungs_from(records: &BuildRecords, project_root: &Path, file: &Path) -> Option<Vec<(u32, PathBuf)>> {
    let ext = file.extension()?.to_str()?;
    if !moss_core::asset_paths::is_ladder_source_ext(ext) {
        return None;
    }
    let root = std::fs::canonicalize(project_root).ok()?;
    let source = file.strip_prefix(&root).ok()?.to_str()?.replace('\\', "/");
    let mp = crate::moss_paths::MossPaths::new(&root);
    let generation = mp.current_generation_id().ok()?;
    let generation_dir = mp.sealed_generation_dir(&generation)?;
    let receipt = records.receipt(&mp, &generation)?;
    let rungs = receipt.rungs(&source);
    if rungs.is_empty() {
        return None;
    }
    let hashes = records.hashes(&mp)?;
    let record = hashes.source_metadata_for(&source)?;
    if !record.vouches_for(&FileStat::of(&std::fs::metadata(file).ok()?), hashes.captured_at) {
        return None;
    }
    Some(rungs.into_iter().filter(|(_, (_, original))| original.oid == record.hash).filter_map(|(width, (url, _))| {
        let rung = std::fs::canonicalize(generation_dir.join(url)).ok()?;
        (rung.starts_with(&generation_dir) && rung.is_file()).then_some((width, rung))
    }).collect())
}

/// Reads a source asset off the async runtime, waiting briefly for a
/// cloud-evicted file to come back.
///
/// `tokio::task::spawn_blocking`, not `tauri::async_runtime::spawn_blocking`:
/// both mounts already run inside a tokio runtime (the axum handler natively;
/// the Tauri scheme via `async_runtime::spawn`, which drives Tauri's own tokio
/// runtime), and this file is the tauri-free half of the split.
async fn read_source_asset(file: std::path::PathBuf) -> std::io::Result<Vec<u8>> {
    tokio::task::spawn_blocking(move || {
        crate::build::cloud_readiness::read_with_materialize_wait(
            &file,
            crate::build::cloud_readiness::INTERACTIVE_DEADLINE,
        )
    })
    .await
    .unwrap_or_else(|e| Err(std::io::Error::other(e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn every_answer_is_held_to_its_type_and_sandboxed() {
        let answer = |content_type: &str| SourceAssetResponse { status: 200, content_type: content_type.to_string(), body: Vec::new() };
        for r in [answer("image/svg+xml"), answer("application/pdf"), SourceAssetResponse::not_found()] {
            let headers = r.headers();
            assert!(headers.contains(&("Content-Type", r.content_type.as_str())));
            assert!(headers.contains(&("X-Content-Type-Options", "nosniff")), "{}", r.content_type);
            assert!(headers.contains(&("Cache-Control", "no-cache")), "{}", r.content_type);
            assert!(headers.iter().any(|(name, v)| *name == "Content-Security-Policy" && v.starts_with("sandbox;")), "{}", r.content_type);
        }
    }

    #[test]
    fn resolves_nested_source_file_and_rejects_escape() {
        let root = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let p = root.path();
        fs::create_dir_all(p.join("图片/摄影")).unwrap();
        fs::write(p.join("图片/摄影/DSCF4053.jpeg"), b"jpegbytes").unwrap();

        // In-root nested path resolves.
        let got = resolve_scoped(p, "/图片/摄影/DSCF4053.jpeg").unwrap();
        assert!(got.ends_with("图片/摄影/DSCF4053.jpeg"));

        // Escape attempt is rejected.
        assert!(resolve_scoped(p, "/../../etc/passwd").is_none());
        // Missing file is rejected.
        assert!(resolve_scoped(p, "/nope.jpg").is_none());
    }

    /// Being inside the vault is not enough to be servable.
    ///
    /// `.moss/identity-key` is the user's private signing key and it lives INSIDE
    /// the vault, so the containment check alone happily resolves it. That was
    /// tolerable while only moss's own webview could ask; it is a key disclosure
    /// once the same resolver answers on a TCP port. Each assertion below is a
    /// real file this would have served.
    #[test]
    fn secrets_inside_the_vault_are_refused_even_though_they_are_contained() {
        let root = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let p = root.path();
        fs::create_dir_all(p.join(".moss")).unwrap();
        fs::write(p.join(".moss/identity-key"), b"DEADBEEF").unwrap();
        fs::write(p.join(".moss/config.toml"), b"api_key = \"sk-live\"").unwrap();
        fs::create_dir_all(p.join(".git")).unwrap();
        fs::write(p.join(".git/config"), b"[remote]").unwrap();
        fs::write(p.join(".env"), b"TOKEN=1").unwrap();

        // Sanity: these files really exist and really are inside the vault, so a
        // None here means the dotfile rule refused them — not that they were missing.
        assert!(p.join(".moss/identity-key").is_file());

        assert!(resolve_scoped(p, "/.moss/identity-key").is_none(), "private key");
        assert!(resolve_scoped(p, "/.moss/config.toml").is_none(), "config w/ api_key");
        assert!(resolve_scoped(p, "/.git/config").is_none(), "git config");
        assert!(resolve_scoped(p, "/.env").is_none(), "dotfile at the root");

        // Theme assets are public on the built site (`/_moss/theme/`), so the
        // fence lets them through — that is what the editor's viewer loads.
        fs::create_dir_all(p.join(".moss/theme")).unwrap();
        fs::write(p.join(".moss/theme/grain.png"), b"png").unwrap();
        fs::write(p.join(".moss/theme/.secret"), b"x").unwrap();
        assert!(resolve_scoped(p, "/.moss/theme/grain.png").is_some(), "theme asset served");
        assert!(resolve_scoped(p, "/.moss/theme/.secret").is_none(), "dotfile under theme");

        // And the rule must not swallow ordinary content.
        fs::write(p.join("photo.jpg"), b"jpeg").unwrap();
        assert!(resolve_scoped(p, "/photo.jpg").is_some(), "normal file still served");
    }

    /// A promoted generation whose receipt records `SOURCE` under a folder
    /// override (`gallery/`), so the default path mapping could not find it.
    /// The rung files themselves are written by each test that wants them.
    const SOURCE: &str = "Photos/Holiday/beach.png";
    const RUNG: &str = "gallery/beach.w800.webp";
    const RUNG_1600: &str = "gallery/beach.w1600.webp";

    struct Built {
        _temp: tempfile::TempDir,
        root: PathBuf,
        generation: PathBuf,
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(width, height).write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    fn built_site() -> Built {
        use crate::build::manifest::PendingManifest;
        let temp = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let root = temp.path().canonicalize().unwrap();
        let mp = crate::moss_paths::MossPaths::new(&root);
        let source = root.join(SOURCE);
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, png(2000, 1000)).unwrap();
        // Saved well before the build, outside the racy-write window that
        // keeps a record taken in the same instant from vouching for the file.
        let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options().write(true).open(&source).unwrap().set_modified(an_hour_ago).unwrap();
        let mut hashes = crate::types::content::SiteHashes::default();
        hashes.sources.insert(SOURCE.into(), crate::build::types::SourceMetadata::from_stat(
            crate::build::cache::ObjectStore::hash_file(&source).unwrap(),
            crate::build::stat::FileStat::of(&fs::metadata(&source).unwrap()),
        ));
        let mut pending = PendingManifest::new(hashes);
        pending.register_preview_original("gallery/beach.png".into(), SOURCE.into());
        pending.register_preview_original(RUNG.into(), SOURCE.into());
        pending.register_preview_original(RUNG_1600.into(), SOURCE.into());
        fs::create_dir_all(mp.staging_dir()).unwrap();
        fs::write(mp.staging_dir().join("index.html"), b"<!doctype html>").unwrap();
        pending.register(&crate::build::served_path::ServedPath::from_source("index.html").unwrap(),
            b"<!doctype html>", crate::build::manifest::HashBucket::Files);
        let sealed = pending.seal();
        crate::build::ship::materialize_and_promote(&sealed, &mp, &mp.staging_dir(), 1, None,
            crate::build::ship::ShipVerdict::Ship).unwrap();
        sealed.write_to_disk(&mp.hashes()).unwrap();
        let generation = mp.generation_dir(&mp.current_generation_id().unwrap());
        Built { _temp: temp, root, generation }
    }

    impl Built {
        fn with_rungs(self, rungs: &[&str]) -> Self {
            for name in rungs {
                let width = if *name == RUNG_1600 { 1600 } else { 800 };
                let rung = self.generation.join(name);
                fs::create_dir_all(rung.parent().unwrap()).unwrap();
                fs::write(&rung, png(width, width / 2)).unwrap();
            }
            self
        }

        fn with_rung(self) -> Self {
            self.with_rungs(&[RUNG])
        }

        async fn get(&self, query: &str) -> SourceAssetResponse {
            serve_source_asset(&self.root, &format!("/{SOURCE}"), Some(query)).await
        }
    }

    #[tokio::test]
    async fn the_original_is_served_until_a_rung_is_built_then_the_rung() {
        let site = built_site();
        let got = serve_source_asset(&site.root, &format!("/{SOURCE}"), None).await;
        assert_eq!((got.status, got.content_type.as_str()), (200, "image/png"));
        assert_eq!(got.body, fs::read(site.root.join(SOURCE)).unwrap());

        let site = site.with_rung();
        let got = serve_source_asset(&site.root, &format!("/{SOURCE}"), None).await;
        assert_eq!((got.status, got.content_type.as_str()), (200, "image/webp"));
        assert_eq!(got.body, fs::read(site.generation.join(RUNG)).unwrap());
    }

    /// An editor's atomic save replaces the photo after the build recorded it,
    /// keeping its size and its exact modification time, so the rung is newer
    /// than the source and a size or timestamp comparison would vouch for it.
    /// Only the build's stat record (here, the inode) tells them apart.
    #[tokio::test]
    async fn an_edited_source_with_a_stale_rung_gets_the_original() {
        let site = built_site().with_rung();
        let source = site.root.join(SOURCE);
        let before = fs::metadata(&source).unwrap();
        let mut edited = png(2000, 1000);
        let last = edited.len() - 20;
        edited[last] ^= 0xff;
        assert_eq!(edited.len() as u64, before.len());
        let saved = source.with_extension("png.tmp");
        fs::write(&saved, &edited).unwrap();
        fs::File::options().write(true).open(&saved).unwrap().set_modified(before.modified().unwrap()).unwrap();
        fs::rename(&saved, &source).unwrap();
        assert_eq!(fs::metadata(&source).unwrap().modified().unwrap(), before.modified().unwrap());
        assert!(fs::metadata(site.generation.join(RUNG)).unwrap().modified().unwrap() >= before.modified().unwrap());

        let got = serve_source_asset(&site.root, &format!("/{SOURCE}"), None).await;
        assert_eq!((got.status, got.content_type.as_str()), (200, "image/png"));
        assert_eq!(got.body, edited);
    }

    /// A later build recorded the edited photo (its real hash and stat) but has
    /// not promoted its generation, so the current rungs were made from the
    /// old bytes: the record vouches for the file, yet names other bytes.
    #[tokio::test]
    async fn a_record_of_newer_bytes_than_the_rung_gets_the_original() {
        let site = built_site().with_rung();
        let source = site.root.join(SOURCE);
        let mut edited = png(2000, 1000);
        let last = edited.len() - 20;
        edited[last] ^= 0xff;
        fs::write(&source, &edited).unwrap();
        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        fs::File::options().write(true).open(&source).unwrap().set_modified(two_hours_ago).unwrap();
        let hashes_path = crate::moss_paths::MossPaths::new(&site.root).hashes();
        let mut hashes = SiteHashes::read(&hashes_path).unwrap();
        hashes.sources.insert(SOURCE.into(), crate::build::types::SourceMetadata::from_stat(
            crate::build::cache::ObjectStore::hash_file(&source).unwrap(),
            FileStat::of(&fs::metadata(&source).unwrap()),
        ));
        fs::write(&hashes_path, serde_json::to_vec(&hashes).unwrap()).unwrap();

        let got = serve_source_asset(&site.root, &format!("/{SOURCE}"), None).await;
        assert_eq!((got.status, got.content_type.as_str()), (200, "image/png"));
        assert_eq!(got.body, edited);
    }

    #[tokio::test]
    async fn a_width_below_every_built_rung_gets_the_narrowest_rung_not_the_original() {
        let site = built_site().with_rungs(&[RUNG_1600]);
        let got = site.get("w=800").await;
        assert_eq!((got.status, got.content_type.as_str()), (200, "image/webp"));
        assert_eq!(got.body, fs::read(site.generation.join(RUNG_1600)).unwrap());
    }

    /// Without `w` the editor gets the widest rung up to its default width, so
    /// a ladder that starts below it does not hand the editor a thumbnail.
    #[test]
    fn without_a_width_the_editor_gets_the_widest_rung_up_to_its_default() {
        let wanted = wanted(None).unwrap();
        let Wanted::AtMost(w) = wanted else { panic!("no width asked for {wanted:?}") };
        assert_eq!(widest_up_to(vec![(400, ()), (800, ()), (1600, ())], w).map(|(width, _)| width), Some(800));
        assert_eq!(widest_up_to(vec![(1600, ())], w).map(|(width, _)| width), Some(1600), "every rung wider: the narrowest");
        assert_eq!(widest_up_to(Vec::<(u32, ())>::new(), w), None);
    }

    #[test]
    fn the_receipt_is_read_once_per_generation() {
        let site = built_site().with_rung();
        let records = BuildRecords::default();
        let source = site.root.join(SOURCE);
        let widths = |records: &BuildRecords| built_rungs_from(records, &site.root, &source)
            .unwrap_or_default().into_iter().map(|(width, _)| width).collect::<Vec<_>>();
        assert_eq!(widths(&records), vec![800]);
        let mp = crate::moss_paths::MossPaths::new(&site.root);
        let generation = mp.current_generation_id().unwrap();
        fs::write(crate::build::manifest::preview_originals::receipt_path(&mp.generations_dir(), &generation), b"{").unwrap();
        assert_eq!(widths(&records), vec![800], "a second request re-read the receipt");

        let next = "0123456789abcdef";
        fs::create_dir_all(mp.generation_dir(next)).unwrap();
        Originals { generation: next.into(), images: Default::default() }.bind(&mp.generations_dir()).unwrap();
        mp.set_current_ptr(next).unwrap();
        assert_eq!(widths(&records), Vec::<u32>::new(), "the new generation was not picked up");
    }

    /// The rung is reached only through an admitted source; naming it, or
    /// anything else in the build output, is still refused.
    #[tokio::test]
    async fn a_request_cannot_name_the_rung_or_the_build_output() {
        let site = built_site().with_rung();
        let generation = site.generation.file_name().unwrap().to_str().unwrap().to_string();
        for path in [
            format!("/{RUNG}"),
            "/gallery/beach.png".to_string(),
            format!("/.moss/build.nosync/current/{RUNG}"),
            format!("/.moss/build.nosync/generations/{generation}/{RUNG}"),
            "/.moss/build.nosync/current/index.html".to_string(),
        ] {
            assert_eq!(serve_source_asset(&site.root, &path, None).await.status, 404, "{path}");
        }
    }

    #[tokio::test]
    async fn a_width_outside_the_ladder_is_refused() {
        let site = built_site().with_rungs(&[RUNG, RUNG_1600]);
        for query in ["w=1000", "w=abc", "w=", "w=800&w=1600"] {
            assert_eq!(site.get(query).await.status, 400, "{query}");
        }
        assert_eq!(site.get("_t=1&w=1600").await.status, 200, "other parameters pass");
    }

    #[tokio::test]
    async fn the_original_flag_serves_the_source_while_a_rung_is_fresh() {
        let site = built_site().with_rungs(&[RUNG, RUNG_1600]);
        let got = site.get("original").await;
        assert_eq!((got.status, got.content_type.as_str()), (200, "image/png"));
        assert_eq!(got.body, fs::read(site.root.join(SOURCE)).unwrap());
    }

    #[tokio::test]
    async fn the_original_flag_with_a_width_is_refused() {
        let site = built_site().with_rungs(&[RUNG, RUNG_1600]);
        for query in ["original&w=800", "w=1600&original", "original=1", "original&original"] {
            assert_eq!(site.get(query).await.status, 400, "{query}");
        }
    }

    #[tokio::test]
    async fn the_requested_rung_is_served_when_built() {
        let site = built_site().with_rungs(&[RUNG, RUNG_1600]);
        let got = site.get("w=1600").await;
        assert_eq!((got.status, got.content_type.as_str()), (200, "image/webp"));
        assert_eq!(got.body, fs::read(site.generation.join(RUNG_1600)).unwrap());
        assert_eq!(site.get("w=800").await.body, fs::read(site.generation.join(RUNG)).unwrap());
    }

    #[tokio::test]
    async fn a_missing_wider_rung_falls_back_to_the_widest_built_below_it() {
        let site = built_site().with_rung();
        let got = site.get("w=1600").await;
        assert_eq!((got.status, got.content_type.as_str()), (200, "image/webp"));
        assert_eq!(got.body, fs::read(site.generation.join(RUNG)).unwrap());
    }

    /// The editor sizes the image box from the dimensions before any bytes
    /// arrive, so they stay the source's while a rung exists, and offers the
    /// listed widths in a `srcset`.
    #[test]
    fn the_resolver_lists_the_built_rung_widths_and_the_source_dimensions() {
        use crate::editor::resolve::asset_resolver::{resolve_asset, EditorScope};
        let resolve = |site: &Built| resolve_asset(SOURCE, &site.root.join("index.md"), &site.root, EditorScope::Site).unwrap();
        let widths = |site: &Built| resolve(site).rung_widths;
        let asset = resolve(&built_site().with_rungs(&[RUNG, RUNG_1600]));
        assert_eq!(asset.request_url, format!("/{SOURCE}"));
        assert_eq!((asset.width, asset.height), (Some(2000), Some(1000)));
        assert_eq!(asset.rung_widths, vec![800, 1600]);
        assert_eq!(widths(&built_site().with_rung()), vec![800]);
        assert_eq!(widths(&built_site()), Vec::<u32>::new());
    }

    /// The tree's `.moss/` allowlist and this fence were once written apart, and
    /// a theme image listed in the tree 404'd in the viewer. Walk the allowlist:
    /// the theme is the one entry whose files the viewer loads as assets and it
    /// must resolve; the rest are opened as text through `read_file`, never as
    /// an asset URL, and must stay refused (`config.toml` can hold secrets).
    #[test]
    fn tree_allowlist_and_fence_agree() {
        use crate::build::scan::classify::MOSS_INTERNAL_ALLOWLIST;
        let root = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let p = root.path();
        for allowed in MOSS_INTERNAL_ALLOWLIST {
            let probe = if allowed.ends_with(".toml") {
                fs::create_dir_all(p.join(".moss")).unwrap();
                allowed.to_string()
            } else {
                fs::create_dir_all(p.join(allowed)).unwrap();
                format!("{allowed}/probe.png")
            };
            fs::write(p.join(&probe), b"x").unwrap();
            let served = resolve_scoped(p, &format!("/{probe}")).is_some();
            assert_eq!(served, allowed.starts_with(".moss/theme"), "{probe}");
        }
    }
}
