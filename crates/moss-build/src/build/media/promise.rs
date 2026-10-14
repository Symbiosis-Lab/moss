//! Every preview image URL is derived once, before first paint, for both the
//! transient registry and the generation's durable original identity.
//! Missing encodes show the full original while available; cloud-only bytes
//! receive a retryable response until foreground materialization completes.
//!
//! The other half of a promise is what a build owes once media settles: the
//! thumbnail, poster and ladder a page could not name at render time. A page
//! names them from what the render could see, and the render runs before the
//! encode, so a build that settles one has to render once more. A watch loop does
//! that by enqueueing a rebuild; a one-shot build (`moss build`, `moss deploy`) has
//! no worker, so `run_pipeline` runs a second pass when
//! [`trigger_media_settle_rerender`] says yes. Both ask it, so there is one answer
//! to "does this build owe a re-render".

use std::collections::HashMap;
use std::path::PathBuf;

use crate::build::PluginMode;
use crate::build::media::image::{ImageConversionItem, SkipReason};
use crate::types::assets::{AssetRegistry, Failure};
use crate::types::content::ProjectStructure;

/// Register every collected item's variant URLs with `registry` (when the
/// build has one) and return the items the encoder should run — the ones
/// whose bytes are here and whose verdict is "encode".
///
/// `rung_collisions` is `rungs::rung_collision_map`, built once per build and
/// also threaded to the encode worker; `vault_root` resolves each source to
/// the absolute file the preview serves through the passthrough.
pub(crate) fn promise_image_variants(
    registry: Option<&AssetRegistry>,
    manifest: &mut crate::build::manifest::PendingManifest,
    items: Vec<ImageConversionItem>,
    project_structure: &ProjectStructure,
    dir_overrides: &HashMap<String, String>,
    rung_collisions: &HashMap<String, PathBuf>,
    vault_root: &str,
) -> Vec<ImageConversionItem> {
    for item in &items {
        // Map source path through dir_overrides, then derive
        // the .webp URL. This ensures the registry key matches
        // exactly what the synthesizer emits as
        // <source srcset="./assets/X.webp">. The preview
        // server's normalized_path strips the leading slash;
        // the synthesizer's leading `./` does not survive the
        // browser's URL resolution. So both sides settle on
        // `assets/X.webp` as the registry key.
        let mapped = crate::build::scan::page_map::resolve_path_with_overrides(
            &item.source_path.to_string_lossy(),
            dir_overrides,
        );
        let webp_path = moss_core::asset_paths::to_webp(&mapped);

        // Look up the source's MediaMetadata to carry
        // dimensions / dominant_color into the registry. NOT the
        // LQIP: a blur is a published-site technique for a visitor
        // on a network, and in the preview it would be a stand-in
        // the author could mistake for their own photo. The preview
        // serves the full original instead — see
        // `set_source_passthrough` below.
        let img_meta = project_structure
            .image_files
            .iter()
            .find(|m| std::path::Path::new(&m.path) == item.source_path);

        let dimensions = img_meta.and_then(|m| m.dimensions);
        let dominant_color = img_meta.and_then(|m| m.dominant_color.clone());

        // The variant URLs the synthesizer has already emitted for this
        // source: the base webp and, for a ladder source, its rungs. The
        // registry key must EXACTLY equal the emitted URL: same `mapped`
        // path + same to_webp_rung + same ladder_rungs(w, h, false) over
        // the same scan dims. (ladder_rungs is height-aware — pass BOTH
        // scan dims, NEVER `(w, w, false)`.) The `false` literal, the
        // animated-flag proof, and the EXIF-oriented png/webp dims
        // agreement are documented canonically on asset_paths::ladder_rungs.
        // `is_ladder_source_ext` covers png/jpg/jpeg AND webp, so a webp
        // source registers its rung URLs here too (its base URL is the
        // source itself: to_webp(webp) == webp).
        let mut variants = vec![webp_path];
        if moss_core::asset_paths::is_ladder_source_ext(&item.ext) {
            if let Some((w, h)) = dimensions {
                for rung in moss_core::asset_paths::ladder_rungs(w, h, false) {
                    let rung_url = moss_core::asset_paths::to_webp_rung(&mapped, *rung);
                    if let Some(user_file) = rung_collisions.get(&rung_url) {
                        log::warn!(
                            "'{}' is named like a generated responsive variant of '{}'; keeping your file — rename '{}' to avoid the reserved '.w{}.webp' naming pattern if you meant it to be a different image",
                            user_file.display(),
                            item.source_path.display(),
                            user_file.display(),
                            rung
                        );
                        // Deferred-window guard: a small colliding
                        // file may never register its own promise
                        // (it converts to photo.w800.webp only if
                        // IT is a convertible raster), yet the
                        // emitted srcset references this URL before
                        // asset copy lands the file. Passthrough
                        // the USER'S file so the preview serves its
                        // real bytes in that window instead of 404.
                        if let Ok(relative) = user_file.strip_prefix(vault_root) {
                            manifest.register_preview_original(rung_url.clone(), relative.to_string_lossy().into_owned());
                        }
                        if let Some(registry) = registry { registry.set_source_passthrough(rung_url, user_file.clone()); }
                        continue;
                    }
                    variants.push(rung_url);
                }
            }
        }

        // Source passthrough: the original's own resolved URL (the inner
        // <img>) and, while the encoder is still working, every variant
        // URL map to the ABSOLUTE source file so the preview serves the
        // FULL ORIGINAL bytes (sharp). Resolve the source the same way
        // run_image_conversion does (project_root.join(source_path)).
        let source_abs = std::path::Path::new(vault_root).join(&item.source_path);
        manifest.register_preview_original(mapped.clone(), item.source_path.to_string_lossy().into_owned());
        if let Some(registry) = registry { registry.set_source_passthrough(mapped.clone(), source_abs.clone()); }

        // A variant this build will never encode settles `Failed`
        // instead, so the post-seal degrade pass drops its `<source>`
        // and the page keeps the original `<img>` — see
        // `ImageConversionItem::skip`.
        let never = item.skip.as_ref().and_then(SkipReason::failure_message);
        for url in variants {
            manifest.register_preview_original(url.clone(), item.source_path.to_string_lossy().into_owned());
            if let Some(registry) = registry { match never {
                Some(why) => registry.set_failed(url, Failure::skip(why)),
                None => {
                    registry.set_source_passthrough(url.clone(), source_abs.clone());
                    registry.set_pending(url, dimensions, dominant_color.clone());
                }
            }}
        }
    }
    // Registration is over ALL collected items; the ENCODER gets only the
    // ones it can encode.
    //
    // An item carrying a `skip` verdict has just had its variant URLs
    // settled above — Pending with a passthrough for a source still in the
    // cloud, Failed for one that will never encode — which is the whole
    // reason it was collected (`ImageConversionItem::skip`). Handing it to
    // the encoder as well would read the source, take an `EDEADLK`, and
    // `set_failed` the variant, replacing "still downloading" with a
    // "⚠ asset failed" tile for a file that is on its way down. Dropping
    // it here also keeps the background progress total honest: it counts
    // work this build can do.
    items.into_iter().filter(|i| i.skip.is_none()).collect()
}

/// Which pass of a one-shot build this is, and so what it may repeat.
///
/// The second pass re-renders pages over output the first pass already
/// produced. It must not repeat what belongs to the build as a whole: the
/// plugins' process hooks (minutes of external fetching, meant for the initial
/// build only), the link-preview prewarm, the agent-guidance sync, and the
/// completion line and problem reports the user has already seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pass {
    pub(crate) plugins: PluginMode,
    pub(crate) repeat: bool,
}

impl Pass {
    pub(crate) fn first(plugins: PluginMode) -> Self {
        Pass { plugins, repeat: false }
    }

    /// `Skip` stays `Skip`; every mode that would run hooks runs only the
    /// slots, as a watch rebuild does.
    pub(crate) fn second(self) -> Self {
        let plugins = match self.plugins {
            PluginMode::Skip => PluginMode::Skip,
            PluginMode::Blocking | PluginMode::NonBlocking | PluginMode::SlotsOnly => PluginMode::SlotsOnly,
        };
        Pass { plugins, repeat: true }
    }
}

/// What one pass reports to [`drive_passes`].
pub(crate) struct PassOutcome {
    pub(crate) message: String,
    /// Media settled that this pass's render could not name (`media::promise`).
    pub(crate) owed: bool,
    pub(crate) is_empty: bool,
    /// The preview port when the pass ended, for the completion event.
    pub(crate) port: Option<u16>,
}

/// Run one pass, and a second only for a one-shot build whose first pass
/// settled media the first render could not name.
///
/// A page links a video's ladder and a hero's full-size poster only when the
/// file is already on disk at render time, and the render runs before the
/// encode that makes it. A long-lived host heals this by re-rendering when the
/// media settles (`media::promise`); `moss build` and `moss deploy` exit instead,
/// so without this pass the first publish of a site shipped the progressive
/// file and the small still. The second pass is an ordinary incremental build:
/// it registers what is now on disk (`register_existing_ladders`/`posters`),
/// finds nothing new to encode, and never asks for a third. Only the inline
/// seal arm reports the request, so the app and `--watch` builds never loop.
///
/// The second pass runs quiet (`cli_output::quiet`): progress lines are
/// dropped, and a problem line is dropped only if the first pass already
/// printed the same text, so each problem is reported once and one that only
/// the second pass can see is still shown and counted. Matching on the printed
/// text avoids teaching each of the build's many problem reports to remember
/// whether it has fired. The returned flag says the
/// completion line was held back and is still owed to the caller.
///
/// If the second pass fails, the first is a complete, sealed site, so the build
/// does not fail: it reports one problem (which `--strict` fails on) and
/// returns the first pass's result.
pub(crate) async fn drive_passes<F, Fut>(plugins: PluginMode, mut run: F) -> Result<(PassOutcome, bool), String>
where
    F: FnMut(Pass) -> Fut,
    Fut: std::future::Future<Output = Result<PassOutcome, String>>,
{
    let first_pass = Pass::first(plugins);
    let first = run(first_pass).await?;
    if !first.owed {
        return Ok((first, false));
    }
    log::info!("media settled after the first render; rendering the pages once more");
    let second = {
        let _quiet = crate::build::cli_output::quiet();
        run(first_pass.second()).await
    };
    match second {
        Ok(second) => Ok((second, true)),
        Err(e) => {
            crate::build::cli_output::log_warn_problem!(
                "The pages could not be rendered again after the videos were encoded ({e}); \
                 this build links the plain video files and the small thumbnails."
            );
            Ok((first, true))
        }
    }
}

/// Decide whether the media that just settled owes `folder_path` another
/// render, and enqueue it on the folder's rebuild worker when there is one.
///
/// A watch loop (a worker is registered) re-renders when `settled` contains an
/// asset a placeholder-consuming page can never self-heal from on its own:
///
/// - **Every video poster** (`asset_type == "thumbnail"`).
///   `color_extract::resolve_card_color`'s video branch and
///   `media::cover::render_cover_html`'s `<img class="cover-thumb">` both
///   read straight from the OUTPUT thumb file with no source-file fallback,
///   and no swap signal reaches the client either: `iframe-bridge.ts`'s
///   `"thumbnail"` case only rewrites a `<video data-thumb-src>` element's
///   `poster` attribute, and the grid-card markup emits neither that
///   attribute nor any handle for the colour band (a static inline style).
///   The colour and the thumbnail both resolve correctly on a fresh render —
///   the same output-file lookup every render already makes.
///
/// - **A brand-new image** (`asset_type == "image"` AND `path` was absent
///   from the PREVIOUS sealed manifest). `resolve_card_color`'s image branch
///   decodes the SOURCE file on a cache miss, so a page bakes an empty
///   `data-cover-color` only when that decode fails at render time (a
///   dataless/not-yet-materialized source) — a client swap can't fix it for
///   the same reason as the poster case: no colour payload ever reaches the
///   browser. A RE-encode of an image this folder has settled before is
///   excluded: any image with a PRIOR successful settle already enriched the
///   persisted stat-cache entry `extract_media_metadata_cached` reads
///   (scan.rs), so its color was already available to every render since —
///   re-triggering on every such re-encode would Full-rebuild a large site
///   on ordinary content-image churn the client already swaps live via
///   `AssetsSettled`/`AssetReady`. This is the coarser of the two checks:
///   without threading a live "which cover keys baked empty" set from the
///   render phase through to here (a wider change across every
///   `resolve_card_color` call site — grid_card.rs, grid_cells.rs,
///   folder_embed.rs, child_summary.rs — for a condition this rare), "newly
///   added" is the closest correlate this function's two existing inputs
///   (the settle diff and the previous manifest) can compute. A share card
///   under `_moss/og/` never counts: it is drawn by the render of the page it
///   belongs to, from that page's own text, so no page baked anything from it
///   before it existed. Its path is a content hash, so every `title:` edit
///   produces a "new" one, and counting it cost a full re-render per edit.
///
/// Returns whether the calling build itself owes a second pass. With a watcher
/// the re-render is enqueued and the answer is always `false`. With none
/// (`ops::watch::worker::get` returns `None` for a deploy, CLI build, or plugin
/// install) nothing is enqueued, the narrower [`one_shot_render_owed`] decides,
/// and the one-shot caller acts on the return value instead. Self-limiting: the follow-up build
/// reuses the SAME rebuild path a file edit already drives
/// (`ops::watch::worker`), and that build's own `diff_settled_assets`
/// compares against the manifest THIS build just sealed — an unchanged
/// re-encode reports nothing new, so it triggers no further rebuild.
///
/// `previous` is `None` when the last manifest could not be read: every variant
/// then diffs as settled, and a rebuild enqueued on that is a rebuild for
/// nothing — under active cloud management, one per seal.
pub(crate) fn trigger_media_settle_rerender(
    folder_path: &str,
    settled: &[crate::build::progress::SettledAsset],
    previous: Option<&crate::types::content::SiteHashes>,
) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    let Some(worker) = crate::ops::watch::worker::get(folder_path) else {
        return one_shot_render_owed(settled, previous);
    };
    let needs_rerender = settled.iter().any(|a| {
        a.asset_type == "thumbnail"
            || (a.asset_type == "image"
                && !a.path.starts_with(crate::build::served_path::OG_CARD_PREFIX)
                && !previous.files.contains_key(&a.path))
    });
    if !needs_rerender {
        return false;
    }
    log::info!(
        "media settle: a video poster or a brand-new image landed for '{}' — \
         enqueueing a full re-render so grid-card covers pick up the color band and thumbnail",
        folder_path
    );
    worker.enqueue(crate::ops::watch::worker::RebuildRequest::full());
    // The worker owns the re-render; a build must not also run a second pass.
    false
}

/// A one-shot build (`moss build`, `moss deploy`) renders its pages once more
/// only for what a page could not name the first time round: a video still or a
/// streaming master that the previous manifest did not hold. Those are the files
/// a render links only if they are already on disk. A re-encode of one already
/// published changes no markup, and an image changes none either (see the
/// cold-image test in `first_build_media_test`), so neither earns a second pass.
fn one_shot_render_owed(
    settled: &[crate::build::progress::SettledAsset],
    previous: &crate::types::content::SiteHashes,
) -> bool {
    settled.iter().any(|a| {
        let names_a_video_output = a.asset_type == "thumbnail" || moss_core::asset_paths::hls_master_stem(&a.path).is_some();
        names_a_video_output && !previous.files.contains_key(&a.path)
    })
}

#[cfg(test)]
#[path = "promise_tests.rs"]
mod tests;

#[cfg(test)]
mod media_settle_rerender_tests {
    use super::trigger_media_settle_rerender;
    use crate::build::progress::SettledAsset;
    use crate::ops::watch::worker;
    use crate::types::content::{file_entry, SiteHashes};

    fn thumbnail(path: &str) -> SettledAsset {
        SettledAsset { path: path.to_string(), asset_type: "thumbnail".to_string() }
    }

    fn image(path: &str) -> SettledAsset {
        SettledAsset { path: path.to_string(), asset_type: "image".to_string() }
    }

    fn video(path: &str) -> SettledAsset {
        SettledAsset { path: path.to_string(), asset_type: "video".to_string() }
    }

    /// A previous manifest that already knows `paths` — the "this image has
    /// settled before" state `trigger_media_settle_rerender`'s image branch
    /// reads to tell a re-encode from a brand-new arrival.
    fn previous_knowing(paths: &[&str]) -> SiteHashes {
        let mut h = SiteHashes::default();
        for p in paths {
            h.files.insert(p.to_string(), file_entry("prior-hash"));
        }
        h
    }

    /// A settled video poster is exactly the case `resolve_card_color`'s video
    /// branch and the grid-card `<img class="cover-thumb">` cannot self-heal
    /// without a fresh render (see `trigger_media_settle_rerender`'s doc) — so
    /// it must enqueue on the SAME slot a file edit would.
    #[test]
    fn a_settled_thumbnail_enqueues_a_full_rebuild() {
        let folder = "/tmp/media-settle-rerender-test-thumbnail";
        let handle = worker::register(folder);
        assert!(!handle.slot_occupied(), "clean start");

        let second_pass_owed = trigger_media_settle_rerender(
            folder,
            &[thumbnail("videos/clip.thumb.jpg")],
            Some(&SiteHashes::default()),
        );

        assert!(
            handle.slot_occupied(),
            "a settled thumbnail must enqueue a follow-up rebuild"
        );
        assert!(!second_pass_owed, "the worker owns the re-render, so the build must not also run a second pass");
        worker::deregister(folder, &handle);
    }

    /// A re-encode of an image this folder has settled before is excluded —
    /// its color was already available to every render since the FIRST
    /// settle (the persisted stat-cache entry `resolve_card_color`'s image
    /// branch reads on a cache hit), so re-triggering here would pay a
    /// full-site reload for ordinary content-image churn the client already
    /// swaps live via `AssetsSettled`/`AssetReady`.
    #[test]
    fn a_reencoded_image_previously_settled_does_not_enqueue() {
        let folder = "/tmp/media-settle-rerender-test-reencoded-image";
        let handle = worker::register(folder);

        trigger_media_settle_rerender(
            folder,
            &[image("assets/pic.webp"), video("videos/clip.mp4")],
            Some(&previous_knowing(&["assets/pic.webp"])),
        );

        assert!(
            !handle.slot_occupied(),
            "a re-encode of an already-known image, and a plain video settle, must not trigger a full re-render"
        );
        worker::deregister(folder, &handle);
    }

    /// A brand-new image (absent from the previous manifest) IS the case
    /// `trigger_media_settle_rerender`'s doc names: its source may have been
    /// unreadable (dataless) at the render that first referenced it, baking
    /// an empty `data-cover-color` with no client-side fix. Must enqueue.
    #[test]
    fn a_brand_new_image_settle_enqueues_a_full_rebuild() {
        let folder = "/tmp/media-settle-rerender-test-new-image";
        let handle = worker::register(folder);

        trigger_media_settle_rerender(
            folder,
            &[image("assets/new-cover.webp")],
            Some(&SiteHashes::default()), // previous manifest has never seen this path
        );

        assert!(
            handle.slot_occupied(),
            "a brand-new image settle must enqueue a follow-up rebuild"
        );
        worker::deregister(folder, &handle);
    }

    /// A share card is drawn by its own page's render, so a new one (every
    /// `title:` edit makes one, its path being a content hash) is never media
    /// another page baked a placeholder from.
    #[test]
    fn a_new_share_card_does_not_enqueue() {
        let folder = "/tmp/media-settle-rerender-test-og-card";
        let handle = worker::register(folder);

        trigger_media_settle_rerender(
            folder,
            &[image("_moss/og/0123456789abcdef.png")],
            Some(&SiteHashes::default()),
        );

        assert!(!handle.slot_occupied(), "a new share card must not trigger a full re-render");
        worker::deregister(folder, &handle);
    }

    /// An unreadable previous manifest is not an empty one. Diffed against
    /// empty, every poster reads as newly settled, and a rebuild enqueued on
    /// that is one rebuild per seal for as long as the file stays unreadable.
    #[test]
    fn an_unreadable_previous_manifest_enqueues_nothing() {
        let folder = "/tmp/media-settle-rerender-test-unreadable-previous";
        let handle = worker::register(folder);

        trigger_media_settle_rerender(folder, &[thumbnail("videos/clip.thumb.jpg")], None);

        assert!(
            !handle.slot_occupied(),
            "a settle diffed against a manifest nobody could read must not enqueue"
        );
        worker::deregister(folder, &handle);
    }

    /// Deploy, `moss build`, and plugin-install builds never register a
    /// worker (no live preview to refresh) — the function must not panic or
    /// otherwise assume one exists.
    #[test]
    fn no_registered_worker_is_a_noop() {
        let folder = "/tmp/media-settle-rerender-test-no-worker";
        assert!(worker::get(folder).is_none(), "precondition: nothing registered");
        trigger_media_settle_rerender(
            folder,
            &[thumbnail("videos/clip.thumb.jpg")],
            Some(&SiteHashes::default()),
        );
        assert!(worker::get(folder).is_none(), "still nothing registered");
    }

    /// With no watcher to enqueue on, the one-shot caller still has to learn
    /// that a re-render is owed: the answer is the return value, not the queue.
    #[test]
    fn a_build_with_no_worker_is_still_told_a_rerender_is_owed() {
        let folder = "/tmp/media-settle-rerender-test-owed-without-worker";
        assert!(trigger_media_settle_rerender(
            folder,
            &[thumbnail("videos/clip.poster.jpg")],
            Some(&SiteHashes::default()),
        ));
        assert!(!trigger_media_settle_rerender(folder, &[video("videos/clip.mp4")], Some(&SiteHashes::default())));
        assert!(!trigger_media_settle_rerender(folder, &[thumbnail("videos/clip.thumb.jpg")], None));
    }

    /// A site first built by a version that cut thumbnails but no ladders: the
    /// ladder is the only new output, and the pages still have to link it.
    #[test]
    fn a_one_shot_build_that_only_gained_a_ladder_owes_a_second_pass() {
        let folder = "/tmp/media-settle-rerender-test-ladder-only";
        let had_thumb = previous_knowing(&["videos/clip.thumb.jpg", "videos/clip.mp4"]);
        assert!(trigger_media_settle_rerender(folder, &[video("videos/clip.hls/master.m3u8")], Some(&had_thumb)));
        // The rung files are not the gate; a ladder is announced by its master.
        assert!(!trigger_media_settle_rerender(folder, &[video("videos/clip.hls/v0.m3u8")], Some(&had_thumb)));
        // A master the previous manifest already held was linkable by the render.
        let had_ladder = previous_knowing(&["videos/clip.hls/master.m3u8"]);
        assert!(!trigger_media_settle_rerender(folder, &[video("videos/clip.hls/master.m3u8")], Some(&had_ladder)));
    }

    /// A one-shot build's pages do not change for a new image, so a site of
    /// pictures builds once. (The watch loop still re-renders for one.)
    #[test]
    fn a_one_shot_build_does_not_owe_a_second_pass_for_an_image() {
        let folder = "/tmp/media-settle-rerender-test-image-only";
        assert!(!trigger_media_settle_rerender(folder, &[image("assets/new.webp")], Some(&SiteHashes::default())));
    }

    /// A thumbnail the previous build already published is a re-encode, not
    /// something a page could not name.
    #[test]
    fn a_one_shot_build_does_not_owe_a_second_pass_for_a_re_encoded_still() {
        let folder = "/tmp/media-settle-rerender-test-reencoded-still";
        let known = previous_knowing(&["videos/clip.thumb.jpg"]);
        assert!(!trigger_media_settle_rerender(folder, &[thumbnail("videos/clip.thumb.jpg")], Some(&known)));
    }
}

#[cfg(test)]
mod pass_tests {
    use super::*;
    use std::cell::RefCell;

    fn outcome(owed: bool) -> Result<PassOutcome, String> {
        Ok(PassOutcome { message: format!("owed={owed}"), owed, is_empty: false, port: None })
    }

    /// Drive `drive_passes` with a stub whose k-th pass answers `answers[k]`,
    /// returning the passes it was asked for and the result.
    fn drive(
        plugins: PluginMode,
        answers: Vec<Result<PassOutcome, String>>,
    ) -> (Vec<Pass>, Result<(PassOutcome, bool), String>) {
        let seen = RefCell::new(Vec::new());
        let answers = RefCell::new(answers.into_iter());
        let result = futures::executor::block_on(drive_passes(plugins, |pass| {
            seen.borrow_mut().push(pass);
            let next = answers.borrow_mut().next().expect("no third pass was expected");
            async move { next }
        }));
        (seen.into_inner(), result)
    }

    #[test]
    fn a_build_that_owes_nothing_runs_one_pass() {
        let (seen, result) = drive(PluginMode::Blocking, vec![outcome(false)]);
        assert_eq!(seen, vec![Pass::first(PluginMode::Blocking)]);
        assert!(!result.unwrap().1, "nothing was held back");
    }

    #[test]
    fn a_build_that_owes_a_render_runs_exactly_a_second_pass() {
        // The second pass also reporting `owed` must not start a third.
        let (seen, result) = drive(PluginMode::Blocking, vec![outcome(true), outcome(true)]);
        assert_eq!(seen.len(), 2);
        assert!(!seen[0].repeat && seen[1].repeat);
        let (out, held) = result.unwrap();
        assert!(held, "the completion line is owed after the last pass");
        assert_eq!(out.message, "owed=true");
    }

    #[test]
    fn a_failed_first_pass_never_starts_a_second() {
        let (seen, result) = drive(PluginMode::Blocking, vec![Err("scan failed".into())]);
        assert_eq!(seen.len(), 1);
        assert_eq!(result.err().as_deref(), Some("scan failed"));
    }

    #[test]
    fn a_failed_second_pass_keeps_the_sealed_first_and_reports_one_problem() {
        let _ = crate::build::cli_output::take_cli_problems();
        let (_, result) = drive(PluginMode::Blocking, vec![outcome(true), Err("disk full".into())]);
        let (out, held) = result.expect("the first pass is a valid site");
        assert_eq!(out.message, "owed=true", "the first pass's result is returned");
        assert!(held);
        assert_eq!(crate::build::cli_output::take_cli_problems(), 1, "one problem, so --strict fails");
    }

    /// Print one problem line the way `log_warn_problem!` does: note, then the line.
    fn report(text: &str) {
        crate::build::cli_output::note_cli_problem();
        crate::build::cli_output::eprint_status_line(format_args!("{text}"));
    }

    #[test]
    fn a_problem_is_reported_once_and_one_only_the_second_pass_sees_still_counts() {
        let _ = crate::build::cli_output::take_cli_problems();
        let (_, result) = futures::executor::block_on(async {
            let r = drive_passes(PluginMode::Blocking, |pass| async move {
                report("broken link A");
                if pass.repeat {
                    report("a streaming file is missing a part");
                }
                outcome(true)
            })
            .await;
            ((), r)
        });
        assert!(result.is_ok());
        assert_eq!(
            crate::build::cli_output::take_cli_problems(),
            2,
            "A once (not once per pass), plus the problem only the second pass saw"
        );
    }

    /// Hooks fetch external data and belong to the initial build; the second
    /// pass runs the slots only, and a build that skipped plugins still does.
    #[test]
    fn the_second_pass_never_runs_process_hooks() {
        for (first, second) in [
            (PluginMode::Blocking, PluginMode::SlotsOnly),
            (PluginMode::NonBlocking, PluginMode::SlotsOnly),
            (PluginMode::SlotsOnly, PluginMode::SlotsOnly),
            (PluginMode::Skip, PluginMode::Skip),
        ] {
            let (seen, _) = drive(first, vec![outcome(true), outcome(false)]);
            assert_eq!(seen[0].plugins, first);
            assert_eq!(seen[1].plugins, second, "second pass of {first:?}");
        }
    }
}
