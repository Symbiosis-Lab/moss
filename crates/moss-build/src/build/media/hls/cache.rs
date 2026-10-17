//! Whether a video already has a usable HLS ladder cached, and the key that
//! decides it.
//!
//! Split out of `hls.rs` (the encode/link/argument-building half) because
//! this is a different question: not "how do we ask ffmpeg for a ladder" but
//! "does the cache already hold the bytes today's policy would produce, and
//! how do we tell without reading the source." `HLS_TRANSFORM_PREFIX` and
//! `transform_name` stay in the parent module — `link_members` there needs
//! them too, and both files reach them the same way, through `super::`.

use moss_core::asset_paths::{
    rung_list_fingerprint, video_ladder_rungs_within, VideoRung, HLS_MASTER_NAME, VIDEO_LADDER,
};

use crate::build::cache::{TransformCache, TransformRecord};
use crate::build::media::ffmpeg::{progressive_rung, SourceVideo, VideoCompressionConfig};

use super::{transform_name, HLS_TRANSFORM_PREFIX, KEYFRAME_SECONDS, SEGMENT_SECONDS};

/// Bump when the bytes a rung's encode produces change for the same inputs.
/// 3: every rung aims at a picture quality under its rate, and the master
/// declares what each rung measures; version 2 ladders aimed at the rate.
/// 2: rungs are frames fitted to the source's own shape with square pixels;
/// version 1 ladders (and the unversioned ones before them) forced every rung
/// to a fixed 16:9 frame. Each bump rebuilds existing ladders once.
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

/// A cached ladder, or nothing — never a partial one, and never a LEGACY-form
/// one (a record from before this crate started keying on effective rungs: it
/// carries no `"source"` block, so it misses and is encoded afresh).
///
/// The rung count is read back out of the record rather than re-derived from
/// the source, because the record is the only thing that knows how many
/// files were encoded.
///
/// Never reads the source: its probe facts are read back out of the record —
/// `ladder_params` stored them there at encode time — and recomputed against
/// TODAY's config. `video_ladder_rungs_within` only ever narrows from the top,
/// so two rung lists computed from the SAME source facts and landing at the
/// SAME length are the same rungs; a length mismatch alone is therefore
/// enough to catch a budget or table edit that would change this ladder. If
/// the record's `video/hls*` entries are not exactly that census — a rung
/// evicted, a blob swept, a table edited — it is a miss, because a ladder's
/// files that reference each other are only valid together.
pub(super) fn cached_ladder(
    transforms: &TransformCache,
    source_oid: &str,
    config: &VideoCompressionConfig,
) -> Option<(Vec<String>, Vec<String>, serde_json::Value)> {
    let record = transforms.get_with(source_oid, crate::build::cache::RecordMode::Wait)?;
    let rung_count = record_rung_count(&record)?;

    let stored = record.transforms.get(&transform_name(HLS_MASTER_NAME))?;
    let source = source_from_json(stored.params.get("source")?)?;
    let effective = effective_rungs(&source, config);
    if effective.len() != rung_count {
        return None;
    }

    let members = super::ladder_members(&effective, source.has_audio);
    if record.transforms.keys().filter(|k| k.starts_with(HLS_TRANSFORM_PREFIX)).count() != members.len() {
        return None;
    }

    let params = ladder_params(config, &effective, &source);
    let oids: Option<Vec<String>> = members
        .iter()
        .map(|name| transforms.find_cached_output(source_oid, &transform_name(name), &params, crate::build::cache::RecordMode::Wait))
        .collect();
    Some((members, oids?, params))
}

/// The transform-cache key for one ladder member: what shapes its bytes — the
/// encoder settings and the EFFECTIVE rung list this video is (or would be)
/// encoded at — plus the probe facts `cached_ladder` needs to answer
/// "would TODAY's budget produce a different ladder?" without re-probing.
///
/// Policy inputs that do not shape the bytes ON THEIR OWN are deliberately
/// absent: the static table's fingerprint, the per-file byte budget, and the
/// old bitrate-cap marker. Their entire effect already lives in `rungs` — a
/// table edit or a budget edit that leaves one particular video's effective
/// ladder unchanged no longer re-encodes it, which is the point of keying on
/// the EFFECT rather than every input that could in principle change it.
///
/// `source` is never independently verified against anything: every caller
/// reads it straight back out of a record (or a fresh probe) before calling
/// this, so it always matches what's already there or about to be stored —
/// it rides along in the key purely so a later `cached_ladder` lookup has
/// somewhere to read it back from.
///
/// `pub(crate)`, not private: video.rs's own tests seed a `TransformRecord`
/// directly (no real ffmpeg) to exercise the staging self-heal, and need the
/// exact params a real encode would have stored so `cached_ladder` sees a hit.
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
