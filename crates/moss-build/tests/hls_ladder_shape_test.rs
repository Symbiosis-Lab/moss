//! What only real ffmpeg can answer about the HLS ladder.
//!
//! The unit tests in `hls_tests.rs` assert the command line; they cannot see
//! whether ffmpeg accepts it, how many files it writes, or whether the rungs
//! segment on the same boundaries. Those are the facts the ladder's
//! correctness rests on, and every one of them was a real finding:
//!
//! - Two `agroup`s in one `-var_stream_map` compose at all.
//! - The file count is `2N + 2G + 1` — 17 for the six rungs a 1280x720 source gets (the
//!   1080p row is offered only to a source that fills it) and two renditions.
//! - `hls_members()` is the truth: exactly the files that appear on disk, and
//!   none of them — nor anything inside them — mentions the video's name.
//! - Every rung segments on the same boundary. With a flat `-g 60` the 15 fps
//!   bottom rung segmented at 8.0s against 6.0s elsewhere, because 60 frames
//!   is four seconds at 15 fps. That misalignment is what this guards.
//!
//! Skipped when ffmpeg is absent rather than failing: the encoder is an
//! optional runtime dependency moss downloads, not a build requirement.

use moss_build::build::media::hls::{build_hls_args, patch_master, segment_rates};
use moss_core::asset_paths::{hls_members, video_ladder_rungs, VideoRung};

/// The ladder a 1280x720 source gets: the synthetic source below stops at the
/// 720p box, so the 1080p row is not offered to it.
fn ladder_720() -> &'static [VideoRung] {
    video_ladder_rungs((1280, 720))
}

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
        ladder_720(),
        (1280, 720),
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

    let mut promised: Vec<String> = hls_members(ladder_720());
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
        ladder_720(),
        (1280, 720),
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
    for i in 0..ladder_720().len() {
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
        ladder_720(),
        (1280, 720),
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
    let master = patch_master(&raw, ladder_720(), |_| None);

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
    assert_eq!(variants.len(), ladder_720().len(), "no spurious audio-only variant");
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
    for (i, rung) in ladder_720().iter().enumerate() {
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
        moss_core::asset_paths::video_ladder_rungs(probe.display()),
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

/// A 3:2 source of 3 s, optionally declaring a pixel aspect ratio.
fn make_non_16x9_source(dir: &std::path::Path, bin: &str, vf: &str, name: &str) -> Option<std::path::PathBuf> {
    let src = dir.join(name);
    let ok = std::process::Command::new(bin)
        .args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", "testsrc=size=1200x800:rate=30:duration=3",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=3",
            "-vf", vf,
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-shortest",
        ])
        .arg(&src)
        .status()
        .ok()?;
    ok.success().then_some(src)
}

/// What ffprobe reports for a rung file: (width, height, sample aspect ratio).
fn probe_rung(path: &std::path::Path) -> (u32, u32, String) {
    let out = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0",
               "-show_entries", "stream=width,height,sample_aspect_ratio",
               "-of", "csv=p=0"])
        .arg(path)
        .output()
        .expect("run ffprobe");
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let mut f = text.split(',');
    let w = f.next().unwrap().parse().unwrap();
    let h = f.next().unwrap().parse().unwrap();
    (w, h, f.next().unwrap_or("").to_string())
}

/// Every rung of a ladder built from a non-16:9 source keeps the source's
/// display shape with square pixels, and the master's `RESOLUTION` is the
/// frame the rung file really has. Run once with square pixels and once with
/// a source that declares a non-square pixel aspect ratio.
#[test]
fn rungs_keep_the_sources_shape_with_square_pixels() {
    let Some(bin) = ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let cases = [
        // coded 1200x800 square pixels: 3:2
        ("setsar=1", "square.mp4", 1.5),
        // coded 1200x800 declaring 3:4 pixels: displayed 900x800, 9:8
        ("setsar=3/4", "anamorphic.mp4", 1.125),
    ];
    for (vf, name, display_aspect) in cases {
        let Some(src) = make_non_16x9_source(dir.path(), &bin, vf, name) else {
            eprintln!("skipping: could not synthesise a source");
            return;
        };
        let mgr = moss_build::build::media::ffmpeg::FFmpegManager::from_bin_path(bin.clone());
        let probe = mgr.probe_source(&src).expect("probe");
        let (dw, dh) = probe.display();
        assert!((dw as f64 / dh as f64 - display_aspect).abs() < 0.01, "{name}: display {dw}x{dh}");

        let out_dir = dir.path().join(format!("out-{name}"));
        let rungs = moss_core::asset_paths::video_ladder_rungs(probe.display());
        moss_build::build::media::hls::encode_ladder(
            std::path::Path::new(&bin),
            &src,
            &out_dir,
            rungs,
            &probe,
            &moss_build::build::media::ffmpeg::VideoCompressionConfig::default(),
            None,
            None,
            None,
        )
        .expect("encode_ladder");

        let master = std::fs::read_to_string(out_dir.join("master.m3u8")).unwrap();
        let resolutions: Vec<&str> = master
            .lines()
            .filter(|l| l.starts_with("#EXT-X-STREAM-INF"))
            .map(|l| l.split("RESOLUTION=").nth(1).unwrap().split(',').next().unwrap())
            .collect();
        assert_eq!(resolutions.len(), rungs.len());
        for (i, rung) in rungs.iter().enumerate() {
            let (w, h) = rung.fitted_frame(probe.display());
            assert_eq!(resolutions[i], format!("{w}x{h}"), "{name}: master RESOLUTION for rung {i}");

            // The rung's init segment sits in v{i}.m4s; ffprobe reads it.
            let (pw, ph, sar) = probe_rung(&out_dir.join(format!("v{i}.m4s")));
            assert_eq!((pw, ph), (w, h), "{name}: rung {i} file frame");
            assert!(sar == "1:1" || sar == "N/A" || sar.is_empty(), "{name}: rung {i} SAR {sar}");
            assert!(
                (pw as f64 / ph as f64 - display_aspect).abs() < 2.0 / ph as f64 + 0.01,
                "{name}: rung {i} is {pw}x{ph}, not {display_aspect}:1"
            );
            assert!(pw <= rung.width && ph <= rung.height);
        }
    }
}

/// A phone clip is stored sideways with a display matrix; ffmpeg turns it
/// upright when it decodes, so the shape to plan against is the turned one.
#[test]
fn a_rotated_source_is_probed_at_its_upright_shape() {
    let Some(bin) = ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let Some(src) = make_non_16x9_source(dir.path(), &bin, "setsar=1", "flat.mp4") else {
        eprintln!("skipping: could not synthesise a source");
        return;
    };
    let turned = dir.path().join("turned.mp4");
    let ok = std::process::Command::new(&bin)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-display_rotation", "90", "-i"])
        .arg(&src)
        .args(["-c", "copy"])
        .arg(&turned)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("skipping: this ffmpeg cannot write a display matrix");
        return;
    }
    let mgr = moss_build::build::media::ffmpeg::FFmpegManager::from_bin_path(bin.clone());
    let probe = mgr.probe_source(&turned).expect("probe");
    assert_eq!(probe.display(), (800, 1200));
}

/// A source that fills the 1080p box gets a seventh rung delivered at its own
/// size, with square pixels.
#[test]
fn a_1080p_source_gets_a_1920x1080_top_rung() {
    let Some(bin) = ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("hd.mp4");
    let made = std::process::Command::new(&bin)
        .args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", "testsrc=size=1920x1080:rate=30:duration=3",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=3",
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-shortest",
        ])
        .arg(&src)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !made {
        eprintln!("skipping: could not synthesise a source");
        return;
    }
    let mgr = moss_build::build::media::ffmpeg::FFmpegManager::from_bin_path(bin.clone());
    let probe = mgr.probe_source(&src).expect("probe");
    let rungs = video_ladder_rungs(probe.display());
    assert_eq!(rungs.len(), moss_core::asset_paths::VIDEO_LADDER.len());
    let out_dir = dir.path().join("out");
    moss_build::build::media::hls::encode_ladder(
        std::path::Path::new(&bin),
        &src,
        &out_dir,
        rungs,
        &probe,
        &moss_build::build::media::ffmpeg::VideoCompressionConfig::default(),
        None,
        None,
        None,
    )
    .expect("encode_ladder");
    let top = rungs.len() - 1;
    assert_eq!(probe_rung(&out_dir.join(format!("v{top}.m4s"))), (1920, 1080, "1:1".to_string()));
    let master = std::fs::read_to_string(out_dir.join("master.m3u8")).unwrap();
    assert!(master.contains("RESOLUTION=1920x1080"), "{master}");
}

/// A file with no video stream has no size to plan against; the probe fails
/// like any other failed probe instead of handing an encode a zero side.
#[test]
fn a_probe_of_a_file_with_no_video_size_is_an_error() {
    let Some(bin) = ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let audio = dir.path().join("tone.m4a");
    let made = std::process::Command::new(&bin)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-c:a", "aac"])
        .arg(&audio)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !made {
        eprintln!("skipping: could not synthesise a source");
        return;
    }
    let mgr = moss_build::build::media::ffmpeg::FFmpegManager::from_bin_path(bin.clone());
    let err = mgr.probe_source(&audio).expect_err("no video size");
    assert!(err.contains("no video size"), "{err}");
}

/// A rung's rate is a ceiling, and the master playlist advertises what the
/// rung actually costs. A player switches on that figure, so a lean rung
/// declared at its ceiling would be refused by a link that could carry it.
/// True on any ffmpeg: before 7.1 it writes the ceiling plus a tenth, and the
/// declaration is replaced with what the rung's segments measure.
#[test]
fn a_simple_picture_is_advertised_at_what_it_costs_not_at_its_rungs_rate() {
    let Some(bin) = ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    // Slowly drifting gradients: an encoder told to spend a rate on them can,
    // and one told to reach a quality needs a small fraction of it.
    let src = dir.path().join("drift.mp4");
    let made = std::process::Command::new(&bin)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi"])
        .args(["-i", "gradients=size=1280x720:rate=30:speed=0.01", "-t", "8"])
        .args(["-c:v", "libx264", "-crf", "12", "-pix_fmt", "yuv420p"])
        .arg(&src)
        .status()
        .is_ok_and(|s| s.success());
    if !made {
        eprintln!("skipping: could not synthesise a source");
        return;
    }
    let out_dir = dir.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let rungs = ladder_720();
    let args = build_hls_args(
        src.to_str().unwrap(),
        out_dir.to_str().unwrap(),
        rungs,
        (1280, 720),
        30.0,
        false,
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
    let master = patch_master(&raw, rungs, |playlist| {
        segment_rates(&std::fs::read_to_string(out_dir.join(playlist)).ok()?)
    });
    let top = rungs.len() - 1;
    let lines: Vec<&str> = master.lines().collect();
    let declared: u64 = lines
        .iter()
        .zip(lines.iter().skip(1))
        .find(|(_, uri)| uri.trim() == format!("v{top}.m3u8"))
        .and_then(|(inf, _)| inf.split(',').find_map(|f| f.strip_prefix("#EXT-X-STREAM-INF:BANDWIDTH=")))
        .and_then(|v| v.parse().ok())
        .expect("the top variant declares BANDWIDTH first");
    let ceiling = u64::from(rungs[top].video_kbps) * 1000;
    assert!(
        declared < ceiling / 2,
        "the top rung is advertised at {declared} bps against a {ceiling} bps ceiling"
    );
}
