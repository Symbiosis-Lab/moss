//! Whether a video already has a usable HLS ladder cached, and the key that
//! decides it.
//!
//! Split out of `hls.rs` (the encode/link/argument-building half) because
//! this is a different question: not "how do we ask ffmpeg for a ladder" but
//! "does the cache already hold a whole ladder for this source, and how do we
//! tell without reading the source." `HLS_TRANSFORM_PREFIX` and
//! `transform_name` stay in the parent module — `link_members` there needs
//! them too, and both files reach them the same way, through `super::`.

use moss_core::asset_paths::{
    rung_list_fingerprint, video_ladder_rungs_within, VideoRung, HLS_MASTER_NAME, VIDEO_LADDER,
};

use crate::build::cache::{Fit, TransformCache, TransformRecord};
use crate::build::media::ffmpeg::{progressive_rung, SourceVideo, VideoCompressionConfig};

use super::{transform_name, HLS_TRANSFORM_PREFIX, KEYFRAME_SECONDS, SEGMENT_SECONDS};

/// The recipe a ladder was made with, recorded in its key. Bump when the bytes
/// a rung's encode produces change for the same inputs; a ladder already made
/// keeps its version (see [`cached_ladder`]).
/// 3: every rung aims at a picture quality under its rate, and the master
/// declares what each rung measures; version 2 ladders aimed at the rate.
/// 2: rungs are frames fitted to the source's own shape with square pixels;
/// version 1 ladders (and the unversioned ones before them) forced every rung
/// to a fixed 16:9 frame.
pub(super) const LADDER_FORMAT: u32 = 3;

/// This source's effective ladder under `config`'s current budgets: the
/// per-file one, and the web file's, which decides the rung the ladder must
/// hold ([`progressive_rung`]). The one place [`video_ladder_rungs_within`] is
/// called from, whether the source was just probed or read back out of a
/// record, so the two can never compute a different answer for the same
/// source.
pub(crate) fn effective_rungs(source: &SourceVideo, config: &VideoCompressionConfig) -> Vec<VideoRung> {
    let max_file_bytes = u64::from(config.hls_max_file_mb) * 1024 * 1024;
    video_ladder_rungs_within(
        source.display(),
        source.duration_secs,
        max_file_bytes,
        progressive_rung(source, config),
    )
}

/// The probe facts stored in `ladder_params`'s `"source"` field, so a later
/// lookup can ask "what would this source's effective ladder be under TODAY's
/// budgets?" without reading the source: `cached_ladder` must never read it
/// before a hit.
fn source_to_json(source: &SourceVideo) -> serde_json::Value {
    serde_json::json!({
        "width": source.width,
        "height": source.height,
        "sar": source.sar,
        "duration_secs": source.duration_secs,
        "fps": source.fps,
        "total_kbps": source.total_kbps,
        "web_playable": source.web_playable,
        "has_audio": source.has_audio,
    })
}

/// [`source_to_json`] read back. A record from before a field existed reads
/// as the safe default (no height, square pixels, 30 fps, not web-playable,
/// with audio), and misses anyway on [`LADDER_FORMAT`].
fn source_from_json(v: &serde_json::Value) -> Option<SourceVideo> {
    Some(SourceVideo {
        width: v.get("width")?.as_u64()? as u32,
        height: v.get("height").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32,
        sar: v.get("sar").and_then(serde_json::Value::as_f64).unwrap_or(1.0),
        duration_secs: v.get("duration_secs")?.as_f64()?,
        fps: v.get("fps").and_then(serde_json::Value::as_f64).unwrap_or(30.0),
        total_kbps: v.get("total_kbps").and_then(serde_json::Value::as_f64),
        web_playable: v.get("web_playable").and_then(serde_json::Value::as_bool).unwrap_or(false),
        has_audio: v.get("has_audio").and_then(serde_json::Value::as_bool).unwrap_or(true),
    })
}

/// How many rungs a cached ladder's own file census names — its `video/hls/
/// v*.m3u8` keys counted. `None` for a record with no ladder keys, too few to
/// be a real ladder, or more than the table can produce.
fn record_rung_count(record: &TransformRecord) -> Option<usize> {
    let rung_prefix = format!("{HLS_TRANSFORM_PREFIX}v");
    let rung_count = record
        .transforms
        .keys()
        .filter(|k| k.starts_with(rung_prefix.as_str()) && k.ends_with(".m3u8"))
        .count();
    (2..=VIDEO_LADDER.len()).contains(&rung_count).then_some(rung_count)
}

/// A cached ladder, or nothing — never a partial one.
///
/// A ladder is made once per source: one made with an older recipe or rung
/// table is kept while every file its own master names is usable and within
/// today's per-file budget. The page links only the master, so a kept ladder
/// stays self-consistent. A ladder made with today's key is checked against
/// today's file list instead, with no read. Never reads the source: on an
/// evicted cloud vault, probing first would turn a build that has everything
/// into a download.
///
/// The params returned are the ones the ladder was made with, so the record
/// keeps saying which recipe made it.
pub(super) fn cached_ladder(
    transforms: &TransformCache,
    source_oid: &str,
    config: &VideoCompressionConfig,
) -> Option<(Vec<String>, Vec<String>, serde_json::Value)> {
    let record = transforms.get_with(source_oid, crate::build::cache::RecordMode::Wait)?;
    record_rung_count(&record)?;
    let master = record.transforms.get(&transform_name(HLS_MASTER_NAME))?;
    let today = master.params.get("source").and_then(source_from_json).map(|source| {
        let rungs = effective_rungs(&source, config);
        (super::ladder_members(&rungs, source.has_audio), ladder_params(config, &rungs, &source))
    });
    let (members, today_params) = match today {
        Some((members, params)) if params == master.params => (members, params),
        other => {
            let text = std::fs::read_to_string(transforms.objects().ready_blob(&master.oid)?).ok()?;
            (super::members_named_by(&text), other.map_or(serde_json::Value::Null, |(_, params)| params))
        }
    };
    if record.transforms.keys().filter(|k| k.starts_with(HLS_TRANSFORM_PREFIX)).count() != members.len() {
        return None;
    }
    let budget = u64::from(config.hls_max_file_mb) * 1024 * 1024;
    let within = || Some(crate::build::cache::Need::AtMost(budget));
    let oids: Option<Vec<String>> = members
        .iter()
        .map(|name| transforms.kept_entry(&record, &transform_name(name), &today_params, Fit::Kept(&within)).map(|e| e.oid))
        .collect();
    Some((members, oids?, master.params.clone()))
}

/// The transform-cache key for one ladder member: the encoder settings and the
/// effective rung list that shape its bytes, plus the probe facts
/// [`cached_ladder`] reads back to compute today's ladder without probing.
/// Budgets and the static table are absent: their whole effect lives in
/// `rungs`. A ladder under another key is still kept while it fits.
///
/// `pub(crate)` so video.rs's tests can seed the params a real encode stores.
pub(crate) fn ladder_params(
    config: &VideoCompressionConfig,
    rungs: &[VideoRung],
    source: &SourceVideo,
) -> serde_json::Value {
    serde_json::json!({
        "preset": config.preset,
        "segment_seconds": SEGMENT_SECONDS,
        "keyframe_seconds": KEYFRAME_SECONDS,
        "format": LADDER_FORMAT,
        "rungs": rung_list_fingerprint(rungs),
        "source": source_to_json(source),
    })
}

/// The two real shapes [`ladder_params`] built before this crate started
/// keying on effective rungs — test-only, reconstructed from git history so
/// a test can seed a genuinely legacy-form record without a second
/// reconstruction living in every module that needs one (hls_tests.rs's own
/// migration tests, and video.rs's staging-heal test). Neither variant
/// carries a `"cap"` field: that marker only ever existed in this branch's
/// own WIP commit and never shipped anywhere a real site's cache could hold
/// it, so a shape including it would test against bytes no record on disk
/// has ever actually had.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum LegacyShape {
    /// Commit `3f5111ff`'s own parent (develop): before the per-file budget
    /// was part of the key at all.
    NoBudget,
    /// Commit `3f5111ff` (develop): the per-file budget added to the key —
    /// this repo's real shape immediately before this crate's own rekeying.
    WithBudget,
}

/// A master playlist naming `members`, as a real encode writes one.
#[cfg(test)]
pub(crate) fn master_naming(members: &[String]) -> String {
    let mut master = String::from("#EXTM3U\n");
    for name in members.iter().filter(|n| n.ends_with(".m3u8") && n.as_str() != HLS_MASTER_NAME) {
        if name.starts_with('a') {
            master.push_str(&format!("#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"{name}\",URI=\"{name}\"\n"));
        } else {
            master.push_str(&format!("#EXT-X-STREAM-INF:BANDWIDTH=1\n{name}\n"));
        }
    }
    master
}

#[cfg(test)]
pub(crate) fn legacy_form_params(config: &VideoCompressionConfig, shape: LegacyShape) -> serde_json::Value {
    use moss_core::asset_paths::video_ladder_fingerprint;
    let mut params = serde_json::json!({
        "ladder": video_ladder_fingerprint(),
        "preset": config.preset,
        "segment_seconds": SEGMENT_SECONDS,
        "keyframe_seconds": KEYFRAME_SECONDS,
    });
    if matches!(shape, LegacyShape::WithBudget) {
        params["max_file_mb"] = serde_json::json!(config.hls_max_file_mb);
    }
    params
}
