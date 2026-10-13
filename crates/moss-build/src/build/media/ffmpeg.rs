//! FFmpeg integration for video conversion.
//!
//! This module provides functionality for converting video files to web-optimized
//! formats using FFmpeg. It includes:
//! - System FFmpeg detection
//! - MOV to MP4 conversion (H.264)
//! - Thumbnail generation for video previews
//!
//! # Usage
//!
//! ```rust,ignore
//! use moss::build::media::ffmpeg::{FFmpegManager, VideoCompressionConfig};
//!
//! let ffmpeg = FFmpegManager::detect()?;
//! let config = VideoCompressionConfig::default();
//! ffmpeg.convert_to_mp4_with_config(Path::new("input.mov"), Path::new("output.mp4"), &config)?;
//! ```

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::build::assets::binary_resolver::{
    resolve_binary, ArchiveFormat, BinaryConfig, BinarySource, VersionCheck,
};
use moss_core::asset_paths::{self, video_ladder_rungs, VideoRung};
use crate::types::{content::ProjectStructure, runtime::ChildProcessRegistry};

/// Configuration for moss's web video rendition.
///
/// # What is NOT here
///
/// Delivery policy is not configuration. The sizes, frame rates, audio and
/// rate ceilings live in `asset_paths::VIDEO_LADDER`, one table that states
/// the floor, the rungs and the ceiling together, and the picture quality
/// every encode aims at under those ceilings is [`VIDEO_QUALITY_CRF`]. This
/// struct carries only what those cannot: the hosting-cost bound and how the
/// encoder is run.
///
/// `max_size_mb` bounds what the **site owner** has to host, and it is the one
/// input that can pull an encode *down* the ladder: a film long enough that no
/// higher rung fits the budget is delivered at a lower one rather than at an
/// arbitrary bitrate of its own. `hls_max_file_mb` is the same idea applied to
/// the OTHER delivery form this config drives — see its own doc for why the
/// HLS ladder needs a second, separate budget rather than reusing this one.
///
/// Before 2026-08-27 only the size budget existed, and the bitrate fell out of
/// it as a by-product: a 5.7-minute video was encoded at 2.15 Mbps purely
/// because 97 MB ÷ 340 s happens to equal that. Nothing about any viewer's
/// bandwidth entered the calculation, and a shorter clip got a *higher*
/// bitrate for the same reason. That is what made playback fail on throttled
/// links.
///
/// # References
/// - Rate Control: https://slhck.info/video/2017/03/01/rate-control.html
#[derive(Debug, Clone)]
pub struct VideoCompressionConfig {
    /// Maximum output file size in megabytes.
    /// Default: 100 MB (suitable for web delivery without CDN issues)
    pub max_size_mb: u32,

    /// Maximum size, in MiB, of any single file the HLS ladder writes.
    /// Default: 150 MiB, matching moss hosting's per-file cap.
    ///
    /// This is a SEPARATE budget from `max_size_mb`, not a reuse of it, because
    /// the two encodes have a different file shape. The progressive MP4 is one
    /// file for the whole video, so `max_size_mb` bounds it directly. The HLS
    /// ladder is `-hls_flags single_file`: every rung's video is its OWN file,
    /// and so is each audio rendition (`alo.m4s`, `ahi.m4s`) — a rung's file
    /// can reach its rate ceiling times the video's whole duration, which the width
    /// truncation `asset_paths::video_ladder_rungs` already applies cannot see.
    /// A 15-minute, 1280 px source whose every rung fit the width still wrote
    /// a 221 MB top-rung file and a 123 MB rung below it — hosting rejects any
    /// one of those files over its cap, independent of what the whole ladder
    /// adds up to. `asset_paths::video_ladder_rungs_within` is where this
    /// field is applied.
    pub hls_max_file_mb: u32,

    /// x264 encoding preset. Slower = better compression ratio.
    /// Options: ultrafast, superfast, veryfast, faster, fast, medium, slow, slower, veryslow
    pub preset: String,


    /// The share of `max_size_mb` a rung's rate ceiling may fill (0.0-1.0).
    /// Default: 0.97, leaving room for container overhead.
    pub target_fill_percentage: f64,

    /// Max x264 encoder threads. Default: half the logical cores (floor 2),
    /// leaving headroom for WebKit + the UI thread. Execution detail only —
    /// deliberately excluded from `to_params()` so the transform cache and
    /// video-set fingerprint stay machine-independent.
    pub encode_threads: u32,
}

impl Default for VideoCompressionConfig {
    fn default() -> Self {
        Self {
            max_size_mb: 100,
            hls_max_file_mb: 150,
            preset: "slow".to_string(),
            target_fill_percentage: 0.97,
            encode_threads: default_encode_threads(),
        }
    }
}

/// Half the logical cores, floor 2 — leaves headroom for WebKit + the UI.
pub(crate) fn default_encode_threads() -> u32 {
    let cores = std::thread::available_parallelism().map(|c| c.get()).unwrap_or(4) as u32;
    (cores / 2).max(2)
}

impl VideoCompressionConfig {
    /// Serialize compression config for transform cache storage.
    ///
    /// Used as a cache key in the content-addressed transform cache: same params
    /// produce the same output, so matching params means a cache hit.
    ///
    /// Inspired by Bazel's action cache where the action descriptor includes
    /// all configuration that affects the output. If any parameter changes
    /// (e.g., CRF 18 → CRF 23), the cache correctly misses and re-converts.
    ///
    /// `hls_max_file_mb` is deliberately absent: it affects only the HLS
    /// ladder, which is cached under its own `video/hls/*` transform names
    /// keyed by `hls::ladder_params` (which does carry it), never under the
    /// `video/mp4` name these params key. Folding it in here would bust the
    /// progressive MP4's cache on a config that never touches it.
    pub fn to_params(&self) -> serde_json::Value {
        serde_json::json!({
            "preset": self.preset,
            "max_size_mb": self.max_size_mb,
            // The delivery policy is a const table, not a field, so nothing in
            // this struct changes when a rung is edited — and every site would
            // keep serving renditions encoded under the old table. The table's
            // own contents are therefore part of the key.
            "ladder": asset_paths::video_ladder_fingerprint(),
            // 3: the top size is the source's own picture; 2 fitted it into
            // the table row below it, and 1 forced a fixed 16:9 frame.
            // Nothing in the table changed when these did, so the key must
            // say so.
            "frame": 3,
            // The quality every encode aims at. A file encoded to hit its
            // rung's rate instead carries no such field and is rebuilt once.
            "crf": VIDEO_QUALITY_CRF,
        })
    }
}

/// Result of video encoding operation.
///
/// Indicates which encoding path was taken and the resulting file size.
#[derive(Debug, Clone, PartialEq)]
pub enum EncodingResult {
    /// Video was encoded; `size_bytes` is the file written.
    Encoded { size_bytes: u64 },

    /// The source already satisfies the rung it would be encoded to, so no
    /// output was written and the caller ships the original bytes (see
    /// [`EncodePlan::KeepOriginal`] for why).
    KeptOriginal,
}

/// What ffprobe tells us about a source, and everything the delivery decision
/// needs. Gathered in ONE probe rather than three — the encode used to shell
/// out separately for duration and codec.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourceVideo {
    pub duration_secs: f64,
    /// Coded width and height, as stored (rotation applied: a phone clip
    /// recorded upright reports its upright size here).
    pub width: u32,
    pub height: u32,
    /// Sample (pixel) aspect ratio, width over height of one pixel; 1.0 when
    /// the source declares none. See [`SourceVideo::display`].
    pub sar: f64,
    pub fps: f64,
    /// Whether the source carries an audio stream at all. A ladder that maps
    /// `a:0` on a silent source fails the entire encode.
    pub has_audio: bool,
    /// Container bitrate in kbps — video plus audio plus overhead, i.e. what a
    /// viewer actually has to sustain. This is what a rung promises, so this
    /// is what decides whether a source already meets one
    /// ([`plan_video_encode`]).
    pub total_kbps: Option<f64>,
    /// A codec every target browser decodes, in a container they all open. The
    /// container alone does not prove this: HEVC-in-mp4 does not play in Firefox.
    pub web_playable: bool,
}

/// ffprobe's `sample_aspect_ratio` text ("64:45"); `0:1`, `N/A` or anything
/// unparseable means the source declares none, i.e. square pixels.
pub(crate) fn parse_sar(v: Option<&str>) -> f64 {
    v.and_then(|v| {
        let (n, d) = v.split_once(':')?;
        let (n, d) = (n.parse::<f64>().ok()?, d.parse::<f64>().ok()?);
        (n > 0.0 && d > 0.0).then_some(n / d)
    })
    .unwrap_or(1.0)
}

/// Whether a display-matrix rotation (degrees, either sign) turns the picture
/// a quarter turn, swapping its width and height.
pub(crate) fn is_quarter_turn(rotation: Option<&str>) -> bool {
    rotation
        .and_then(|v| v.parse::<f64>().ok())
        .is_some_and(|deg| (deg.rem_euclid(180.0) - 90.0).abs() < 1.0)
}

/// Whether the file can ship as-is: a container and video codec every target
/// browser opens, 8-bit 4:2:0 pixels (10-bit and HDR phone footage does not
/// play everywhere), and audio that is AAC or absent. Anything else is
/// re-encoded, which normalises it.
pub(crate) fn is_web_playable(
    container_ok: bool,
    video_codec: Option<&str>,
    pix_fmt: Option<&str>,
    audio_codecs: &[String],
) -> bool {
    container_ok
        && video_codec.is_some_and(|c| matches!(c, "h264" | "vp8" | "vp9" | "av1"))
        && pix_fmt.is_some_and(|p| matches!(p, "yuv420p" | "yuvj420p"))
        && audio_codecs.iter().all(|a| a == "aac")
}

impl SourceVideo {
    /// The shape the viewer sees: coded width stretched by the pixel aspect
    /// ratio, over the height. What a rung's box is compared against.
    pub fn display(&self) -> (u32, u32) {
        asset_paths::display_size(self.width, self.height, self.sar)
    }
}

/// How to encode (or not encode) a video.
///
/// Pure decision function — the negative-bitrate class of failure (a 2.5-hour
/// film squeezed into a 100MB target yields a bitrate below zero, which libx264
/// rejects at frame 0) cannot arise here, because a bitrate is never derived:
/// it is only ever chosen from `VIDEO_LADDER`.
#[derive(Debug, Clone, PartialEq)]
pub enum EncodePlan {
    /// Encode at this rung.
    Encode { rung: VideoRung },
    /// The source is already something every browser plays, no larger than
    /// the delivery ceiling and no heavier than its own size is allowed or
    /// the size budget holds. It plays everywhere within the rate a viewer
    /// was promised, so the source bytes ship: a re-encode might come out
    /// smaller, but it would cost the picture a generation for a saving no
    /// viewer needs.
    KeepOriginal,
}

/// Picks the rung a video is delivered at.
///
/// One decision, in one place, replacing two: a `needs_transcode` gate that
/// asked about **file size** and a bitrate derivation that asked about
/// **duration**. Neither was the right question. The right one is whether the
/// source already satisfies the delivery policy — and the policy is the ladder.
///
/// One thing pulls the choice below the source's own picture
/// (`video_top_rung`): the hosting size budget. If not even the bottom rung
/// fits the budget, the bottom rung ships anyway — an over-budget file that
/// plays beats a video nobody can watch, and the alternative, shipping the
/// multi-gigabyte original, is worse for the viewer *and* the bill.
///
/// The original is kept on what it costs, not on which box it happens to
/// match. It is judged against its own top rung and against the budget
/// directly, so a lean recording between two table sizes, or one long enough
/// that the budget would place a re-encode several sizes down, ships as the
/// file it already is. A 20-minute 1708x1080 screen recording at 220 kbps is
/// 33 MB; measured against the rung the budget picks for a 20-minute encode
/// (640x360) it was "too large" and was re-encoded to that size.
///
/// This is the progressive MP4's own decision, over ONE file that can weigh
/// up to `total_kbps * duration`. The HLS ladder's analogous truncation
/// (`asset_paths::video_ladder_rungs_within`, applied in `hls::produce_ladder`)
/// answers a different question — not "which single rung", but "which PREFIX
/// of rungs", because a ladder ships every surviving rung as its own file — so
/// it is not a call into this function and does not share `EncodePlan`.
pub fn plan_video_encode(source: &SourceVideo, config: &VideoCompressionConfig) -> EncodePlan {
    let budget_bytes =
        config.max_size_mb as f64 * 1024.0 * 1024.0 * config.target_fill_percentage;
    let budget_kbps = (budget_bytes * 8.0 / 1024.0) / source.duration_secs.max(0.001);

    let display = source.display();
    let rungs = video_ladder_rungs(display);
    let top = rungs[rungs.len() - 1];
    let ceiling = asset_paths::VIDEO_LADDER[asset_paths::VIDEO_LADDER.len() - 1];

    // Judged on what the viewer must sustain, in the same unit the budget
    // uses: the container total. Testing the video stream alone would keep a
    // 300 kbps picture wrapped in a 320 kbps audio track and call it a 365 kbps
    // rung. Frame rate counts too — the bottom rung is 15 fps because 30 fps at
    // that bitrate measured as a smear, so a 30 fps original is not that rung
    // however small its file is.
    let keep = source.web_playable
        && display.0 <= ceiling.width
        && display.1 <= ceiling.height
        && source.fps <= top.fps as f64 + 0.5
        && source
            .total_kbps
            .is_some_and(|kbps| kbps <= (top.total_kbps() as f64).min(budget_kbps));
    if keep {
        return EncodePlan::KeepOriginal;
    }

    let rung = rungs
        .iter()
        .rev()
        .find(|r| r.total_kbps() as f64 <= budget_kbps)
        .copied()
        .unwrap_or(rungs[0]);
    EncodePlan::Encode { rung }
}

/// The rung of the web file [`plan_video_encode`] decides on: the source's
/// own top when it is kept, the rung it is encoded at otherwise. An HLS
/// ladder is played instead of this file, so it must hold this rung.
pub(crate) fn progressive_rung(source: &SourceVideo, config: &VideoCompressionConfig) -> VideoRung {
    match plan_video_encode(source, config) {
        EncodePlan::KeepOriginal => asset_paths::video_top_rung(source.display()),
        EncodePlan::Encode { rung } => rung,
    }
}

/// Collects every video file from the project structure for the background
/// video worker.
///
/// Returns source paths relative to the project root — that is the whole of
/// what the worker needs. This used to return a struct carrying two more
/// fields: a `collection` derived from the folder name that nothing has ever
/// read, and a `source_oid` that was always empty here and whose doc claimed
/// it drove in-flight dedup, which it did not — the worker resolves its own
/// hash at `video.rs`. Both were kept alive only by tests asserting the
/// collector populated them.
///
/// The worker is the SOLE owner of video output: every video's bytes reach the
/// site through it, whether re-encoded or passed through, and
/// `copy_deferred_assets` handles no video at all. That ownership is what
/// removed the old `needs_transcode` flag. The flag asked "is this file big, or
/// is it a .mov?" here while the asset copier asked "is this file big?"
/// elsewhere; the two spellings of one policy disagreed on a sub-threshold
/// .mov, which shipped twice — once as a 31 MB original nothing linked to,
/// once as the 4 MB transcode the page actually referenced.
///
/// Whether to re-encode is not knowable from a directory listing, so it is no
/// longer asked here. `plan_video_encode` asks it once, at encode time, from
/// what ffprobe measured.
pub fn collect_videos_for_conversion(project: &ProjectStructure) -> Vec<String> {
    project.video_files.iter().map(|f| f.path.clone()).collect()
}


/// Returns the path to the moss binary directory (~/.moss/bin/).
///
/// Creates the directory if it doesn't exist.
pub fn get_moss_bin_dir() -> Result<PathBuf, String> {
    let bin_dir = crate::infra::home::moss_home()?.join("bin");

    // allow:raw_write ~/.moss/bin, not the build tree
    std::fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("Failed to create ~/.moss/bin: {}", e))?;

    Ok(bin_dir)
}

/// Returns a `BinaryConfig` for FFmpeg.
///
/// Defines download sources for macOS (arm64, x64) and Windows (x64).
/// Linux users should install FFmpeg via their package manager since
/// the Linux distribution uses tar.xz which is not supported by `binary_resolver`.
pub fn ffmpeg_binary_config() -> BinaryConfig {
    BinaryConfig {
        name: "ffmpeg".to_string(),
        binary_name: None,
        version_check: Some(VersionCheck {
            args: vec!["-version".to_string()],
            pattern: Some(r"ffmpeg version (\d+\.\d+(?:\.\d+)?)".to_string()),
        }),
        sources: HashMap::from([
            (
                "darwin-arm64".to_string(),
                BinarySource {
                    github: None,
                    direct_url: Some(
                        "https://evermeet.cx/ffmpeg/getrelease/zip".to_string(),
                    ),
                    sha256: None,
                    archive_format: Some(ArchiveFormat::Zip),
                },
            ),
            (
                "darwin-x64".to_string(),
                BinarySource {
                    github: None,
                    direct_url: Some(
                        "https://evermeet.cx/ffmpeg/getrelease/zip".to_string(),
                    ),
                    sha256: None,
                    archive_format: Some(ArchiveFormat::Zip),
                },
            ),
            (
                "windows-x64".to_string(),
                BinarySource {
                    github: None,
                    direct_url: Some(
                        "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
                            .to_string(),
                    ),
                    sha256: None,
                    archive_format: Some(ArchiveFormat::Zip),
                },
            ),
            // Linux tar.xz is not supported by binary_resolver (only tar.gz and zip).
            // Linux users should install FFmpeg via their package manager.
        ]),
        archive_layout: None,
        cache_dir: None,
        required_disk_space: None,
    }
}

/// Binary configuration for **ffprobe** — the media-probe tool moss uses to read
/// video duration and dimensions.
///
/// ffprobe ships from the same ffmpeg project but is distributed as its own
/// download. moss must own provisioning it separately: assuming a sibling next
/// to the ffmpeg binary is the root cause of a real site's video 404s — the
/// macOS evermeet `getrelease/zip` contains ffmpeg ONLY, so the derived ffprobe
/// path did not exist and conversion failed with `Failed to run ffprobe`.
///
/// - macOS: evermeet serves ffprobe as its own single-binary zip.
/// - Windows: the gyan "essentials" zip bundles both ffmpeg.exe and ffprobe.exe;
///   the resolver's zip extractor selects `ffprobe.exe` by the config `name`.
/// - Linux: omitted (package managers install both binaries co-located).
pub fn ffprobe_binary_config() -> BinaryConfig {
    BinaryConfig {
        name: "ffprobe".to_string(),
        binary_name: None,
        version_check: Some(VersionCheck {
            args: vec!["-version".to_string()],
            pattern: Some(r"ffprobe version (\d+\.\d+(?:\.\d+)?)".to_string()),
        }),
        sources: HashMap::from([
            (
                "darwin-arm64".to_string(),
                BinarySource {
                    github: None,
                    direct_url: Some(
                        "https://evermeet.cx/ffmpeg/getrelease/ffprobe/zip".to_string(),
                    ),
                    sha256: None,
                    archive_format: Some(ArchiveFormat::Zip),
                },
            ),
            (
                "darwin-x64".to_string(),
                BinarySource {
                    github: None,
                    direct_url: Some(
                        "https://evermeet.cx/ffmpeg/getrelease/ffprobe/zip".to_string(),
                    ),
                    sha256: None,
                    archive_format: Some(ArchiveFormat::Zip),
                },
            ),
            (
                "windows-x64".to_string(),
                BinarySource {
                    github: None,
                    // The gyan essentials zip contains both ffmpeg.exe and
                    // ffprobe.exe; the extractor picks ffprobe.exe by name.
                    direct_url: Some(
                        "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
                            .to_string(),
                    ),
                    sha256: None,
                    archive_format: Some(ArchiveFormat::Zip),
                },
            ),
            // Linux: install ffprobe via the system package manager (co-located
            // with ffmpeg, so the sibling lookup in resolve_ffprobe_path finds it).
        ]),
        archive_layout: None,
        cache_dir: None,
        required_disk_space: None,
    }
}

/// Platform filename for the ffprobe binary.
fn ffprobe_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "ffprobe.exe"
    } else {
        "ffprobe"
    }
}

/// Resolves the ffprobe binary path given the already-resolved ffmpeg path.
///
/// Resolution order:
/// 1. If `ffmpeg_bin_path` is absolute, the sibling `ffprobe` beside it — covers
///    Homebrew/system installs and a `~/.moss/bin` cache that already holds both.
/// 2. Otherwise (bare name from PATH, or no sibling), the shared binary
///    resolver: system PATH → `~/.moss/bin` cache → download from evermeet
///    (macOS) / gyan (Windows) via [`ffprobe_binary_config`].
fn resolve_ffprobe_path(ffmpeg_bin_path: &str) -> Result<String, String> {
    let ffmpeg = Path::new(ffmpeg_bin_path);
    if ffmpeg.is_absolute() {
        let sibling = ffmpeg.with_file_name(ffprobe_filename());
        if sibling.exists() {
            return Ok(sibling.to_string_lossy().into_owned());
        }
    }
    let resolution = resolve_binary(&ffprobe_binary_config(), None, true, None)?;
    Ok(resolution.path)
}

/// Copies a video file as-is when conversion fails or is unavailable.
///
/// This implements graceful degradation: when FFmpeg is unavailable or
/// conversion fails, we copy the original video to the output location.
/// The video will still play (browsers can handle MOV files), just without
/// optimization.
///
/// # Arguments
/// * `source` - Path to the source video file
/// * `output` - Path to the output location
///
/// # Returns
/// * `Ok(())` - File copied successfully
/// * `Err(String)` - Copy failed
pub fn copy_video_as_fallback(source: &Path, output: &Path) -> Result<(), String> {
    // `copy_output`: the destination is build output, so a cloud-evicted one
    // is discarded rather than materialized. Parent dirs included.
    crate::build::io_utils::copy_output(source, output)
        .map_err(|e| format!("Failed to copy video: {}", e))?;

    Ok(())
}

/// Parses FFmpeg's `time=HH:MM:SS.ff` progress field from a stderr line.
///
/// FFmpeg writes progress to stderr using carriage returns (`\r`), with lines like:
/// ```text
/// frame=  120 fps= 30 q=28.0 size=    1024kB time=00:01:30.50 bitrate= 2097.2kbits/s speed=1.5x
/// ```
///
/// # Returns
/// * Positive f64 — parsed time in seconds
/// * Negative f64 (`-1.0`) — no `time=` field found or value is malformed
fn parse_ffmpeg_time(line: &str) -> f64 {
    // Find "time=" in the line
    let Some((_, after_time)) = line.split_once("time=") else {
        return -1.0;
    };

    // Handle negative time (e.g., "time=-00:00:00.01")
    let (negative, time_str) = match after_time.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, after_time),
    };

    // Extract the time value (up to next space or end of string)
    let value = time_str.split_once(' ').map_or(time_str, |(v, _)| v);

    // Parse HH:MM:SS.ff format
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() != 3 {
        return -1.0;
    }

    let hours: f64 = match parts[0].parse() {
        Ok(v) => v,
        Err(_) => return -1.0,
    };
    let minutes: f64 = match parts[1].parse() {
        Ok(v) => v,
        Err(_) => return -1.0,
    };
    let seconds: f64 = match parts[2].parse() {
        Ok(v) => v,
        Err(_) => return -1.0,
    };

    let total = hours * 3600.0 + minutes * 60.0 + seconds;
    if negative {
        -total
    } else {
        total
    }
}

/// Maximum lines [`strip_ffmpeg_progress`] keeps, as a defensive bound
/// independent of the progress filter — insurance against some other chatty
/// line a future ffmpeg build adds that the `frame=`/`fps=` heuristic doesn't
/// recognise.
const MAX_STDERR_ERROR_LINES: usize = 200;

/// Strip ffmpeg's `\r`-delimited progress spam (`frame=… fps=… …`) out of a
/// captured stderr blob, keeping every other line.
///
/// ffmpeg overwrites its progress report in place with `\r`, not `\n`, so a
/// blob captured whole (see [`spawn_ffmpeg_streaming`]) carries hundreds of
/// `frame=…` updates run together on what looks like one enormous line. That
/// blob is embedded verbatim into the `Err(String)` a failed encode returns
/// — which is what a real failure showed eating the log-tail budget: the
/// actual diagnostic (`[mp4 @ …] Unable to re-open …`, `Error writing
/// trailer`, `Conversion failed!`, the libx264 stats block) was almost
/// entirely crowded out by the last encode's progress ticks.
pub(crate) fn strip_ffmpeg_progress(stderr: &str) -> String {
    let is_progress = |line: &str| line.contains("frame=") && line.contains("fps=");
    let kept: Vec<&str> = stderr
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|line| !line.is_empty() && !is_progress(line))
        .collect();
    let start = kept.len().saturating_sub(MAX_STDERR_ERROR_LINES);
    kept[start..].join("\n")
}

/// Niceness applied to all ffmpeg/ffprobe children (Unix). 10 ≈ "background
/// but not idle": yields to the UI under contention, full speed otherwise.
#[cfg(unix)]
const BACKGROUND_NICENESS: i32 = 10;

/// Build a `Command` for background media work at reduced OS priority, so
/// encodes never starve WebKit / the UI thread.
pub(crate) fn background_command(bin_path: &Path) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(bin_path);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setpriority is a single async-signal-safe syscall; the
        // closure allocates nothing. Errors ignored on purpose — a failed
        // renice must not fail the conversion.
        unsafe {
            cmd.pre_exec(|| {
                libc::setpriority(libc::PRIO_PROCESS, 0, BACKGROUND_NICENESS);
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::BELOW_NORMAL_PRIORITY_CLASS;
        cmd.creation_flags(BELOW_NORMAL_PRIORITY_CLASS);
    }
    cmd
}

/// Spawns an FFmpeg process with registry tracking and cancellation support.
///
/// This helper method implements the spawn+wait pattern for FFmpeg processes,
/// enabling proper cleanup on folder switch:
/// 1. Check cancellation flag before starting
/// 2. Spawn process (instead of blocking .output())
/// 3. Register PID with registry for tracking
/// 4. Wait for process completion
/// 5. Unregister PID after completion
/// 6. Detect SIGKILL (signal 9) and return "Cancelled" error
///
/// # Arguments
/// * `bin_path` - Path to the FFmpeg binary
/// * `args` - Command line arguments for FFmpeg
/// * `registry` - Optional process registry for PID tracking
/// * `cancel_flag` - Optional cancellation flag to check before starting
///
/// # Returns
/// * `Ok(Output)` - Process completed, includes stdout/stderr
/// * `Err("Cancelled")` - Process was cancelled or killed by signal 9
/// * `Err(String)` - Other errors (spawn/wait failed)
fn spawn_ffmpeg_tracked(
    bin_path: &Path,
    args: &[&str],
    registry: Option<&ChildProcessRegistry>,
    cancel_flag: Option<&AtomicBool>,
) -> Result<std::process::Output, String> {
    // Check cancellation before starting
    if cancel_flag.map_or(false, |f| f.load(Ordering::SeqCst)) {
        return Err("Cancelled".to_string());
    }

    let child = background_command(bin_path)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("FFmpeg spawn failed: {}", e))?;

    let pid = child.id();
    if let Some(r) = registry {
        r.register_pid(pid);
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("FFmpeg wait failed: {}", e))?;

    if let Some(r) = registry {
        r.unregister_pid(pid);
    }

    // Detect if killed by signal (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if output.status.signal() == Some(9) {
            return Err("Cancelled".to_string());
        }
    }

    Ok(output)
}

/// Spawns an FFmpeg process that streams stderr for real-time progress reporting.
///
/// Unlike `spawn_ffmpeg_tracked()` which blocks until completion, this function
/// reads FFmpeg's stderr output in real-time, parses `time=HH:MM:SS.ff` fields,
/// and calls `on_progress` with a fraction (0.0..1.0) representing encoding progress.
///
/// FFmpeg writes progress to stderr using `\r` (carriage return) rather than newlines,
/// so we read in chunks and split on both `\r` and `\n`.
///
/// # Arguments
/// * `bin_path` - Path to the FFmpeg binary
/// * `args` - Command line arguments for FFmpeg
/// * `duration_secs` - Total video duration in seconds (for computing fraction)
/// * `on_progress` - Callback receiving fraction 0.0..1.0 (throttled to 100ms min)
/// * `registry` - Optional process registry for PID tracking
/// * `cancel_flag` - Optional cancellation flag to check before starting
///
/// # Returns
/// * `Ok(Output)` - Process completed, includes stdout/stderr
/// * `Err("Cancelled")` - Process was cancelled or killed by signal 9
/// * `Err(String)` - Other errors (spawn/wait failed)
pub(crate) fn spawn_ffmpeg_streaming(
    bin_path: &Path,
    args: &[&str],
    duration_secs: f64,
    on_progress: &dyn Fn(f64),
    registry: Option<&ChildProcessRegistry>,
    cancel_flag: Option<&AtomicBool>,
) -> Result<std::process::Output, String> {
    // Check cancellation before starting
    if cancel_flag.map_or(false, |f| f.load(Ordering::SeqCst)) {
        return Err("Cancelled".to_string());
    }

    let mut child = background_command(bin_path)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("FFmpeg spawn failed: {}", e))?;

    let pid = child.id();
    if let Some(r) = registry {
        r.register_pid(pid);
    }

    // Take ownership of stderr for streaming reads
    let mut stderr_handle = child.stderr.take()
        .ok_or_else(|| "Failed to capture FFmpeg stderr".to_string())?;

    // Read stderr in chunks, parse time= fields for progress
    let mut all_stderr = Vec::new();
    let mut buf = [0u8; 4096];
    let mut remainder = String::new();
    let mut last_callback = std::time::Instant::now() - Duration::from_millis(100);

    loop {
        let n = stderr_handle.read(&mut buf)
            .map_err(|e| format!("Failed to read FFmpeg stderr: {}", e))?;
        if n == 0 {
            break; // EOF
        }

        all_stderr.extend_from_slice(&buf[..n]);

        // Append new data to remainder and process complete lines
        remainder.push_str(&String::from_utf8_lossy(&buf[..n]));

        // Split on \r or \n to find complete progress lines
        while let Some((line, rest)) = remainder
            .split_once(|c: char| c == '\r' || c == '\n')
            .map(|(line, rest)| (line.to_string(), rest.to_string()))
        {
            remainder = rest;

            if line.is_empty() {
                continue;
            }

            let time_secs = parse_ffmpeg_time(&line);
            if time_secs >= 0.0 && duration_secs > 0.0 {
                let fraction = (time_secs / duration_secs).min(1.0);
                let now = std::time::Instant::now();
                if now.duration_since(last_callback) >= Duration::from_millis(100) {
                    on_progress(fraction);
                    last_callback = now;
                }
            }
        }
    }

    // Process any remaining data in the buffer
    if !remainder.is_empty() {
        let time_secs = parse_ffmpeg_time(&remainder);
        if time_secs >= 0.0 && duration_secs > 0.0 {
            let fraction = (time_secs / duration_secs).min(1.0);
            on_progress(fraction);
        }
    }

    // Note: stdout is read after stderr EOF. This is safe because FFmpeg encoding
    // writes negligible stdout (output goes to file, progress goes to stderr).
    // Take stdout before wait (child.wait() doesn't capture stdout)
    let mut stdout_handle = child.stdout.take()
        .ok_or_else(|| "Failed to capture FFmpeg stdout".to_string())?;
    let mut all_stdout = Vec::new();
    stdout_handle.read_to_end(&mut all_stdout)
        .map_err(|e| format!("Failed to read FFmpeg stdout: {}", e))?;

    // Wait for process to exit
    let status = child.wait()
        .map_err(|e| format!("FFmpeg wait failed: {}", e))?;

    if let Some(r) = registry {
        r.unregister_pid(pid);
    }

    // Detect if killed by signal (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() == Some(9) {
            return Err("Cancelled".to_string());
        }
    }

    // Build Output struct matching what spawn_ffmpeg_tracked returns
    Ok(std::process::Output {
        status,
        stdout: all_stdout,
        stderr: all_stderr,
    })
}

/// Maximum frames between keyframes (`-g`).
///
/// x264's default is 250 — 8.3 s at 30 fps, which is what the pre-2026-08-27
/// encodes shipped. Every recovery a player makes from a stall or a seek
/// starts at the preceding keyframe, so on a slow link that default means up
/// to 8 s of video re-fetched and re-decoded before the first frame appears.
/// 60 frames is 2 s at 30 fps, matching the segment length HLS ladders are
/// normally cut at, and costs a few percent of bitrate.
///
/// Scene-cut keyframes are deliberately left ENABLED (no `-sc_threshold 0`):
/// this is a single progressive file, so nothing needs keyframes at fixed
/// offsets, and suppressing them only costs quality at every cut. That flag
/// belongs with a segmented ladder, where rungs must align.
const MAX_KEYFRAME_INTERVAL_FRAMES: &str = "60";

/// The picture quality every video encode aims at: libx264's constant rate
/// factor, where lower is better and larger.
///
/// A rung's `video_kbps` only caps it. Aiming at the rate instead spends it
/// whether or not the picture needs it, and how much a picture needs has
/// little to do with its size. A 91-second screen recording encoded at
/// 1708x1080 is 6.1 MB at this quality (527 kbps, SSIM 0.998 against the
/// source) and was 22.5 MB aimed at its rung's 4000 kbps; a clip of slowly
/// drifting gradients is 203 kbps here and 2026 aimed at a 2000 kbps rung.
/// Footage that does need the rate (a film scan measured 2990 kbps at 1080p, a
/// 4K camera clip ran into the 4000 kbps cap) is bounded by the cap as before.
///
/// 23 is libx264's own default. Measured at the `slow` preset against each
/// source scaled to the same frame, it kept SSIM at 0.9993 for the first half
/// minute of that screen recording and 0.994 for the film scan; 21 cost
/// 12-19% more bytes for a difference in the fourth decimal.
pub(crate) const VIDEO_QUALITY_CRF: u32 = 23;

/// The rate-control flags every video encode carries, for the progressive
/// file (`stream` empty) and for each ladder rung (`stream` like `:v:3`).
///
/// The encoder aims at [`VIDEO_QUALITY_CRF`], and `cap_kbps` is what it may
/// not exceed getting there. A picture that needs less spends less; one that
/// needs more is held to the rate a viewer was promised. `-maxrate` bounds
/// the sustained rate and `-bufsize` is how much short-window overshoot is
/// allowed: 2x is the knee, where 1x bought ~4% less peak for ~10% less
/// delivered bitrate. Without the pair a high-motion passage bursts far above
/// the rung (an uncapped 1489 kbps average measured a 3797 kbps peak), and a
/// viewer's buffer drains on the peak.
pub(crate) fn rate_control_args(stream: &str, cap_kbps: u32) -> [String; 6] {
    [
        format!("-crf{stream}"),
        VIDEO_QUALITY_CRF.to_string(),
        format!("-maxrate{stream}"),
        format!("{cap_kbps}k"),
        format!("-bufsize{stream}"),
        format!("{}k", cap_kbps.saturating_mul(2)),
    ]
}

/// Build the argument list for the progressive encode.
///
/// Pure function so tests can assert on the actual command line. The
/// `-threads` cap scopes the x264 encoder (it precedes the output target).
///
/// One pass. A second pass exists to hit an average bitrate exactly, and this
/// encode has no average to hit: it aims at a quality and is only bounded by
/// the rung's rate.
pub(crate) fn build_encode_args(
    input: &str,
    output: &str,
    rung: VideoRung,
    source_fps: f64,
    display: (u32, u32),
    config: &VideoCompressionConfig,
) -> Vec<String> {
    // Downsample the frame rate only when the source is faster than the rung
    // asks for. Forcing 30 on a 24 fps source would duplicate frames and spend
    // bitrate on them; MIN_BITS_PER_PIXEL_PER_FRAME is computed against the
    // rung's fps, so arriving under it is the safe direction.
    //
    // The frame is the source's display size fitted into the rung's box, as
    // explicit even numbers with square pixels, so a source of another shape
    // keeps it and a declared pixel aspect ratio is baked into the picture.
    let (w, h) = rung.fitted_frame(display);
    let vf = if source_fps > rung.fps as f64 {
        format!("scale={w}:{h},setsar=1,fps={}", rung.fps)
    } else {
        format!("scale={w}:{h},setsar=1")
    };
    let mut args: Vec<String> = vec![
        "-i".into(),
        input.into(),
        "-vf".into(),
        vf,
        "-c:v".into(),
        "libx264".into(),
        "-threads".into(),
        config.encode_threads.to_string(),
    ];
    args.extend(rate_control_args("", rung.video_kbps));
    args.extend([
        "-g".into(),
        MAX_KEYFRAME_INTERVAL_FRAMES.into(),
        // 8-bit 4:2:0 High profile. libx264 already picks these for an 8-bit
        // 4:2:0 source, so they change nothing for the common case — they are
        // insurance against the uncommon one. A 10-bit HDR clip straight off
        // an iPhone encodes to yuv420p10le by default, which Safari and most
        // TVs will not decode; the level is left for x264 to derive, because
        // pinning one can only raise it and a higher level excludes older
        // devices.
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-profile:v".into(),
        "high".into(),
        "-preset".into(),
        config.preset.clone(),
        "-c:a".into(),
        "aac".into(),
        "-ac".into(),
        rung.audio_channels.to_string(),
        "-b:a".into(),
        format!("{}k", rung.audio_kbps),
        "-movflags".into(),
        "+faststart".into(), // Enable progressive download
        "-y".into(),
        output.into(),
    ]);
    args
}

/// Outcome of one [`FFmpegManager::convert_to_mp4_with_config`] call, for the
/// encode-end log line. Narrower than `Result<EncodingResult, String>` on
/// purpose: this line is only ever logged once the source is known to need
/// encoding — the `KeepOriginal` branch returns before it, carrying its own
/// dedicated `log::info!` already — so the only success shape worth naming
/// here is the size shipped, with whether a lower-rung retry was needed
/// folded in as context rather than a fourth outcome variant.
pub(crate) enum EncodeOutcome<'a> {
    Success { size_bytes: u64, retried: bool },
    Failed(&'a str),
}

/// One INFO line naming the source and the rung it is about to be encoded
/// at — the "start" half of a start/end pair a shared, interleaved log can
/// correlate against wall-clock time.
pub(crate) fn encode_start_line(source: &Path, rung: VideoRung, display: (u32, u32)) -> String {
    let (w, h) = rung.fitted_frame(display);
    format!(
        "Encoding {} at {w}x{h}@{}fps (up to {} kbps)",
        source.display(),
        rung.fps,
        rung.video_kbps,
    )
}

/// The "end" half of the pair: elapsed wall time and how it went. `elapsed`
/// covers the whole attempt, including a retry at a lower rung when one
/// happened.
pub(crate) fn encode_end_line(source: &Path, elapsed: Duration, outcome: &EncodeOutcome) -> String {
    match outcome {
        EncodeOutcome::Success { size_bytes, retried } => format!(
            "Encoded {} in {:.1}s: {} bytes{}",
            source.display(),
            elapsed.as_secs_f64(),
            size_bytes,
            if *retried { " (after a retry at a lower rung)" } else { "" },
        ),
        EncodeOutcome::Failed(reason) => format!(
            "Encode failed for {} after {:.1}s: {}",
            source.display(),
            elapsed.as_secs_f64(),
            reason,
        ),
    }
}

/// Where to seek for a video's poster frame: half the clip's duration, capped
/// at 1 s. A fixed 1 s seek lands at or past EOF for anything shorter than
/// that, and ffmpeg turns an out-of-range seek into a 0-byte JPEG rather than
/// an error. `duration_secs <= 0.0` (an unknown or failed probe) seeks the
/// very first frame, which always exists.
///
/// Pure so the calculation can be tested without spawning ffmpeg.
pub(crate) fn thumbnail_seek_secs(duration_secs: f64) -> f64 {
    if duration_secs > 0.0 {
        (duration_secs / 2.0).min(1.0)
    } else {
        0.0
    }
}

/// The frame a hero poster is cut at: the video's display size fitted into the
/// top ladder rung's box, so at most 1920x1080 and never enlarged, with both
/// sides even like every other frame the ladder makes.
pub(crate) fn poster_frame(display: (u32, u32)) -> (u32, u32) {
    asset_paths::VIDEO_LADDER[asset_paths::VIDEO_LADDER.len() - 1].fitted_frame(display)
}

/// ffmpeg's `-q:v` for a hero poster (2 best, 31 worst). A 1920x1080 still
/// measured about 90 to 250 KB (footage and a grainy test pattern).
pub(crate) const POSTER_JPEG_QUALITY: &str = "5";

/// Manager for FFmpeg operations.
///
/// Provides methods for video conversion and thumbnail generation.
/// Uses system FFmpeg if available.
pub struct FFmpegManager {
    /// Path to the ffmpeg binary
    bin_path: String,
    /// Path to the ffprobe binary, resolved lazily on first probe and cached.
    ///
    /// ffprobe is a SEPARATE binary from ffmpeg and is NOT included in moss's
    /// downloaded ffmpeg (evermeet/gyan), so it cannot be assumed to sit beside
    /// every ffmpeg. Resolved on demand via [`resolve_ffprobe_path`] (which may
    /// download it) and cached here. `OnceLock` (not `Cell`/`OnceCell`) because
    /// a manager is shared by reference across the background conversion threads.
    ffprobe: OnceLock<String>,
}

impl FFmpegManager {
    /// Internal constructor — every public constructor funnels through here so
    /// the lazy ffprobe cache is initialised consistently.
    fn new(bin_path: String) -> Self {
        Self {
            bin_path,
            ffprobe: OnceLock::new(),
        }
    }

    /// Returns the path to the FFmpeg binary.
    pub fn bin_path(&self) -> &str {
        &self.bin_path
    }

    /// Returns the path to the ffprobe binary, resolving (and downloading if
    /// necessary) on first use and caching the result.
    ///
    /// See [`resolve_ffprobe_path`] for the resolution order. ffprobe is owned
    /// and provisioned by moss separately from ffmpeg — never assumed to be a
    /// sibling of the ffmpeg binary.
    fn ffprobe_path(&self) -> Result<PathBuf, String> {
        if let Some(p) = self.ffprobe.get() {
            return Ok(PathBuf::from(p));
        }
        // Serialize the first-time resolution across threads. The video pipeline
        // converts several videos concurrently (encode semaphore), all sharing
        // one FFmpegManager; without this lock each would independently call
        // resolve_binary and race the ffprobe DOWNLOAD — and the resolver writes
        // the single-file zip artifact to its final cache path non-atomically,
        // so concurrent writers could corrupt ~/.moss/bin/ffprobe. The lock makes
        // exactly one thread download; the rest hit the cache it just populated.
        static RESOLVE_LOCK: Mutex<()> = Mutex::new(());
        let _guard = RESOLVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Another thread may have resolved while we waited on the lock.
        if let Some(p) = self.ffprobe.get() {
            return Ok(PathBuf::from(p));
        }
        let resolved = resolve_ffprobe_path(&self.bin_path)?;
        // Errors are not cached, so a transient failure can retry next call.
        let _ = self.ffprobe.set(resolved.clone());
        Ok(PathBuf::from(resolved))
    }

    /// Create an FFmpegManager from a known binary path.
    ///
    /// Used when reconstructing an FFmpegManager inside a singleflight
    /// closure where the original reference can't be captured.
    pub fn from_bin_path(path: String) -> Self {
        Self::new(path)
    }

    /// Gets FFmpeg from system PATH, cache, or downloads it.
    ///
    /// Delegates to the unified binary resolver which tries in order:
    /// 1. System FFmpeg (in PATH)
    /// 2. Previously downloaded FFmpeg (~/.moss/bin/ffmpeg)
    /// 3. Downloads FFmpeg to ~/.moss/bin/
    ///
    /// # Returns
    /// * `Ok(FFmpegManager)` - Ready to convert videos
    /// * `Err(String)` - FFmpeg unavailable and download failed
    pub fn get_or_download(
        on_progress: Option<&crate::build::assets::download::DownloadProgress>,
    ) -> Result<Self, String> {
        let config = ffmpeg_binary_config();
        let resolution =
            crate::build::assets::binary_resolver::resolve_binary(&config, None, true, on_progress)?;
        Ok(Self::new(resolution.path))
    }

    /// Detects FFmpeg in the system PATH.
    ///
    /// Returns an FFmpegManager if ffmpeg is found, otherwise an error.
    #[allow(dead_code)]
    pub fn detect() -> Result<Self, String> {
        // Try to find ffmpeg in PATH
        let ffmpeg_path = if cfg!(target_os = "windows") {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };

        // Verify ffmpeg is available
        let output = background_command(Path::new(ffmpeg_path))
            .arg("-version")
            .output()
            .map_err(|_| {
                "FFmpeg not found in PATH. Please install FFmpeg to enable video conversion."
                    .to_string()
            })?;

        if !output.status.success() {
            return Err("FFmpeg found but failed to execute".to_string());
        }

        Ok(Self::new(ffmpeg_path.to_string()))
    }

    /// Encodes a video into moss's single web rendition.
    ///
    /// # Algorithm
    /// 1. Probe the source once.
    /// 2. `plan_video_encode` picks the rung: the best one the source fills
    ///    that the size budget allows, or `KeepOriginal` when the source
    ///    already plays everywhere and costs no more than its own size and
    ///    the budget allow.
    /// 3. One encode aimed at `VIDEO_QUALITY_CRF`, capped at the rung's rate.
    /// 4. Validate; on failure retry once, one rung down.
    ///
    /// # References
    /// - Rate Control: https://slhck.info/video/2017/03/01/rate-control.html
    ///
    /// # Arguments
    /// * `source` - Path to the source video file
    /// * `output` - Path to the output MP4 file
    /// * `config` - Compression configuration
    /// * `progress` - Optional callback for progress reporting (receives fraction 0.0..1.0)
    /// * `registry` - Optional process registry for PID tracking (enables cleanup on folder switch)
    /// * `cancel_flag` - Optional cancellation flag (checked before starting conversion)
    ///
    /// # Returns
    /// * `Ok(EncodingResult)` - Success with encoding method used
    /// * `Err("Cancelled")` - Conversion was cancelled
    /// * `Err(String)` - Encoding failed
    pub fn convert_to_mp4_with_config(
        &self,
        source: &Path,
        output: &Path,
        config: &VideoCompressionConfig,
        progress: Option<&dyn Fn(f64)>,
        registry: Option<&ChildProcessRegistry>,
        cancel_flag: Option<&AtomicBool>,
    ) -> Result<EncodingResult, String> {
        // Check cancellation before starting
        if cancel_flag.map_or(false, |f| f.load(Ordering::SeqCst)) {
            return Err("Cancelled".to_string());
        }

        let probe = self.probe_source(source)?;

        // Create output directory if needed
        if let Some(parent) = output.parent() {
            crate::build::io_utils::create_output_dir_all(parent)
                .map_err(|e| format!("Failed to create output directory: {}", e))?;
        }

        let rung = match plan_video_encode(&probe, config) {
            EncodePlan::KeepOriginal => {
                log::info!(
                    "Source already meets its delivery rung; keeping the original: {}",
                    source.display()
                );
                return Ok(EncodingResult::KeptOriginal);
            }
            EncodePlan::Encode { rung } => rung,
        };

        let top = asset_paths::video_top_rung(probe.display());
        if rung != top {
            let (w, h) = rung.fitted_frame(probe.display());
            let (tw, th) = top.fitted_frame(probe.display());
            log::info!(
                "{} is {:.0} minutes, so the {} MB budget places it at {w}x{h} rather than {tw}x{th}",
                source.display(),
                probe.duration_secs / 60.0,
                config.max_size_mb,
            );
        }

        // No-op progress callback when caller doesn't provide one
        let noop = |_: f64| {};
        let on_progress: &dyn Fn(f64) = match progress {
            Some(cb) => cb,
            None => &noop,
        };

        // One INFO line naming what's about to happen, one naming how it
        // went — a start/end pair a shared, interleaved log can correlate
        // against wall-clock time, independent of whatever the error text
        // itself says (that's `strip_ffmpeg_progress`'s job, above). The body
        // is an IIFE so every exit — the happy path and every early
        // `Err`/`?` below — reports through the ONE end-of-encode log call
        // rather than needing one hand-placed at each of the half-dozen
        // return sites.
        let encode_started_at = std::time::Instant::now();
        log::info!("{}", encode_start_line(source, rung, probe.display()));

        let outcome = (|| -> Result<(u64, bool), String> {
            self.run_encode(source, output, rung, &probe, config, on_progress, registry, cancel_flag)?;

            let mut retried = false;

            // Validate output
            if !self.validate_encoded_video(output)? {
                // Retry one rung down, not at 90% of this one. A rung is a
                // resolution/frame-rate/bitrate triple that clears the quality floor
                // together; shaving 10% off the bitrate alone leaves the other two
                // where they were and lands between rungs, below the floor. If there
                // is no rung below, there is nothing left to try.
                let Some(lower) = asset_paths::VIDEO_LADDER
                    .iter()
                    .rev()
                    .find(|r| r.video_kbps < rung.video_kbps)
                    .copied()
                else {
                    return Err(format!(
                        "Failed to encode valid video at the lowest rung: {}",
                        source.display()
                    ));
                };
                log::warn!(
                    "Validation failed at {}x{}; retrying at {}x{}: {}",
                    rung.width, rung.height, lower.width, lower.height, source.display()
                );
                retried = true;

                self.run_encode(source, output, lower, &probe, config, on_progress, registry, cancel_flag)?;

                if !self.validate_encoded_video(output)? {
                    return Err(format!("Failed to encode valid video after retry: {}", source.display()));
                }
            }

            let final_size = std::fs::metadata(output)
                .map_err(|e| format!("Failed to read output file size: {}", e))?
                .len();

            Ok((final_size, retried))
        })();

        let log_outcome = match &outcome {
            Ok((size_bytes, retried)) => {
                EncodeOutcome::Success { size_bytes: *size_bytes, retried: *retried }
            }
            Err(e) => EncodeOutcome::Failed(e),
        };
        log::info!("{}", encode_end_line(source, encode_started_at.elapsed(), &log_outcome));

        outcome.map(|(size_bytes, _retried)| EncodingResult::Encoded { size_bytes })
    }

    /// Run the one encode for `rung`, streaming ffmpeg's stderr for progress.
    #[allow(clippy::too_many_arguments)]
    fn run_encode(
        &self,
        input: &Path,
        output: &Path,
        rung: VideoRung,
        probe: &SourceVideo,
        config: &VideoCompressionConfig,
        on_progress: &dyn Fn(f64),
        registry: Option<&ChildProcessRegistry>,
        cancel_flag: Option<&AtomicBool>,
    ) -> Result<(), String> {
        let args = build_encode_args(
            input.to_str().ok_or("Invalid input path")?,
            output.to_str().ok_or("Invalid output path")?,
            rung,
            probe.fps,
            probe.display(),
            config,
        );
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        let out = spawn_ffmpeg_streaming(
            Path::new(&self.bin_path),
            &arg_refs,
            probe.duration_secs,
            on_progress,
            registry,
            cancel_flag,
        )?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!("Video encode failed: {}", strip_ffmpeg_progress(&stderr)));
        }

        Ok(())
    }

    /// Get video duration in seconds using ffprobe.
    ///
    /// ffprobe is the standard tool for media analysis, bundled with ffmpeg.
    /// It provides accurate duration without decoding the entire file.
    ///
    /// # Command
    /// ```bash
    /// ffprobe -v error -show_entries format=duration \
    ///         -of default=noprint_wrappers=1:nokey=1 input.mov
    /// ```
    /// Output: "123.456\n" (duration in seconds)
    ///
    /// # Arguments
    /// * `input` - Path to the video file
    ///
    /// # Returns
    /// * `Ok(f64)` - Duration in seconds
    /// * `Err(String)` - Error message if ffprobe fails
    /// Probe the first video stream's codec name (e.g. "h264", "hevc",
    /// "vp9"). Returns None when ffprobe fails — callers treat unknown as
    /// not-web-playable and fall back to encoding.
    /// Everything the delivery decision needs, in one ffprobe call.
    ///
    /// This replaced two separate probes (duration, then codec) plus a
    /// container-extension guess. The guess was the weakest of the three: an
    /// `.mp4` extension does not prove the codec inside plays everywhere —
    /// HEVC-in-mp4 does not play in Firefox — and neither proves the bitrate is
    /// one a viewer can stream, which is the question that actually decides
    /// whether to re-encode.
    pub fn probe_source(&self, input: &Path) -> Result<SourceVideo, String> {
        let ffprobe_path = self.ffprobe_path()?;
        let output = background_command(&ffprobe_path)
            .args([
                "-v", "error",
                // Every stream, not just `v:0`: whether the source HAS audio
                // decides the ladder's `-map a:0`, and mapping an audio stream
                // that is not there fails the whole encode.
                "-show_entries", "stream=codec_type,codec_name,pix_fmt,width,height,sample_aspect_ratio,r_frame_rate,bit_rate",
                // Display-matrix rotation (phone clips): ffmpeg turns the
                // picture upright when it decodes, so the shape to plan
                // against is the rotated one.
                "-show_entries", "stream_side_data=rotation",
                "-show_entries", "format=duration,bit_rate",
                // Section wrappers stay ON: `bit_rate` appears in BOTH the
                // stream and the format section, and they mean different
                // things. A flat scan cannot tell them apart.
                "-of", "default",
                input.to_str().ok_or("Invalid input path")?,
            ])
            .output()
            .map_err(|e| format!("Failed to run ffprobe: {}", e))?;
        if !output.status.success() {
            return Err(format!(
                "ffprobe failed for {}: {}",
                input.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let (streams_text, format_text) = text.split_once("[FORMAT]").unwrap_or((&text, ""));
        // One `[STREAM]` block per stream. The video fields are read from the
        // first video block rather than the first block, because a file whose
        // audio stream is listed first would otherwise report no width.
        let blocks: Vec<&str> = streams_text.split("[STREAM]").skip(1).collect();
        let is_kind = |b: &str, kind: &str| {
            b.lines().any(|l| l.trim() == format!("codec_type={kind}"))
        };
        let has_audio = blocks.iter().any(|b| is_kind(b, "audio"));
        let stream_text = blocks
            .iter()
            .find(|b| is_kind(b, "video"))
            .copied()
            .unwrap_or("");
        let field_in = |section: &str, key: &str| -> Option<String> {
            section
                .lines()
                .filter_map(|l| l.strip_prefix(key).and_then(|v| v.strip_prefix('=')))
                .map(str::trim)
                .find(|v| !v.is_empty() && *v != "N/A")
                .map(str::to_owned)
        };
        let stream = |key: &str| field_in(stream_text, key);
        let format = |key: &str| field_in(format_text, key);
        let kbps = |v: String| v.parse::<f64>().ok().map(|bps| bps / 1000.0);

        let duration_secs = format("duration")
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|d| *d > 0.0)
            .ok_or_else(|| format!("ffprobe reported no duration for {}", input.display()))?;

        let (mut width, mut height) = (
            stream("width").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0),
            stream("height").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0),
        );
        let sar = parse_sar(stream("sample_aspect_ratio").as_deref());
        let quarter_turn = is_quarter_turn(stream("rotation").as_deref());
        let sar = if quarter_turn { 1.0 / sar } else { sar };
        if quarter_turn {
            std::mem::swap(&mut width, &mut height);
        }
        // A video stream with no size cannot be planned or scaled: fitting a
        // zero side would "succeed" on a 2-pixel-tall frame.
        if width == 0 || height == 0 {
            return Err(format!("ffprobe reported no video size for {}", input.display()));
        }

        // "30000/1001" — a rational, not a decimal.
        let fps = stream("r_frame_rate")
            .and_then(|v| {
                let (n, d) = v.split_once('/')?;
                let (n, d) = (n.parse::<f64>().ok()?, d.parse::<f64>().ok()?);
                (d > 0.0).then_some(n / d)
            })
            .unwrap_or(30.0);

        // Missing only for a container that stores no overall figure; then the
        // source is not provably within any rung and gets re-encoded, which is
        // the safe direction — the other error ships an unstreamable video.
        // `> 0.0`, mirroring the `duration_secs` guard above: a zero or
        // negative figure is not a real bitrate, and treating it as one would
        // keep any web-playable source.
        let total_kbps = format("bit_rate").and_then(kbps).filter(|v| *v > 0.0);

        let container_ok = input
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_lowercase().as_str(), "mp4" | "m4v" | "webm"));
        let audio_codecs: Vec<String> = blocks
            .iter()
            .filter(|b| is_kind(b, "audio"))
            .filter_map(|b| field_in(b, "codec_name"))
            .collect();
        let web_playable = is_web_playable(
            container_ok,
            stream("codec_name").as_deref(),
            stream("pix_fmt").as_deref(),
            &audio_codecs,
        );

        Ok(SourceVideo {
            duration_secs,
            width,
            height,
            sar,
            fps,
            total_kbps,
            has_audio,
            web_playable,
        })
    }

    pub fn get_duration(&self, input: &Path) -> Result<f64, String> {
        let ffprobe_path = self.ffprobe_path()?;

        let output = background_command(&ffprobe_path)
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                input.to_str().ok_or("Invalid input path")?,
            ])
            .output()
            .map_err(|e| format!("Failed to run ffprobe: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("ffprobe failed: {}", stderr));
        }

        String::from_utf8(output.stdout)
            .map_err(|e| format!("Invalid ffprobe output: {}", e))?
            .trim()
            .parse::<f64>()
            .map_err(|e| format!("Failed to parse duration: {}", e))
    }

    /// Get video dimensions (width, height) from stream headers via ffprobe.
    ///
    /// Fast (~50ms) — reads only container metadata, not video frames.
    ///
    /// # Arguments
    /// * `input` - Path to the video file
    ///
    /// # Returns
    /// * `Ok((width, height))` - Video dimensions in pixels
    /// * `Err(String)` - Error message if ffprobe fails
    pub fn get_dimensions(&self, input: &Path) -> Result<(u32, u32), String> {
        let ffprobe_path = self.ffprobe_path()?;

        let output = background_command(&ffprobe_path)
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=p=0:s=x",
                input.to_str().ok_or("Invalid input path")?,
            ])
            .output()
            .map_err(|e| format!("Failed to run ffprobe: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("ffprobe failed: {}", stderr));
        }

        let stdout = String::from_utf8(output.stdout)
            .map_err(|e| format!("Invalid ffprobe output: {}", e))?;
        let parts: Vec<&str> = stdout.trim().split('x').collect();
        if parts.len() == 2 {
            let w = parts[0]
                .parse::<u32>()
                .map_err(|e| format!("Parse width: {}", e))?;
            let h = parts[1]
                .parse::<u32>()
                .map_err(|e| format!("Parse height: {}", e))?;
            Ok((w, h))
        } else {
            Err(format!("Unexpected ffprobe output: {}", stdout.trim()))
        }
    }

    /// Generates a thumbnail from a video, seeking to [`thumbnail_seek_secs`]
    /// of `video`'s own duration.
    ///
    /// # Arguments
    /// * `video` - Path to the video file
    /// * `output` - Path to the output thumbnail (JPEG)
    /// * `registry` - Optional process registry for PID tracking (enables cleanup on folder switch)
    /// * `cancel_flag` - Optional cancellation flag (checked before starting generation)
    ///
    /// # Returns
    /// * `Ok(true)` - Thumbnail was generated
    /// * `Ok(false)` - Thumbnail was skipped (output exists)
    /// * `Err("Cancelled")` - Generation was cancelled
    /// * `Err(String)` - Generation failed
    pub fn generate_thumbnail(
        &self,
        video: &Path,
        output: &Path,
        registry: Option<&ChildProcessRegistry>,
        cancel_flag: Option<&AtomicBool>,
    ) -> Result<bool, String> {
        self.extract_still(video, output, "scale=800:-1", &[], registry, cancel_flag)
    }

    /// Generates a hero's poster: the thumbnail's frame (same
    /// [`thumbnail_seek_secs`]) at the video's display size fitted into the
    /// top ladder rung's box ([`poster_frame`]), square pixels, JPEG at
    /// [`POSTER_JPEG_QUALITY`]. Returns as [`Self::generate_thumbnail`] does.
    pub fn generate_poster(
        &self,
        video: &Path,
        output: &Path,
        registry: Option<&ChildProcessRegistry>,
        cancel_flag: Option<&AtomicBool>,
    ) -> Result<bool, String> {
        // Cancelled before the probe, so a stale run does not spawn ffprobe.
        if cancel_flag.map_or(false, |f| f.load(Ordering::SeqCst)) {
            return Err("Cancelled".to_string());
        }
        let (w, h) = poster_frame(self.probe_source(video)?.display());
        let vf = format!("scale={w}:{h},setsar=1");
        self.extract_still(
            video,
            output,
            &vf,
            &["-q:v", POSTER_JPEG_QUALITY],
            registry,
            cancel_flag,
        )
    }

    /// One JPEG still of `video` at [`thumbnail_seek_secs`] of its duration,
    /// through the filter `vf`; `quality` is extra encoder arguments. The one
    /// owner of the seek, the pixel format and the 0-byte check, so a still
    /// that is not the thumbnail cannot drift from it.
    fn extract_still(
        &self,
        video: &Path,
        output: &Path,
        vf: &str,
        quality: &[&str],
        registry: Option<&ChildProcessRegistry>,
        cancel_flag: Option<&AtomicBool>,
    ) -> Result<bool, String> {
        // Check cancellation before starting
        if cancel_flag.map_or(false, |f| f.load(Ordering::SeqCst)) {
            return Err("Cancelled".to_string());
        }

        // Create output directory if needed
        if let Some(parent) = output.parent() {
            crate::build::io_utils::create_output_dir_all(parent).ok();
        }

        let video_str = video.to_str().ok_or("Invalid video path")?;
        let output_str = output.to_str().ok_or("Invalid output path")?;

        // `get_duration` is the same single-purpose ffprobe call
        // `validate_encoded_video` already uses; a failed probe falls back
        // to the very first frame, which always exists.
        let duration = self.get_duration(video).unwrap_or(0.0);
        let seek = format!("{:.3}", thumbnail_seek_secs(duration));

        let mut args: Vec<&str> = vec!["-i", video_str, "-ss", &seek, "-vframes", "1", "-vf", vf];
        args.extend_from_slice(quality);
        args.extend_from_slice(&[
            // JPEG is full-range YUV; without this the mjpeg encoder warns
            // "Non full-range YUV is non-standard" and some builds refuse a
            // testsrc-style source outright under strict compliance.
            "-pix_fmt",
            "yuvj420p",
            "-y",
            output_str,
        ]);

        let result = spawn_ffmpeg_tracked(
            Path::new(&self.bin_path),
            &args,
            registry,
            cancel_flag,
        )?;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            return Err(format!("Thumbnail generation failed: {}", stderr));
        }

        // ffmpeg can exit 0 while writing an empty file when the sought
        // frame never decodes to anything — the exact failure this function
        // exists to prevent, so a 0-byte file is a failure, not a result to
        // ship as a poster.
        match std::fs::metadata(output) {
            Ok(meta) if meta.len() > 0 => Ok(true),
            Ok(_) => Err(format!(
                "Thumbnail generation produced a 0-byte file: {}",
                output.display()
            )),
            Err(e) => Err(format!("Thumbnail generation did not produce a file: {}", e)),
        }
    }

    /// Validates that an encoded video is playable by checking its duration
    pub fn validate_encoded_video(&self, path: &Path) -> Result<bool, String> {
        if !path.exists() {
            return Ok(false);
        }

        // Use get_duration to check video properties
        match self.get_duration(path) {
            Ok(duration) => {
                // Video must have positive duration (at least 100ms for very short clips)
                Ok(duration > 0.1)
            }
            Err(_) => {
                // If we can't get duration, video is likely corrupted
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
/// Whether a real `ffmpeg` binary is on PATH. Shared by every test that needs
/// to synthesise a source rather than mock the encoder.
pub(crate) fn real_ffmpeg() -> Option<String> {
    let out = std::process::Command::new("ffmpeg").arg("-version").output().ok()?;
    out.status.success().then(|| "ffmpeg".to_string())
}
#[cfg(test)]
/// Synthesise a 4-second test video (with audio) at `size` (e.g. `"320x240"`).
pub(crate) fn synthesise(bin: &str, dest: &Path, size: &str) -> bool {
    std::process::Command::new(bin)
        .args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", &format!("testsrc=size={}:rate=30:duration=4", size),
            "-f", "lavfi", "-i", "sine=frequency=440:duration=4",
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-shortest",
        ])
        .arg(dest)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
#[cfg(test)]
/// Synthesise a short test video with NO audio stream at all — distinct from
/// a quiet one: [`probe_source`]'s `has_audio` is a stream-presence check,
/// not a loudness measurement, and this is the only synthesiser that leaves
/// it false.
pub(crate) fn synthesise_silent(bin: &str, dest: &Path, size: &str) -> bool {
    std::process::Command::new(bin)
        .args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", &format!("testsrc=size={}:rate=30:duration=4", size),
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
        ])
        .arg(dest)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
#[cfg(test)]
/// Synthesise a short test video whose audio stream is present but carries
/// silence (`anullsrc`, not [`synthesise`]'s `sine`) — the source `has_audio`
/// must still read true for, unlike [`synthesise_silent`]'s no-stream-at-all
/// case.
pub(crate) fn synthesise_with_silent_audio_track(bin: &str, dest: &Path, size: &str) -> bool {
    std::process::Command::new(bin)
        .args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", &format!("testsrc=size={}:rate=30:duration=4", size),
            "-f", "lavfi", "-i", "anullsrc=r=44100:cl=mono:duration=4",
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-shortest",
        ])
        .arg(dest)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
#[cfg(test)]
#[path = "ffmpeg_tests.rs"]
mod tests;
