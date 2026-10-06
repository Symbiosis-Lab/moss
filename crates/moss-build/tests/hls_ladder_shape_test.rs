//! What only real ffmpeg can answer about the HLS ladder.
//!
//! The unit tests in `hls_tests.rs` assert the command line; they cannot see
//! whether ffmpeg accepts it, how many files it writes, or whether the rungs
//! segment on the same boundaries. Those are the facts the ladder's
//! correctness rests on, and every one of them was a real finding:
//!
//! - Two `agroup`s in one `-var_stream_map` compose at all.
//! - The file count is `2N + 2G + 1` — 17 for six rungs and two renditions.
//! - `hls_members()` is the truth: exactly the files that appear on disk, and
//!   none of them — nor anything inside them — mentions the video's name.
//! - Every rung segments on the same boundary. With a flat `-g 60` the 15 fps
//!   bottom rung segmented at 8.0s against 6.0s elsewhere, because 60 frames
//!   is four seconds at 15 fps. That misalignment is what this guards.
//!
//! Skipped when ffmpeg is absent rather than failing: the encoder is an
//! optional runtime dependency moss downloads, not a build requirement.

use moss_build::build::media::hls::{build_hls_args, patch_master};
use moss_core::asset_paths::{hls_members, VIDEO_LADDER};

fn ffmpeg() -> Option<String> {
    let out = std::process::Command::new("ffmpeg").arg("-version").output().ok()?;
    out.status.success().then(|| "ffmpeg".to_string())
}

/// A tiny synthetic source: 8 s of 1280x720 colour bars with a tone, which is
/// wide enough for the whole ladder and long enough to produce two segments.
fn make_source(dir: &std::path::Path, bin: &str) -> Option<std::path::PathBuf> {
    let src = dir.join("src.mp4");
    let ok = std::process::Command::new(bin)
        .args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", "testsrc=size=1280x720:rate=30:duration=8",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=8",
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-shortest",
        ])
        .arg(&src)
        .status()
        .ok()?;
    ok.success().then_some(src)
}

#[test]
fn the_ladder_encodes_and_hls_members_names_exactly_what_lands_on_disk() {
    let Some(bin) = ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let Some(src) = make_source(dir.path(), &bin) else {
        eprintln!("skipping: could not synthesise a source");
        return;
    };

    let out_dir = dir.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let args = build_hls_args(
        src.to_str().unwrap(),
        out_dir.to_str().unwrap(),
        &VIDEO_LADDER,
        30.0,
        true,
        "ultrafast",
        2,
    );
    let status = std::process::Command::new(&bin)
        .args(["-hide_banner", "-loglevel", "error"])
        .args(&args)
        .status()
        .expect("run ffmpeg");
    assert!(status.success(), "ffmpeg rejected the ladder arguments");

    let mut on_disk: Vec<String> = std::fs::read_dir(&out_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    on_disk.sort();

    let mut promised: Vec<String> = hls_members(&VIDEO_LADDER);
    promised.sort();

    assert_eq!(
        on_disk, promised,
        "every file ffmpeg wrote is one hls_members promised, and vice versa — \
         a mismatch means the registry promises a URL that 404s, or a written \
         file nothing links to"
    );
    assert_eq!(on_disk.len(), 17, "6 rungs x2 + 2 renditions x2 + master");

    // The ladder is cached by content and relinked under whatever name the
    // video has at build time, including a name it did not have when it was
    // encoded. That only survives a rename because nothing in these bytes
    // names the video — the directory does, and the directory is not read.
    for name in &on_disk {
        if !name.ends_with(".m3u8") {
            continue;
        }
        let text = std::fs::read_to_string(out_dir.join(name)).unwrap();
        assert!(
            !text.contains("src"),
            "{name} names the source video; a cached ladder relinked under a \
             renamed video would reference files that do not exist:\n{text}"
        );
    }
}

#[test]
fn every_rung_segments_on_the_same_boundary_despite_mixed_frame_rates() {
    let Some(bin) = ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let Some(src) = make_source(dir.path(), &bin) else {
        eprintln!("skipping: could not synthesise a source");
        return;
    };
    let out_dir = dir.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let args = build_hls_args(
        src.to_str().unwrap(),
        out_dir.to_str().unwrap(),
        &VIDEO_LADDER,
        30.0,
        true,
        "ultrafast",
        2,
    );
    assert!(std::process::Command::new(&bin)
        .args(["-hide_banner", "-loglevel", "error"])
        .args(&args)
        .status()
        .expect("run ffmpeg")
        .success());

    // First-segment duration per rung. The bottom rung is 15 fps and every
    // other rung is 30; if the keyframe cadence were expressed in frames these
    // would differ by seconds.
    let first_of = |name: &str| -> f64 {
        let text = std::fs::read_to_string(out_dir.join(name)).unwrap();
        text.lines()
            .find_map(|l| l.strip_prefix("#EXTINF:"))
            .and_then(|v| v.trim_end_matches(',').parse::<f64>().ok())
            .unwrap_or_else(|| panic!("no #EXTINF in {name}"))
    };
    let base = first_of("v1.m3u8");
    for i in 0..VIDEO_LADDER.len() {
        let d = first_of(&format!("v{i}.m3u8"));
        assert!(
            (d - base).abs() < 0.25,
            "rung {i} segments at {d}s against {base}s — boundaries must line \
             up for a player to switch rungs cleanly"
        );
    }
}

#[test]
fn the_master_playlist_declares_the_lowest_rung_first_and_binds_each_group() {
    let Some(bin) = ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let Some(src) = make_source(dir.path(), &bin) else {
        eprintln!("skipping: could not synthesise a source");
        return;
    };
    let out_dir = dir.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let args = build_hls_args(
        src.to_str().unwrap(),
        out_dir.to_str().unwrap(),
        &VIDEO_LADDER,
        30.0,
        true,
        "ultrafast",
        2,
    );
    assert!(std::process::Command::new(&bin)
        .args(["-hide_banner", "-loglevel", "error"])
        .args(&args)
        .status()
        .expect("run ffmpeg")
        .success());

    let raw = std::fs::read_to_string(out_dir.join("master.m3u8")).unwrap();
    let master = patch_master(&raw, &VIDEO_LADDER);

    assert!(
        master.contains("#EXT-X-INDEPENDENT-SEGMENTS"),
        "ffmpeg does not emit this for the fmp4 + rendition-group shape, so \
         the post-pass must"
    );

    // Two groups, and each variant naming the one it can pay for.
    assert!(master.contains(r#"GROUP-ID="group_alo""#));
    assert!(master.contains(r#"GROUP-ID="group_ahi""#));

    let variants: Vec<&str> = master
        .lines()
        .filter(|l| l.starts_with("#EXT-X-STREAM-INF"))
        .collect();
    assert_eq!(variants.len(), VIDEO_LADDER.len(), "no spurious audio-only variant");
    assert!(
        variants[0].contains("RESOLUTION=320x180") && variants[0].contains("group_alo"),
        "lowest rung first, on the lean rendition — a starved viewer who \
         starts on 480p sees nothing at all: {}",
        variants[0]
    );
    assert!(
        variants[1].contains("RESOLUTION=416x234") && variants[1].contains("group_alo"),
        "145k + 192k audio would be 370 kbps: {}",
        variants[1]
    );
    assert!(variants[5].contains("RESOLUTION=1280x720") && variants[5].contains("group_ahi"));

    // Apple requires a cellular-delivered multivariant playlist to carry a
    // variant peaking at or below 192 kbit/s.
    let declared = |line: &str| -> u32 {
        line.split(',')
            .find_map(|f| {
                let (k, v) = f.split_once('=')?;
                (k.rsplit(':').next()? == "BANDWIDTH").then(|| v.parse().ok())?
            })
            .expect("BANDWIDTH on the variant")
    };
    let bw = declared(variants[0]);
    assert!(bw <= 192_000, "bottom rung declares {bw} bps, over Apple's cellular ceiling");

    // ...and that ceiling only means something if the figure is the whole
    // price. ffmpeg measures a variant from its video segments alone and omits
    // the rendition it names, which an upper bound can never catch: an
    // under-declared variant passes by being wrong in the safe direction, and
    // then over-selects on the link this ladder exists for.
    let raw_variants: Vec<&str> =
        raw.lines().filter(|l| l.starts_with("#EXT-X-STREAM-INF")).collect();
    for (i, rung) in VIDEO_LADDER.iter().enumerate() {
        assert_eq!(
            declared(variants[i]),
            declared(raw_variants[i]) + rung.audio_kbps * 1000,
            "rung {i} must advertise the {} kbps rendition it plays, not just its picture",
            rung.audio_kbps
        );
    }
}

/// The runner writes exactly what it reports, and patches the master.
#[test]
fn encode_ladder_writes_its_census_and_patches_the_master() {
    let Some(bin) = ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let Some(src) = make_source(dir.path(), &bin) else {
        eprintln!("skipping: could not synthesise a source");
        return;
    };

    let mgr = moss_build::build::media::ffmpeg::FFmpegManager::from_bin_path(bin.clone());
    let probe = mgr.probe_source(&src).expect("probe");
    assert!(probe.has_audio, "the synthesised source has a tone");
    assert!(
        probe.video_kbps.is_some(),
        "ffprobe reports the encoded video stream's own bit_rate, not just the container total"
    );

    let out_dir = dir.path().join("out");
    let written = moss_build::build::media::hls::encode_ladder(
        std::path::Path::new(&bin),
        &src,
        &out_dir,
        moss_core::asset_paths::video_ladder_rungs(probe.width),
        &probe,
        &moss_build::build::media::ffmpeg::VideoCompressionConfig::default(),
        None,
        None,
        None,
    )
    .expect("encode_ladder");

    let mut on_disk: Vec<String> = std::fs::read_dir(&out_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    on_disk.sort();
    let mut reported: Vec<String> = written
        .iter()
        .map(|p| p.rsplit('/').next().unwrap().to_string())
        .collect();
    reported.sort();
    assert_eq!(on_disk, reported, "the runner reports exactly what it wrote");

    let master = std::fs::read_to_string(out_dir.join("master.m3u8")).unwrap();
    assert!(
        master.contains("#EXT-X-INDEPENDENT-SEGMENTS"),
        "the master is patched on the way out"
    );
}
