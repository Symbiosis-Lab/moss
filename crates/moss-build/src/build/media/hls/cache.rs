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
    hls_members, rung_list_fingerprint, video_ladder_fingerprint, video_ladder_rungs_by_count,
    video_ladder_rungs_within, SourceBitrate, VideoRung, HLS_MASTER_NAME,
};

use crate::build::cache::{TransformCache, TransformRecord};
use crate::build::media::ffmpeg::{SourceVideo, VideoCompressionConfig};

use super::{transform_name, HLS_TRANSFORM_PREFIX, KEYFRAME_SECONDS, SEGMENT_SECONDS};

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
    pub(crate) duration_secs: f64,
    pub(crate) video_kbps: Option<f64>,
    pub(crate) total_kbps: Option<f64>,
}

impl SourceFacts {
    pub(crate) fn from_probe(probe: &SourceVideo) -> Self {
        Self {
            width: probe.width,
            duration_secs: probe.duration_secs,
            video_kbps: probe.video_kbps,
            total_kbps: probe.total_kbps,
        }
    }

    /// This source's effective ladder under `config`'s current per-file
    /// budget. The one place [`video_ladder_rungs_within`] is called from
    /// either a fresh probe or a record's stored facts, so the two can never
    /// compute a different answer for what claims to be the same source.
    pub(crate) fn effective_rungs(&self, config: &VideoCompressionConfig) -> Vec<VideoRung> {
        let max_file_bytes = u64::from(config.hls_max_file_mb) * 1024 * 1024;
        video_ladder_rungs_within(
            self.width,
            self.duration_secs,
            max_file_bytes,
            SourceBitrate { video_kbps: self.video_kbps, total_kbps: self.total_kbps },
        )
    }

    fn to_json(self) -> serde_json::Value {
        serde_json::json!({
            "width": self.width,
            "duration_secs": self.duration_secs,
            "video_kbps": self.video_kbps,
            "total_kbps": self.total_kbps,
        })
    }

    fn from_json(v: &serde_json::Value) -> Option<Self> {
        Some(Self {
            width: v.get("width")?.as_u64()? as u32,
            duration_secs: v.get("duration_secs")?.as_f64()?,
            video_kbps: v.get("video_kbps").and_then(serde_json::Value::as_f64),
            total_kbps: v.get("total_kbps").and_then(serde_json::Value::as_f64),
        })
    }
}

/// The rung table a cached ladder's own file census names — its `video/hls/
/// v*.m3u8` keys counted, then bounds-checked against the real table lengths
/// (2..=6, [`video_ladder_rungs_by_count`]). `None` for a record with no
/// ladder keys, too few to be a real ladder, or (in principle) more than the
/// table can produce. The one place `cached_ladder` and `legacy_cached_
/// ladder` both open with this exact census, so the two readings of "how
/// many rungs does this record claim" can't drift apart from each other.
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
/// one (a record from before this crate started keying on effective rungs;
/// see `legacy_cached_ladder`, `produce_ladder`'s own migration path for that).
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
/// evicted, a blob swept, a table edited — it is a miss, because seventeen
/// files that reference each other are only valid together.
pub(super) fn cached_ladder(
    transforms: &TransformCache,
    source_oid: &str,
    config: &VideoCompressionConfig,
) -> Option<(Vec<String>, Vec<String>, serde_json::Value)> {
    let record = transforms.get(source_oid)?;
    // `effective` below stands in for the table slice `record_table_rungs`
    // would otherwise return; only its length matters here.
    let rung_count = record_table_rungs(&record)?.len();

    let stored = record.transforms.get(&transform_name(HLS_MASTER_NAME))?;
    let source = SourceFacts::from_json(stored.params.get("source")?)?;
    let effective = source.effective_rungs(config);
    if effective.len() != rung_count {
        return None;
    }

    let members = hls_members(&effective);
    if record.transforms.keys().filter(|k| k.starts_with(HLS_TRANSFORM_PREFIX)).count() != members.len() {
        return None;
    }

    let params = ladder_params(config, &effective, source);
    let oids: Option<Vec<String>> = members
        .iter()
        .map(|name| transforms.find_cached_output(source_oid, &transform_name(name), &params))
        .collect();
    Some((members, oids?, params))
}

/// `produce_ladder`'s own fallback for a ladder recorded before this crate
/// started keying on effective rungs. `cached_ladder` cannot recognize one —
/// its params carry no `"source"` block to recompute against — so this
/// re-examines it with the probe `produce_ladder`'s miss path already has to
/// take, costing nothing extra.
///
/// Every such record also predates the source-bitrate clamp, so it was
/// encoded at the table's own (unclamped) bitrates. A hit here requires
/// TODAY's clamp to agree that no clamp would apply — `effective ==
/// table_rungs` EXACTLY, not merely the same length — AND the encoder
/// settings and table fingerprint it was recorded under to still match
/// today's. Either check failing means the cached bytes are provably wrong
/// for today's policy, so this returns `None` and the ordinary encode path
/// re-encodes it once, this time under the new keying — every build after
/// this one needs no more probing.
///
/// Added 2026-09-24, migration-only: it earns its keep only while records
/// from before this rekeying are still out there. Delete it the next time
/// the ladder cache key's shape changes again (a migration path for a shape
/// this one doesn't recognize would need rewriting anyway), or once every
/// site has been rebuilt at least once by a release that carries it —
/// whichever comes first; either point means every remaining record has
/// already migrated to the new form and `cached_ladder` alone is enough.
pub(super) fn legacy_cached_ladder(
    transforms: &TransformCache,
    source_oid: &str,
    config: &VideoCompressionConfig,
    probe: &SourceVideo,
) -> Option<(Vec<String>, Vec<String>, serde_json::Value)> {
    let record = transforms.get(source_oid)?;
    let table_rungs = record_table_rungs(&record)?;
    let members = hls_members(table_rungs);
    if record.transforms.keys().filter(|k| k.starts_with(HLS_TRANSFORM_PREFIX)).count() != members.len() {
        return None;
    }

    let legacy = &record.transforms.get(&transform_name(HLS_MASTER_NAME))?.params;
    let settings_match = legacy.get("preset") == Some(&serde_json::json!(config.preset))
        && legacy.get("segment_seconds") == Some(&serde_json::json!(SEGMENT_SECONDS))
        && legacy.get("keyframe_seconds") == Some(&serde_json::json!(KEYFRAME_SECONDS))
        && legacy.get("ladder") == Some(&serde_json::json!(video_ladder_fingerprint()));
    if !settings_match {
        return None;
    }

    let source = SourceFacts::from_probe(probe);
    if source.effective_rungs(config) != table_rungs {
        // Today's clamp (or a table edit) would produce something other than
        // the table's own bitrates — the legacy bytes, encoded at those
        // unclamped bitrates, are wrong under today's policy.
        return None;
    }

    let oids: Vec<String> = members
        .iter()
        .map(|name| record.transforms.get(&transform_name(name)).map(|e| e.oid.clone()))
        .collect::<Option<Vec<_>>>()?;
    // Blob liveness — the same check `find_cached_output` makes, done by hand
    // because this path already bypassed it: a field-by-field comparison that
    // ignores `max_file_mb`/`cap`, not the exact equality that function needs.
    for oid in &oids {
        transforms.objects().ready_blob(oid)?;
    }

    Some((members, oids, ladder_params(config, table_rungs, source)))
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
