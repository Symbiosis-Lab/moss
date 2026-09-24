//! The asset promise registry — app-runtime side of the M6a boundary.
//!
//! The explicit promise model: the pipeline registers every
//! image/video variant URL as Pending, the encoder transitions it to
//! Ready/Failed, and the preview server consults the state to decide what
//! bytes to serve. Request-time state of the running preview, so it stays
//! on the app side; the sibling [`super::content`] module holds the pure
//! build data.
//!
//! Split out of `types.rs` 2026-08-10 (M5a); `types.rs` re-exports every
//! item at its old path, so consumers are unchanged.

/// Placeholder metadata for pending assets during staged build.
///
/// Deliberately carries NO LQIP. A blurred stand-in is a *published-site*
/// technique — it buys a real visitor a fast first paint over the network. In
/// the preview the audience is the author, who is looking at the picture to
/// judge it, and a blur that resembles their photo is indistinguishable from a
/// badly-encoded result. So the preview shows the full original instead
/// (`AssetRegistry::source_passthrough` + the router's passthrough branch), and
/// this struct only holds what the video placeholder needs to draw a neutral
/// box: `dimensions` (reserve the layout) and `dominant_color`.
///
/// The LQIP itself still exists and still ships — computed at scan, carried in
/// `AssetSnapshot::lqip`, and baked into published HTML by
/// `moss_core::render::image`. It is simply never served by the preview server.
/// The field was removed from here on 2026-08-21 so that stays true by
/// construction.
#[derive(Debug, Clone)]
pub struct AssetPlaceholder {
    pub dimensions: Option<(u32, u32)>,
    pub dominant_color: Option<String>,
}

/// State of an asset in the staged build pipeline.
///
/// Pattern: explicit promise model. The pipeline commits to producing an
/// asset by registering Pending; `convert_single_image` transitions to Ready
/// on successful materialization at the served path or Failed on terminal
/// encode or canonical-link failure. The preview server's
/// `handle_asset_request` consults this state to decide what to serve when
/// the file isn't on disk yet.
///
/// Analogs: GraphQL `@defer` (schema-declared deferred fields,
/// github.com/graphql/graphql-spec/blob/master/rfcs/DeferStream.md), Bazel
/// `ActionResult` (action-declared `output_files` contract,
/// bazel.build/remote/caching).
#[derive(Debug, Clone)]
pub enum AssetState {
    Pending(AssetPlaceholder),
    /// The asset has been materialized at the served path.
    ///
    /// Carries nothing, and that is the point: this state answers one question
    /// ("is the promise kept?"), and every reader asks only that. It used to
    /// carry an `lqip_data_uri` for an editor hover-preview whose
    /// consumer had moved to `moss-asset://` source bytes, and then a
    /// `file_path` that no reader ever destructured — both removed 2026-08-21
    /// rather than left as state that invites a stale comment. Where the bytes
    /// landed is a filesystem fact; the preview server asks the filesystem.
    Ready,
    /// Terminal encode or canonical-link failure. The `String` carries the
    /// error message so the preview server can annotate the warning response
    /// it returns to the browser. Added 2026-05-20 to close the silent-warn
    /// trap verified at `~/Library/Logs/host.moss.publisher/moss.log`
    /// L2006/L2446/L2450 (2026-05-19).
    Failed(String),
}

/// Shared state for tracking asset processing status during staged build.
#[derive(Debug, Default)]
pub struct AssetRegistry {
    pub assets: std::sync::RwLock<std::collections::HashMap<String, AssetState>>,
    /// Source-passthrough map: output-URL → ABSOLUTE source file path.
    ///
    /// Populated in the same `set_pending` loops (blocking.rs) that register a
    /// promised image/video variant. The preview server serves the FULL
    /// ORIGINAL source bytes for a not-yet-encoded variant URL (instead of a
    /// blurry LQIP), so the very first paint is sharp. The background encoder
    /// later lands the real webp/mp4; publish awaits the background drain
    /// before sealing, so the on-disk/deployed generation always contains the
    /// encoded variant, by construction.
    ///
    /// Honest-mirror: every entry here is a source that `set_pending` promised
    /// AND `copy_deferred_assets` will copy into staging → generation → deploy,
    /// so the passthrough never surfaces bytes the deploy won't reproduce. The
    /// value is a pre-registered absolute path (never client-joined), so the
    /// router hand-off is path-traversal-safe.
    pub source_passthrough:
        std::sync::RwLock<std::collections::HashMap<String, std::path::PathBuf>>,
}

impl AssetRegistry {
    pub fn new() -> Self {
        Self {
            assets: std::sync::RwLock::new(std::collections::HashMap::new()),
            source_passthrough: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
    /// Register the ABSOLUTE source path that backs a promised variant URL.
    /// Called alongside `set_pending` so passthrough parity is automatic:
    /// every Pending variant has an original the preview server can serve.
    pub fn set_source_passthrough(&self, output_url: String, source_abs: std::path::PathBuf) {
        let mut map = self.source_passthrough.write().unwrap();
        map.insert(output_url, source_abs);
    }
    /// Look up the absolute source path registered for `path` (an output URL).
    /// Mirrors `is_registered`'s tolerance for the leading-slash form so a
    /// request path (`/assets/x.webp`) matches a bare-keyed entry
    /// (`assets/x.webp`) and vice-versa.
    pub fn source_passthrough(&self, path: &str) -> Option<std::path::PathBuf> {
        let map = self.source_passthrough.read().unwrap();
        if let Some(p) = map.get(path) {
            return Some(p.clone());
        }
        let stripped = path.trim_start_matches('/');
        if let Some(p) = map.get(stripped) {
            return Some(p.clone());
        }
        let with_slash = format!("/{}", stripped);
        map.get(with_slash.as_str()).cloned()
    }
    pub fn set_pending(
        &self,
        path: String,
        dimensions: Option<(u32, u32)>,
        dominant_color: Option<String>,
    ) {
        let mut assets = self.assets.write().unwrap();
        assets.insert(
            path,
            AssetState::Pending(AssetPlaceholder {
                dimensions,
                dominant_color,
            }),
        );
    }
    /// Mark an asset Ready. Returns `true` when this was a real Pending→Ready
    /// transition (the asset was still being stood in for), `false` when it was
    /// already Ready (or absent). Callers that relay a placeholder-swap signal
    /// should gate on the return so they don't re-emit for an already-real
    /// asset on every unchanged rebuild.
    pub fn set_ready(&self, path: String) -> bool {
        let mut assets = self.assets.write().unwrap();
        let was_pending = matches!(assets.get(&path), Some(AssetState::Pending(_)));
        assets.insert(path, AssetState::Ready);
        was_pending
    }
    /// Mark an asset as terminally failed (encode error, canonical-link
    /// failure, or other unrecoverable condition). The preview server will
    /// surface the failure to the browser instead of standing in for the
    /// asset indefinitely.
    ///
    /// Pattern: Bazel's `FindMissingBlobs` invariant — a manifest-level "hit"
    /// must not stand in for a filesystem-level "present"
    /// (bazel.build/remote/caching). Failed is the negative form of the same
    /// rule: when materialization fails, the promise must be retracted, not
    /// silently swallowed.
    pub fn set_failed(&self, path: String, error: String) {
        let mut assets = self.assets.write().unwrap();
        assets.insert(path, AssetState::Failed(error));
    }
    pub fn get(&self, path: &str) -> Option<AssetState> {
        let assets = self.assets.read().unwrap();
        assets.get(path).cloned()
    }
    /// Snapshot of every registry key currently in the terminal `Failed`
    /// state. Used by the post-seal degrade-failed-variants pass
    /// to decide which `<source>` srcset candidates to drop from sealed HTML
    /// before publish.
    pub fn failed_keys(&self) -> std::collections::HashSet<String> {
        self.assets
            .read()
            .unwrap()
            .iter()
            .filter(|(_, s)| matches!(s, AssetState::Failed(_)))
            .map(|(k, _)| k.clone())
            .collect()
    }
    /// Snapshot of every registry key currently in the `Pending` state — this
    /// build's own promises not yet kept. Used by the publish-time promise
    /// gate (`link_audit::dead_links_among_promises`) to tell "a link into
    /// this build's own in-flight encode" apart from a pre-existing broken
    /// link, the one distinction a page-vs-manifest diff alone cannot see.
    pub fn pending_keys(&self) -> std::collections::HashSet<String> {
        self.assets
            .read()
            .unwrap()
            .iter()
            .filter(|(_, s)| matches!(s, AssetState::Pending(_)))
            .map(|(k, _)| k.clone())
            .collect()
    }

    pub fn clear(&self) {
        let mut assets = self.assets.write().unwrap();
        assets.clear();
        // Clear the passthrough map in lock-step: registry.clear() runs at
        // build start (blocking.rs), and a stale source path from a prior
        // build must never back a freshly-promised variant.
        let mut passthrough = self.source_passthrough.write().unwrap();
        passthrough.clear();
    }

    /// Returns true if `request_path` is known to the registry, regardless
    /// of state. Used by the editor to gate hover-preview / unresolved styling.
    /// Checks both the bare path (`img/hero.webp`) and the slash-prefixed form
    /// (`/img/hero.webp`) because call sites differ in how they store keys.
    pub fn is_registered(&self, request_path: &str) -> bool {
        let assets = self.assets.read().unwrap();
        let stripped = request_path.trim_start_matches('/');
        // Check exact form as given (handles slash-prefixed keys).
        if assets.contains_key(request_path) {
            return true;
        }
        // Check bare form (no leading slash — most production call sites).
        if assets.contains_key(stripped) {
            return true;
        }
        // Check slash-prefixed form in case the caller omitted the slash but
        // the key was stored with one.
        let with_slash = format!("/{}", stripped);
        assets.contains_key(with_slash.as_str())
    }

    /// Returns (source-path-stem → variant-kinds-registered) for all entries
    /// in Pending or Ready state.
    ///
    /// Variant identity comes from the URL extension (`.webp` = WebP variant,
    /// `.avif` = AVIF variant); the registry's internal data model has no
    /// per-variant typed field — it tracks URLs alongside their source asset
    /// under the same registry. The accessor folds variants by stem so the
    /// synthesizer can ask "does this source asset have a webp variant?"
    /// regardless of the source's own extension (jpg/png/gif/etc.).
    ///
    /// Failed entries are skipped (don't emit a `<source>` for
    /// a failed variant — `<picture>` does not recover from a chosen-source
    /// 404, HTML spec § update-the-source-set).
    ///
    /// Phase 0 Task F1 — feeds `AssetSnapshot.variants`. The accessor is
    /// keyed by **stem path** (extension stripped) per the
    /// [`moss_core::asset_snapshot`] module docs.
    pub fn iter_registered_variants(
        &self,
    ) -> std::collections::HashMap<std::path::PathBuf, moss_core::asset_snapshot::VariantKindSet>
    {
        use moss_core::asset_snapshot::VariantKindSet;
        use std::collections::HashMap;
        use std::path::PathBuf;

        let assets = self.assets.read().unwrap();
        let mut by_stem: HashMap<PathBuf, VariantKindSet> = HashMap::new();
        for (url, state) in assets.iter() {
            // Skip Failed entries.
            if matches!(state, AssetState::Failed(_)) {
                continue;
            }

            // A ladder is one directory named after the video, and its master
            // playlist is the only file in there that names the source. Match
            // it exactly: the rung and audio playlists are registered too
            // (never-404), but they are reached through the master, so one of
            // them arriving without it would not be a ladder to offer.
            if let Some(stem) = moss_core::asset_paths::hls_master_stem(url) {
                by_stem
                    .entry(moss_core::asset_snapshot::variant_key(stem))
                    .or_default()
                    .hls = true;
                continue;
            }

            let url_path = PathBuf::from(url);
            let ext = url_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            let stem_path = moss_core::asset_snapshot::path_strip_extension(&url_path);

            let entry = by_stem.entry(stem_path).or_default();
            match ext.as_str() {
                "webp" => entry.webp = true,
                "avif" => entry.avif = true,
                _ => {
                    // Source asset (jpg/png/gif/etc.) or unknown — keyed but
                    // no variant kinds marked. Ensures `has_webp_for_source`
                    // lookups for sources without variants return false (not
                    // None followed by some other miss path).
                }
            }
        }
        by_stem
    }
}

#[cfg(test)]
mod asset_registry_tests {
    use super::*;

    /// The bridge between a registered URL and the emitter's question "does
    /// this video have a ladder?". A dotted filename used to defeat the match
    /// here, which silently shipped seventeen files nothing linked to.
    #[test]
    fn a_ladder_is_recognised_by_its_master_alone_however_the_video_is_named() {
        let r = AssetRegistry::new();
        for name in ["clip", "2024.05.trip"] {
            for member in moss_core::asset_paths::hls_members(&moss_core::asset_paths::VIDEO_LADDER)
            {
                r.set_pending(format!("/v/{name}.hls/{member}"), None, None);
            }
        }
        // A rung playlist without its master: reached only through the master,
        // so on its own it is not a ladder to offer.
        r.set_pending("/v/lonely.hls/v0.m3u8".into(), None, None);

        let variants = r.iter_registered_variants();
        let has = |stem: &str| {
            variants
                .get(&std::path::PathBuf::from(stem))
                .is_some_and(|v| v.hls)
        };
        // Registered with a leading slash, keyed without one: `variant_key`
        // puts every stem in one form so the emitter — which probes with the
        // root-relative `src` a markdown embed carried — cannot miss a ladder
        // just because the two layers spell the same asset differently.
        assert!(has("v/clip"));
        assert!(has("v/2024.05.trip"), "a dotted video name is still a video name");
        assert!(!has("v/lonely"));
    }

    #[test]
    fn asset_registry_is_registered_basic() {
        let r = AssetRegistry::new();
        r.set_pending("/img/hero.webp".into(), None, None);
        assert!(r.is_registered("/img/hero.webp"));
        assert!(r.is_registered("img/hero.webp"));  // normalize leading slash
        assert!(!r.is_registered("/img/missing.webp"));
    }

    #[test]
    fn set_ready_reports_only_a_real_pending_to_ready_transition() {
        // The return value gates the placeholder-swap relay: re-emitting for an
        // already-Ready asset re-fetches and re-decodes every on-page image on
        // every unchanged rebuild (2026-07-02).
        let r = AssetRegistry::new();
        r.set_pending("img/hero.webp".into(), Some((800, 600)), Some("#aabbcc".into()));
        assert!(
            r.set_ready("img/hero.webp".into()),
            "Pending → Ready is a real transition"
        );
        assert!(
            !r.set_ready("img/hero.webp".into()),
            "Ready → Ready must not re-signal a swap"
        );
        assert!(
            !r.set_ready("img/static.webp".into()),
            "a never-promised asset was never standing in for anything"
        );
    }

    #[test]
    fn source_passthrough_round_trips_and_normalizes_leading_slash() {
        // Bug 1: the preview server serves the full original for a not-yet-
        // encoded variant. The registry maps the output URL → absolute source.
        let r = AssetRegistry::new();
        let src = std::path::PathBuf::from("/Users/x/site/assets/hero.jpg");
        r.set_source_passthrough("assets/hero.webp".into(), src.clone());

        // Bare form (as stored) resolves.
        assert_eq!(r.source_passthrough("assets/hero.webp"), Some(src.clone()));
        // Leading-slash request form (as the router hands it) also resolves.
        assert_eq!(r.source_passthrough("/assets/hero.webp"), Some(src.clone()));
        // An unregistered URL is None.
        assert_eq!(r.source_passthrough("assets/missing.webp"), None);
    }

    #[test]
    fn rung_urls_register_and_serve_like_any_variant() {
        // Responsive ladder rung URLs (photo.w800.webp) go through the same
        // promise model as any variant — set_pending → is_registered. Their
        // distinct stems (photo.w800) are harmless to the stem-keyed variant
        // accessor; pin the basic contract here.
        let r = AssetRegistry::new();
        r.set_pending("assets/photo.w800.webp".into(), Some((800, 480)), None);
        assert!(r.is_registered("assets/photo.w800.webp"));
    }

    #[test]
    fn clear_wipes_source_passthrough_too() {
        // registry.clear() runs at build start; a stale source path from a
        // prior build must never back a freshly-promised variant.
        let r = AssetRegistry::new();
        r.set_source_passthrough(
            "assets/hero.webp".into(),
            std::path::PathBuf::from("/tmp/hero.jpg"),
        );
        r.clear();
        assert_eq!(r.source_passthrough("assets/hero.webp"), None);
    }

    #[test]
    fn pending_keys_lists_only_pending_entries() {
        let r = AssetRegistry::new();
        assert!(r.pending_keys().is_empty());
        r.set_pending("videos/clip.mp4".into(), None, None);
        r.set_pending("assets/ok.webp".into(), None, None);
        r.set_ready("assets/ok.webp".into());
        r.set_pending("assets/bad.webp".into(), None, None);
        r.set_failed("assets/bad.webp".into(), "encode error".into());

        let pending = r.pending_keys();
        assert_eq!(pending.len(), 1);
        assert!(pending.contains("videos/clip.mp4"));
        assert!(!pending.contains("assets/ok.webp"), "Ready is not Pending");
        assert!(
            !pending.contains("assets/bad.webp"),
            "a permanently failed encode must stop being a pending promise"
        );
    }

    #[test]
    fn failed_keys_lists_only_failed_entries() {
        let r = AssetRegistry::new();
        assert!(r.failed_keys().is_empty());
        r.set_pending("assets/ok.webp".into(), None, None);
        r.set_pending("assets/waiting.webp".into(), None, None);
        r.set_ready("assets/ok.webp".into());
        r.set_failed("assets/bad.webp".into(), "encode error".into());
        r.set_failed("assets/bad.w800.webp".into(), "encode error".into());

        let failed = r.failed_keys();
        assert_eq!(failed.len(), 2);
        assert!(failed.contains("assets/bad.webp"));
        assert!(failed.contains("assets/bad.w800.webp"));
        assert!(!failed.contains("assets/ok.webp"), "Ready is not Failed");
        assert!(!failed.contains("assets/waiting.webp"), "Pending is not Failed");
    }
}
