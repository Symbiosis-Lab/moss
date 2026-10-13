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
    display_size, rung_list_fingerprint, video_ladder_rungs_by_count,
    video_ladder_rungs_within, SourceBitrate, VideoRung, HLS_MASTER_NAME,
};

use crate::build::cache::{TransformCache, TransformRecord};
use crate::build::media::ffmpeg::{SourceVideo, VideoCompressionConfig};

use super::{transform_name, HLS_TRANSFORM_PREFIX, KEYFRAME_SECONDS, SEGMENT_SECONDS};

/// Bump when the bytes a rung's encode produces change for the same inputs.
/// 3: every rung aims at a picture quality under its rate, and the master
/// declares what each rung measures; version 2 ladders aimed at the rate.
/// 2: rungs are frames fitted to the source's own shape with square pixels;
/// version 1 ladders (and the unversioned ones before them) forced every rung
/// to a fixed 16:9 frame. Each bump rebuilds existing ladders once.
pub(super) const LADDER_FORMAT: u32 = 3;

/// The source facts [`video_ladder_rungs_within`] needs, captured once at
/// encode time and carried inside `ladder_params`'s `"source"` field so a
/// later lookup can ask "what would this source's effective ladder be under
/// TODAY's budget?" — `cached_ladder` must never read the source before a
/// hit, and storing this is what lets it answer that without one.
///
/// `pub(crate)`, fields included: video.rs's own tests build one directly to
/// seed a `TransformRecord` with (no real ffmpeg) for the staging self-heal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SourceFacts {
    pub(crate) width: u32,
    /// Height and pixel aspect ratio: with `width` they give the display
    /// size that decides which rungs the source fills. A record from before
    /// these existed reads back as height 0 / square pixels, which still
    /// answers by width alone — and misses anyway, on [`LADDER_FORMAT`].
    pub(crate) height: u32,
    pub(crate) sar: f64,
    pub(crate) duration_secs: f64,
    pub(crate) video_kbps: Option<f64>,
    pub(crate) total_kbps: Option<f64>,
    /// Whether the source carries an audio stream — [`super::ladder_members`]
    /// needs this to know which member list a cache hit is vouching for; a
    /// record from before this field existed has none to read back, so
    /// [`Self::from_json`] defaults it to `true` rather than failing the
    /// whole lookup, which is exactly what every such record was actually
    /// encoded with (a silent source never reached a successful cache write
    /// before this field existed — see the encode-site fix this travels
    /// with).
    pub(crate) has_audio: bool,
}

impl SourceFacts {
    pub(crate) fn from_probe(probe: &SourceVideo) -> Self {
        Self {
            width: probe.width,
            height: probe.height,
            sar: probe.sar,
            duration_secs: probe.duration_secs,
            video_kbps: probe.video_kbps,
            total_kbps: probe.total_kbps,
            has_audio: probe.has_audio,
        }
    }

    /// This source's effective ladder under `config`'s current per-file
    /// budget. The one place [`video_ladder_rungs_within`] is called from
    /// either a fresh probe or a record's stored facts, so the two can never
    /// compute a different answer for what claims to be the same source.
    pub(crate) fn effective_rungs(&self, config: &VideoCompressionConfig) -> Vec<VideoRung> {
        let max_file_bytes = u64::from(config.hls_max_file_mb) * 1024 * 1024;
        video_ladder_rungs_within(
            self.display(),
            self.duration_secs,
            max_file_bytes,
            SourceBitrate { video_kbps: self.video_kbps, total_kbps: self.total_kbps },
        )
    }

    pub(crate) fn display(&self) -> (u32, u32) {
        display_size(self.width, self.height, self.sar)
    }

    fn to_json(self) -> serde_json::Value {
        serde_json::json!({
            "width": self.width,
            "height": self.height,
            "sar": self.sar,
            "duration_secs": self.duration_secs,
            "video_kbps": self.video_kbps,
            "total_kbps": self.total_kbps,
            "has_audio": self.has_audio,
        })
    }

    fn from_json(v: &serde_json::Value) -> Option<Self> {
        Some(Self {
            width: v.get("width")?.as_u64()? as u32,
            height: v.get("height").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32,
            sar: v.get("sar").and_then(serde_json::Value::as_f64).unwrap_or(1.0),
            duration_secs: v.get("duration_secs")?.as_f64()?,
            video_kbps: v.get("video_kbps").and_then(serde_json::Value::as_f64),
            total_kbps: v.get("total_kbps").and_then(serde_json::Value::as_f64),
            has_audio: v.get("has_audio").and_then(serde_json::Value::as_bool).unwrap_or(true),
        })
    }
}

/// The rung table a cached ladder's own file census names — its `video/hls/
/// v*.m3u8` keys counted, then bounds-checked against the real table lengths
/// (2..=the table length, [`video_ladder_rungs_by_count`]). `None` for a record
/// with no ladder keys, too few to be a real ladder, or (in principle) more than the
/// table can produce.
fn record_table_rungs(record: &TransformRecord) -> Option<&'static [VideoRung]> {
    let rung_prefix = format!("{HLS_TRANSFORM_PREFIX}v");
    let rung_count = record
        .transforms
        .keys()
        .filter(|k| k.starts_with(rung_prefix.as_str()) && k.ends_with(".m3u8"))
        .count();
    video_ladder_rungs_by_count(rung_count)
}

/// A cached ladder, or nothing — never a partial one, and never a LEGACY-form
/// one (a record from before this crate started keying on effective rungs: it
/// carries no `"source"` block, so it misses and is encoded afresh).
///
/// The rung count is read back out of the record rather than re-derived from
/// the source, because the record is the only thing that knows which table the
/// files were encoded under. `video_ladder_rungs` AND `video_ladder_rungs_within`
/// both always truncate from the top, so a ladder of `k` rungs has the WIDTH,
/// HEIGHT, FPS and AUDIO GROUP of `VIDEO_LADDER[..k]` regardless of which one
/// produced it.
///
/// Never reads the source: `SourceFacts` are read back out of the record —
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
    // `effective` below stands in for the table slice `record_table_rungs`
    // would otherwise return; only its length matters here.
    let rung_count = record_table_rungs(&record)?.len();

    let stored = record.transforms.get(&transform_name(HLS_MASTER_NAME))?;
    let source = SourceFacts::from_json(stored.params.get("source")?)?;
    let effective = source.effective_rungs(config);
    if effective.len() != rung_count {
        return None;
    }

    let members = super::ladder_members(&effective, source.has_audio);
    if record.transforms.keys().filter(|k| k.starts_with(HLS_TRANSFORM_PREFIX)).count() != members.len() {
        return None;
    }

    let params = ladder_params(config, &effective, source);
    let oids: Option<Vec<String>> = members
        .iter()
        .map(|name| transforms.find_cached_output(source_oid, &transform_name(name), &params, crate::build::cache::RecordMode::Wait))
        .collect();
    Some((members, oids?, params))
}

/// The transform-cache key for one ladder member: what shapes its bytes — the
/// encoder settings and the EFFECTIVE rung list this video is (or would be)
/// encoded at — plus the `SourceFacts` `cached_ladder` needs to answer
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
    source: SourceFacts,
) -> serde_json::Value {
    serde_json::json!({
        "preset": config.preset,
        "segment_seconds": SEGMENT_SECONDS,
        "keyframe_seconds": KEYFRAME_SECONDS,
        "format": LADDER_FORMAT,
        "rungs": rung_list_fingerprint(rungs),
        "source": source.to_json(),
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
