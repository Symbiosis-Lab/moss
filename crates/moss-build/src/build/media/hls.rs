//! HLS ladder muxing: one ffmpeg invocation that writes every rung.
//!
//! One invocation and one decode. The source is decoded once and fanned out
//! with `split`, so six rungs cost far less than six encodes — measured on a
//! 10 s 720p clip against a single hardened progressive encode of the same
//! source: **1.65x wall time, 1.32x CPU, 2.16x storage**. (The plan carried
//! 2.1x / 2.6x / 2.4x, measured with five muxed rungs before the audio split;
//! CPU came down because the progressive path is two-pass and decodes twice,
//! while the ladder is single-pass and decodes once.)
//!
//! # Why single-pass here when the progressive encode is two-pass
//!
//! A rung's job is to be *predictable*, not to be the smallest file at a given
//! quality. `BANDWIDTH` in the master playlist is a promise the player makes
//! switching decisions against, and VBV-capped single-pass (`-b:v` = `-maxrate`,
//! `-bufsize` at 2x) delivers a bounded sustained rate that the declaration can
//! be honest about. Capped CRF / QVBR spends fewer bytes on simple content, but
//! its peak is not known until after the encode, so `BANDWIDTH` would have to be
//! measured and back-patched — and a rung that undershoots its declaration is a
//! rung the player declines to use on a link that could have carried it. Fixed
//! bitrate per rung, stated as a choice.
//!
//! # Segment alignment across mixed frame rates
//!
//! Keyframe cadence is expressed in SECONDS, not frames, and this is
//! load-bearing. `-g 60` means 60 *frames*: two seconds at 30 fps but four at
//! 15 fps, and the bottom rung is 15 fps. Measured with a flat `-g 60`, the
//! bottom rung segmented at 8.0s/2.07s while every other rung segmented at
//! 6.0s/4.03s — boundaries that do not line up, which is what a player needs
//! to switch rungs cleanly. With `-g` set per rung to `KEYFRAME_SECONDS * fps`
//! every rung segments at 6.000s.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use moss_core::asset_paths::{
    audio_groups, hls_members, video_ladder_fingerprint, video_ladder_rungs,
    video_ladder_rungs_by_count, AudioGroup, VideoRung, HLS_MASTER_NAME,
};

use crate::build::cache::{TransformCache, TransformEntry};
use crate::build::media::ffmpeg::{
    spawn_ffmpeg_streaming, FFmpegManager, SourceVideo, VideoCompressionConfig,
};
use crate::types::runtime::ChildProcessRegistry;

/// Target segment length. Six seconds is the common VOD choice: long enough
/// that per-segment overhead is small, short enough that a player switching
/// rungs commits to only six seconds of the wrong one.
const SEGMENT_SECONDS: u32 = 6;

/// Keyframe cadence, in seconds of wall time. A segment can only start on a
/// keyframe, so this divides `SEGMENT_SECONDS` evenly.
const KEYFRAME_SECONDS: u32 = 2;

/// The audio settings for a group: (kbps, channels). Read off the first rung
/// carrying it, so the ladder table stays the only statement of policy.
fn audio_settings(rungs: &[VideoRung], group: AudioGroup) -> (u32, u32) {
    rungs
        .iter()
        .find(|r| r.audio_group() == group)
        .map(|r| (r.audio_kbps, r.audio_channels))
        .unwrap_or((32, 1))
}

/// Build the full ffmpeg argument list for an HLS ladder encode.
///
/// Pure so tests can assert the command line without spawning ffmpeg.
/// `out_dir` is the ladder's own directory, e.g. `/site/assets/clip.hls`;
/// every name inside it is constant, which is what makes a cached ladder
/// relocatable — see [`moss_core::asset_paths::to_hls_dir`].
pub fn build_hls_args(
    input: &str,
    out_dir: &str,
    rungs: &[VideoRung],
    source_fps: f64,
    has_audio: bool,
    preset: &str,
    encode_threads: u32,
) -> Vec<String> {
    let n = rungs.len();

    // One decode, fanned out. Frame rate is only ever reduced: forcing 30 on a
    // 24 fps source would duplicate frames and spend bitrate on them.
    let mut filter = format!("[0:v]split={n}");
    for i in 0..n {
        filter.push_str(&format!("[s{i}]"));
    }
    filter.push(';');
    for (i, rung) in rungs.iter().enumerate() {
        filter.push_str(&format!("[s{i}]scale={}:{}", rung.width, rung.height));
        if source_fps > rung.fps as f64 {
            filter.push_str(&format!(",fps={}", rung.fps));
        }
        filter.push_str(&format!("[v{i}];"));
    }
    filter.pop();

    let mut args: Vec<String> = vec![
        "-i".into(),
        input.into(),
        "-filter_complex".into(),
        filter,
        "-threads".into(),
        encode_threads.to_string(),
    ];

    let mut var_map: Vec<String> = Vec::new();
    for (i, rung) in rungs.iter().enumerate() {
        let gop = (KEYFRAME_SECONDS * rung.fps).to_string();
        args.extend([
            "-map".into(),
            format!("[v{i}]"),
            format!("-c:v:{i}"),
            "libx264".into(),
            format!("-b:v:{i}"),
            format!("{}k", rung.video_kbps),
            format!("-maxrate:v:{i}"),
            format!("{}k", rung.video_kbps),
            format!("-bufsize:v:{i}"),
            format!("{}k", rung.video_kbps.saturating_mul(2)),
            format!("-g:v:{i}"),
            gop.clone(),
            format!("-keyint_min:v:{i}"),
            gop,
            // Without this, x264 inserts a keyframe at any hard cut, which
            // lands segment boundaries at different times on different rungs.
            format!("-sc_threshold:v:{i}"),
            "0".into(),
            format!("-pix_fmt:v:{i}"),
            "yuv420p".into(),
            format!("-profile:v:{i}"),
            "high".into(),
            format!("-preset:v:{i}"),
            preset.into(),
        ]);
        // A silent source declares no group: naming one that no
        // `#EXT-X-MEDIA` defines leaves a master playlist a player cannot
        // resolve.
        var_map.push(if has_audio {
            format!("v:{i},agroup:{},name:v{i}", rung.audio_group().as_str())
        } else {
            format!("v:{i},name:v{i}")
        });
    }

    if has_audio {
        for (j, group) in audio_groups(rungs).into_iter().enumerate() {
            let (kbps, channels) = audio_settings(rungs, group);
            args.extend([
                "-map".into(),
                "a:0".into(),
                format!("-c:a:{j}"),
                "aac".into(),
                format!("-b:a:{j}"),
                format!("{kbps}k"),
                format!("-ac:a:{j}"),
                channels.to_string(),
            ]);
            // `default:yes` on each group: a group with no default leaves the
            // player without a rendition to pick within it.
            var_map.push(format!(
                "a:{j},agroup:{},name:{},default:yes",
                group.as_str(),
                group.as_str()
            ));
        }
    }

    args.extend([
        "-var_stream_map".into(),
        var_map.join(" "),
        "-f".into(),
        "hls".into(),
        "-hls_time".into(),
        SEGMENT_SECONDS.to_string(),
        "-hls_playlist_type".into(),
        "vod".into(),
        "-hls_segment_type".into(),
        "fmp4".into(),
        // One file per rung, addressed by `#EXT-X-BYTERANGE`, with
        // `#EXT-X-MAP` byte-ranging the init segment out of the same file.
        // 6 rungs stay 6 files instead of dozens.
        "-hls_flags".into(),
        "single_file".into(),
        "-master_pl_name".into(),
        HLS_MASTER_NAME.into(),
        "-hls_segment_filename".into(),
        format!("{out_dir}/%v.m4s"),
        "-y".into(),
        format!("{out_dir}/%v.m3u8"),
    ]);
    args
}

const INDEPENDENT_SEGMENTS: &str = "#EXT-X-INDEPENDENT-SEGMENTS";

/// The two things ffmpeg leaves wrong in a master playlist for this shape.
///
/// `EXT-X-INDEPENDENT-SEGMENTS` is missing. It asserts that every segment
/// starts with a keyframe, which `-g`/`-keyint_min`/`-sc_threshold 0` above
/// guarantee. It belongs in a VOD multivariant playlist and ffmpeg's
/// `independent_segments` flag declines to emit it here ("whenever applicable"
/// — measured: not emitted for this fmp4 + rendition-group shape), so it is
/// inserted after the version line rather than left missing.
///
/// `BANDWIDTH` is understated — see [`declare_audio_in_bandwidth`].
///
/// The tag doubles as the mark of an already-patched master, which is what
/// keeps this idempotent now that it also rewrites numbers: a second pass
/// would otherwise charge every variant for its audio twice.
pub fn patch_master(master: &str, rungs: &[VideoRung]) -> String {
    if master.lines().any(|l| l.trim() == INDEPENDENT_SEGMENTS) {
        return master.to_string();
    }
    add_independent_segments(&declare_audio_in_bandwidth(master, rungs))
}

/// Fold each variant's audio rendition into the bandwidth it advertises.
///
/// ffmpeg measures a variant from its own video segments and stops there, so a
/// variant that plays a *separate* audio rendition is advertised at the price
/// of its picture alone. Measured on this ladder's shape: the 320x180 rung
/// declared `BANDWIDTH=49669` while the 32 kbps rendition it names cost another
/// ~35,000 bps — understated by 42%.
///
/// RFC 8216 §4.3.4.2 requires the figure to cover every Rendition the variant
/// will use, and it is the only thing a player selects on. The error is worst
/// at the bottom of the ladder, which is the one place accuracy decides whether
/// a starved viewer sees a picture at all: splitting audio into a lean group
/// and a clean one exists so a player can tell the lean rungs are cheap, and it
/// cannot tell that if the price it reads leaves the audio out.
///
/// The rung's *target* audio rate is added rather than the rendition's measured
/// one. That keeps this a pure transform of the master — no second file to read
/// — and the renditions are CBR, so it lands within a few percent.
fn declare_audio_in_bandwidth(master: &str, rungs: &[VideoRung]) -> String {
    let mut out = String::with_capacity(master.len() + 64);
    for line in master.lines() {
        // No `AUDIO=` means no separate rendition, and then ffmpeg's figure is
        // already the whole variant.
        let rung = line
            .starts_with("#EXT-X-STREAM-INF")
            .then(|| attr(line, "AUDIO").and(rung_of(line, rungs)))
            .flatten();
        match rung {
            Some(r) => out.push_str(&add_bitrate(line, u64::from(r.audio_kbps) * 1000)),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// The rung a variant line describes. `RESOLUTION` identifies it: no two rungs
/// in the ladder share a frame size.
fn rung_of<'a>(line: &str, rungs: &'a [VideoRung]) -> Option<&'a VideoRung> {
    let (w, h) = attr(line, "RESOLUTION")?.split_once('x')?;
    let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
    rungs.iter().find(|r| r.width == w && r.height == h)
}

/// One attribute's value, unquoted.
///
/// Attributes are comma-separated but a quoted value may itself hold a comma
/// (`CODECS="avc1.42c00c,mp4a.40.2"`), so splitting on commas yields fragments.
/// Matching the key exactly is enough to ignore them — a fragment carries no
/// `KEY=` this asks for — and it keeps `BANDWIDTH` from matching
/// `AVERAGE-BANDWIDTH`, which a substring search would.
fn attr<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split(',').find_map(|field| {
        let (k, v) = field.split_once('=')?;
        (k.rsplit(':').next()? == key).then(|| v.trim_matches('"'))
    })
}

/// Add `extra` bps to both bandwidth declarations on a variant line, leaving
/// every other attribute — a comma-bearing quoted one included — byte-identical.
fn add_bitrate(line: &str, extra: u64) -> String {
    let bumped = |field: &str| -> Option<String> {
        let (k, v) = field.split_once('=')?;
        matches!(k.rsplit(':').next(), Some("BANDWIDTH" | "AVERAGE-BANDWIDTH"))
            .then(|| v.parse::<u64>().ok())
            .flatten()
            .map(|n| format!("{k}={}", n + extra))
    };
    line.split(',')
        .map(|f| bumped(f).unwrap_or_else(|| f.to_string()))
        .collect::<Vec<_>>()
        .join(",")
}

fn add_independent_segments(master: &str) -> String {
    let mut out = String::with_capacity(master.len() + INDEPENDENT_SEGMENTS.len() + 1);
    let mut inserted = false;
    for line in master.lines() {
        out.push_str(line);
        out.push('\n');
        if !inserted && line.starts_with("#EXT-X-VERSION") {
            out.push_str(INDEPENDENT_SEGMENTS);
            out.push('\n');
            inserted = true;
        }
    }
    if !inserted {
        // No version line to anchor to; the tag still has to precede the
        // variants, so it goes directly after #EXTM3U.
        let anchored = format!("#EXTM3U\n{INDEPENDENT_SEGMENTS}\n");
        return out.replacen("#EXTM3U\n", &anchored, 1);
    }
    out
}

/// Encode the HLS ladder for a source, beside its progressive MP4.
///
/// Encodes the rungs it is given and returns the files it wrote. It does not
/// decide whether a ladder is warranted — [`produce_ladder`] owns that, so the
/// rule lives in one place instead of being checked here and there.
///
/// The master playlist is rewritten on the way out to carry
/// `EXT-X-INDEPENDENT-SEGMENTS`, which ffmpeg will not emit for this shape.
#[allow(clippy::too_many_arguments)]
pub fn encode_ladder(
    ffmpeg_bin: &Path,
    source: &Path,
    out_dir: &Path,
    rungs: &[VideoRung],
    probe: &SourceVideo,
    config: &VideoCompressionConfig,
    progress: Option<&dyn Fn(f64)>,
    registry: Option<&ChildProcessRegistry>,
    cancel_flag: Option<&AtomicBool>,
) -> Result<Vec<String>, String> {
    if cancel_flag.is_some_and(|f| f.load(Ordering::SeqCst)) {
        return Err("Cancelled".to_string());
    }

    let dir_str = out_dir.to_str().ok_or("Invalid output directory")?;
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("Failed to create output directory: {}", e))?;

    let args = build_hls_args(
        source.to_str().ok_or("Invalid input path")?,
        dir_str,
        rungs,
        probe.fps,
        probe.has_audio,
        &config.preset,
        config.encode_threads,
    );
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

    let noop = |_: f64| {};
    let on_progress: &dyn Fn(f64) = progress.unwrap_or(&noop);

    let out = spawn_ffmpeg_streaming(
        ffmpeg_bin,
        &arg_refs,
        probe.duration_secs,
        on_progress,
        registry,
        cancel_flag,
    )?;
    if !out.status.success() {
        return Err(format!(
            "HLS ladder encode failed for {}: {}",
            source.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    let master_path = out_dir.join(HLS_MASTER_NAME);
    let master = std::fs::read_to_string(&master_path)
        .map_err(|e| format!("Failed to read master playlist: {}", e))?;
    let patched = patch_master(&master, rungs);
    if patched != master {
        // The encode that just finished wrote this file into its own scratch
        // directory; the object store copies it out afterwards. Never a vault
        // path, so never evictable.
        // allow:raw_write ffmpeg scratch directory, not .moss/build/
        std::fs::write(&master_path, patched)
            .map_err(|e| format!("Failed to write master playlist: {}", e))?;
    }

    // Every name inside the directory is constant, so the census is the same
    // list wherever the ladder lands — that is what lets a cached ladder be
    // relinked under a renamed video without rewriting a playlist.
    Ok(hls_members(rungs))
}

#[cfg(test)]
#[path = "hls_tests.rs"]
mod hls_tests;

/// Produce the ladder for one video: cache lookup, encode on a miss, link
/// every file into staging, and hand back the transform entries.
///
/// The worker calls only this. Splitting it the other way — the worker
/// orchestrating seventeen files while `hls.rs` owns only the command line —
/// would put the naming scheme in two places, and the master playlist's
/// references are exactly what a naming disagreement breaks.
///
/// `Ok(None)` means no ladder was produced and none is owed: the source fills
/// fewer than two rungs. That is the honest resolution of "`EncodePlan::
/// KeepOriginal` cannot survive HLS" — segmenting is mandatory, so shipping the
/// source bytes as a ladder is not available, but *not building one* is: a
/// ladder is a choice between rungs, and one rung is three extra files offering
/// a player nothing to switch to. The progressive MP4 already serves it.
///
/// An `Err` means the ladder failed but the video did
/// not — the progressive MP4 is still the page's video, so the caller logs and
/// carries on rather than failing the conversion.
///
/// Caching is per file rather than per ladder. Each output is content-addressed
/// under its own transform name (`video/hls/v0.m3u8`), which needs no new cache
/// machinery, and a partial ladder is treated as a miss: seventeen files that
/// reference each other are only valid all together.
///
/// `ladder_dir` is the video's own `<stem>.hls` directory in staging. Every
/// name inside it is a constant, so the bytes never mention the video's name
/// and a cache hit can be relinked under a renamed source unchanged.
#[allow(clippy::too_many_arguments)]
pub(crate) fn produce_ladder(
    ffmpeg: &FFmpegManager,
    source_file: &Path,
    source_oid: &str,
    ladder_dir: &Path,
    temp_dir: &Path,
    transforms: &TransformCache,
    config: &VideoCompressionConfig,
    progress: Option<&dyn Fn(f64)>,
    registry: Option<&ChildProcessRegistry>,
    cancel_flag: Option<&AtomicBool>,
) -> Result<Option<Vec<(String, TransformEntry)>>, String> {
    let objects = transforms.objects();
    let params = ladder_params(config);

    // The cache is consulted before the source is touched. A cached ladder is
    // seventeen finished files and nothing about them needs re-reading the
    // source — and on an evicted cloud vault, probing first would turn a build
    // that has everything it needs into a download.
    let (members, oids) = match cached_ladder(transforms, source_oid, &params) {
        Some(hit) => hit,
        None => {
            let probe = ffmpeg.probe_source(source_file)?;
            let rungs = video_ladder_rungs(probe.width);
            if rungs.len() < 2 {
                return Ok(None);
            }
            // `hls_members` is the one owner of the naming scheme: the scratch
            // names, the staging names and the cache keys are all this same
            // list. The scratch directory can be named anything, because
            // nothing inside the ladder refers to the directory it sits in.
            let members = hls_members(rungs);
            let scratch = temp_dir.join(format!("hls-{}", uuid::Uuid::new_v4()));
            let result = encode_ladder(
                Path::new(ffmpeg.bin_path()),
                source_file,
                &scratch,
                rungs,
                &probe,
                config,
                progress,
                registry,
                cancel_flag,
            );
            let stored: Result<Vec<String>, String> = result.and_then(|_written| {
                members
                    .iter()
                    .map(|name| objects.store_file(&scratch.join(name)))
                    .collect()
            });
            let _ = std::fs::remove_dir_all(&scratch);
            (members, stored?)
        }
    };
    let mut entries = Vec::with_capacity(oids.len());
    for (name, oid) in members.iter().zip(oids) {
        let target = ladder_dir.join(name);
        objects
            .link_to(&oid, &target)
            .map_err(|e| format!("Failed to link {}: {}", target.display(), e))?;
        let size = objects
            .get_path(&oid)
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0);
        entries.push((
            transform_name(name),
            TransformEntry {
                oid,
                size,
                params: params.clone(),
            },
        ));
    }
    Ok(Some(entries))
}

/// A cached ladder, or nothing — never a partial one.
///
/// The rung count is read back out of the record rather than re-derived from
/// the source, because the record is the only thing that knows which table the
/// files were encoded under. `video_ladder_rungs` always truncates from the
/// top, so a ladder of `k` rungs is `VIDEO_LADDER[..k]` and the expected census
/// follows from `k` alone. If the record's `video/hls*` entries are not exactly
/// that census — a rung evicted, a blob swept, a table edited — it is a miss,
/// because seventeen files that reference each other are only valid together.
/// The cache key for one file of a ladder. The name is a constant, so the key
/// says nothing about which video it belongs to beyond the record it sits in.
fn transform_name(member: &str) -> String {
    format!("video/hls/{member}")
}

fn cached_ladder(
    transforms: &TransformCache,
    source_oid: &str,
    params: &serde_json::Value,
) -> Option<(Vec<String>, Vec<String>)> {
    let record = transforms.get(source_oid)?;
    let rung_count = record
        .transforms
        .keys()
        .filter(|k| k.starts_with("video/hls/v") && k.ends_with(".m3u8"))
        .count();
    if rung_count < 2 {
        return None;
    }
    let rungs = video_ladder_rungs_by_count(rung_count)?;
    let members = hls_members(rungs);
    if record
        .transforms
        .keys()
        .filter(|k| k.starts_with("video/hls/"))
        .count()
        != members.len()
    {
        return None;
    }
    let oids: Option<Vec<String>> = members
        .iter()
        .map(|name| transforms.find_cached_output(source_oid, &transform_name(name), params))
        .collect();
    Some((members, oids?))
}

/// What invalidates a cached ladder: the rung table and the encoder settings
/// that shape the bytes. Shared by every file in one ladder, so a table edit
/// re-encodes the whole thing rather than leaving rungs from two generations
/// referencing each other.
fn ladder_params(config: &VideoCompressionConfig) -> serde_json::Value {
    serde_json::json!({
        "ladder": video_ladder_fingerprint(),
        "preset": config.preset,
        "segment_seconds": SEGMENT_SECONDS,
        "keyframe_seconds": KEYFRAME_SECONDS,
    })
}

/// Register every ladder already present in `staging`, returning how many.
///
/// A ladder is registered during video conversion, which runs *after* the HTML
/// is rendered. The app gets away with that: one `AssetRegistry` is shared
/// across rebuilds via `app.manage`, so the emitter picks a ladder up on the
/// rebuild after the one that encoded it. `moss build` does not — it gets a
/// fresh registry per process and then exits, so without this it would emit
/// ladder markup on *no* build, ever, however many times it ran. Seventeen
/// files per video, encoded and uploaded and referenced by nothing.
///
/// Read off disk rather than out of the transform cache, and the trade is one
/// build of latency for never being wrong. The cache knows a ladder exists
/// sooner — it is what causes this build to link the files into staging — so a
/// cache-sourced answer would emit markup one build earlier on a fresh checkout
/// with a warm cache. But what makes an emitted URL true is a file at that URL
/// (ADR-013), and only disk answers that question. The `master.m3u8` is the
/// gate: `hls_master_stem` is what marks a stem as laddered, and a directory
/// without one is a half-written ladder that must not be advertised.
///
/// `set_pending` here has no paired `set_source_passthrough`, which every other
/// caller does have. That pairing exists so a variant requested before it is
/// encoded serves the original bytes; here the file is already on disk, so the
/// passthrough branch is unreachable by construction.
pub fn register_existing_ladders(
    staging: &Path,
    registry: &crate::types::assets::AssetRegistry,
) -> usize {
    let mut found = 0;
    for entry in walkdir::WalkDir::new(staging)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_dir())
    {
        let dir = entry.path();
        if dir.extension().and_then(|e| e.to_str()) != Some("hls")
            || !dir.join(HLS_MASTER_NAME).is_file()
        {
            continue;
        }
        let Ok(rel_dir) = dir.strip_prefix(staging) else {
            continue;
        };
        for member in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let Some(name) = member.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            // Bare, like every other registry writer. The key space no longer
            // has to be guessed — `variant_key` normalizes both forms — so this
            // takes the form the rest of the tree uses.
            let url = format!("{}/{}", rel_dir.to_string_lossy().replace('\\', "/"), name);
            registry.set_pending(url.clone(), None, None);
            registry.set_ready(url);
        }
        found += 1;
    }
    found
}
