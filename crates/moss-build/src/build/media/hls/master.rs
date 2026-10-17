//! The master playlist's post-pass: what ffmpeg leaves wrong or version-
//! dependent in a multivariant playlist of this ladder's shape, corrected as
//! pure text transforms. Each variant is declared at what its segments
//! measure plus the audio it plays, and the playlist says its segments are
//! independent.

use moss_core::asset_paths::VideoRung;

const INDEPENDENT_SEGMENTS: &str = "#EXT-X-INDEPENDENT-SEGMENTS";

/// Correct ffmpeg's master playlist for this ladder's shape.
///
/// Each variant's `BANDWIDTH` and `AVERAGE-BANDWIDTH` become what `rates`
/// measures for its playlist ([`segment_rates`] of `v3.m3u8`, say), or stay
/// what ffmpeg declared when `rates` has no answer, and then grow by the audio
/// rendition the variant plays ([`audio_bps`]). A rung aims at a picture
/// quality and has no rate known before it is encoded; ffmpeg 7.1 and later
/// measure one, earlier versions declare the rung's rate ceiling plus a tenth,
/// which would have a player on a slow link decline a rung it could carry.
///
/// `EXT-X-INDEPENDENT-SEGMENTS` is added when missing. It asserts that every
/// segment starts with a keyframe, which the `-g`/`-keyint_min`/
/// `-sc_threshold 0` flags of `build_hls_args` guarantee. It belongs in a VOD
/// multivariant playlist and ffmpeg's `independent_segments` flag declines to
/// emit it here ("whenever applicable" — measured: not emitted for this fmp4 +
/// rendition-group shape), so it is inserted after the version line.
pub fn patch_master(master: &str, rungs: &[VideoRung], rates: impl Fn(&str) -> Option<(u64, u64)>) -> String {
    let tagged = master.lines().any(|l| l.trim() == INDEPENDENT_SEGMENTS);
    let mut out = String::with_capacity(master.len() + INDEPENDENT_SEGMENTS.len() + 64);
    let mut inserted = tagged;
    let mut lines = master.lines().peekable();
    while let Some(line) = lines.next() {
        match lines.peek().filter(|_| line.starts_with("#EXT-X-STREAM-INF")) {
            Some(uri) => out.push_str(&declare_variant(line, uri.trim(), rungs, &rates)),
            None => out.push_str(line),
        }
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

/// One variant line, its figures measured (or as declared) plus its audio.
/// A line with no readable `BANDWIDTH` is left as it is.
fn declare_variant(line: &str, uri: &str, rungs: &[VideoRung], rates: &impl Fn(&str) -> Option<(u64, u64)>) -> String {
    let declared = || -> Option<(u64, Option<u64>)> {
        let peak = attr(line, "BANDWIDTH")?.parse().ok()?;
        Some((peak, attr(line, "AVERAGE-BANDWIDTH").and_then(|v| v.parse().ok())))
    };
    let Some((peak, average)) = rates(uri).map(|(p, a)| (p, Some(a))).or_else(declared) else {
        return line.to_string();
    };
    let audio = audio_bps(line, uri, rungs);
    set_bitrates(line, peak + audio, average.map(|a| a + audio))
}

/// The bit rate of the audio rendition a variant plays, which its own figure
/// leaves out.
///
/// A variant's figure covers its video segments and stops there, so a variant
/// that plays a *separate* audio rendition is advertised at the price of its
/// picture alone. Measured on this ladder's shape: the 320x180 rung declared
/// `BANDWIDTH=49669` while the 32 kbps rendition it names cost another
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
/// one, and the renditions are CBR, so it lands within a few percent. No
/// `AUDIO=` means no separate rendition: the figure is already the whole
/// variant.
fn audio_bps(line: &str, uri: &str, rungs: &[VideoRung]) -> u64 {
    attr(line, "AUDIO")
        .and(rung_of(uri, rungs))
        .map_or(0, |r| u64::from(r.audio_kbps) * 1000)
}

/// A rung's peak and average bit rate, in bits per second, measured from its
/// own playlist: each segment's `#EXT-X-BYTERANGE` length over its `#EXTINF`
/// duration. `None` when the playlist lists no segment this can read.
///
/// The peak is RFC 8216's: the highest rate over any run of consecutive
/// segments lasting between half and one and a half target durations. A
/// single segment is not enough. The last segment is whatever the video has
/// left, and when that is a fraction of a second holding one keyframe, its
/// own rate is several times the rung's, which would advertise the rung to a
/// player far above what it costs. A video shorter than half a target
/// duration has no such run, and its peak is its average. Rounded up, so a
/// declared peak is never under the measured one.
pub fn segment_rates(rung_playlist: &str) -> Option<(u64, u64)> {
    let mut target: Option<f64> = None;
    let mut segments: Vec<(u64, f64)> = Vec::new();
    let mut duration: Option<f64> = None;
    for line in rung_playlist.lines() {
        if let Some(v) = line.strip_prefix("#EXT-X-TARGETDURATION:") {
            target = v.trim().parse().ok();
        } else if let Some(v) = line.strip_prefix("#EXTINF:") {
            duration = v.split(',').next().and_then(|d| d.trim().parse().ok());
        } else if let Some(v) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            let length: u64 = v.split('@').next()?.trim().parse().ok()?;
            segments.push((length, duration.take().filter(|d| *d > 0.0)?));
        }
    }
    let bytes: u64 = segments.iter().map(|s| s.0).sum();
    let seconds: f64 = segments.iter().map(|s| s.1).sum();
    if seconds <= 0.0 {
        return None;
    }
    let average = bytes as f64 * 8.0 / seconds;
    let target = target.unwrap_or_else(|| segments.iter().map(|s| s.1).fold(0.0, f64::max));
    let mut peak = 0f64;
    for start in 0..segments.len() {
        let (mut run_bytes, mut run_seconds) = (0u64, 0f64);
        for &(length, secs) in &segments[start..] {
            run_bytes += length;
            run_seconds += secs;
            if run_seconds > 1.5 * target {
                break;
            }
            if run_seconds >= 0.5 * target {
                peak = peak.max(run_bytes as f64 * 8.0 / run_seconds);
            }
        }
    }
    if peak == 0.0 {
        peak = average;
    }
    Some((peak.ceil() as u64, average.round() as u64))
}

/// The rung a variant describes, read off its playlist name (`v{i}.m3u8`, the
/// rung's position). The frame size cannot identify it: frames are fitted to
/// the source, so no table of sizes lists them.
fn rung_of<'a>(uri: &str, rungs: &'a [VideoRung]) -> Option<&'a VideoRung> {
    let i = uri.trim().strip_prefix('v')?.strip_suffix(".m3u8")?.parse::<usize>().ok()?;
    rungs.get(i)
}

/// An attribute field's key: `BANDWIDTH` for `#EXT-X-STREAM-INF:BANDWIDTH=1`
/// and for `BANDWIDTH=1` alike.
fn field_key(field: &str) -> Option<&str> {
    field.split_once('=').and_then(|(k, _)| k.rsplit(':').next())
}

/// One attribute's value, unquoted.
///
/// Attributes are comma-separated but a quoted value may itself hold a comma
/// (`CODECS="avc1.42c00c,mp4a.40.2"`), so splitting on commas yields fragments.
/// Matching the key exactly is enough to ignore them — a fragment carries no
/// `KEY=` this asks for — and it keeps `BANDWIDTH` from matching
/// `AVERAGE-BANDWIDTH`, which a substring search would.
pub(super) fn attr<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split(',')
        .find(|field| field_key(field) == Some(key))
        .and_then(|field| field.split_once('='))
        .map(|(_, v)| v.trim_matches('"'))
}

/// Set both bandwidth declarations on a variant line, adding
/// `AVERAGE-BANDWIDTH` after `BANDWIDTH` when the line has none and an average
/// is known. Every other attribute — a comma-bearing quoted one included — is
/// left byte-identical.
fn set_bitrates(line: &str, peak: u64, average: Option<u64>) -> String {
    let has_average = attr(line, "AVERAGE-BANDWIDTH").is_some();
    line.split(',')
        .map(|field| {
            let name = field.split_once('=').map_or("", |(k, _)| k);
            match (field_key(field), average) {
                (Some("BANDWIDTH"), Some(a)) if !has_average => format!("{name}={peak},AVERAGE-BANDWIDTH={a}"),
                (Some("BANDWIDTH"), _) => format!("{name}={peak}"),
                (Some("AVERAGE-BANDWIDTH"), Some(a)) => format!("{name}={a}"),
                _ => field.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}
