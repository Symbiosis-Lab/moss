//! What only real ffmpeg can answer about a hero's poster still.
//!
//! The unit tests assert the arithmetic (`poster_frame`) and the markup. These
//! check the file ffmpeg actually writes: its pixel size, that it is the same
//! frame the thumbnail shows, and that a full-size still stays light enough to
//! be worth showing before the video starts.
//!
//! Skipped when ffmpeg is absent rather than failing: the encoder is an
//! optional runtime dependency moss downloads, not a build requirement.

use moss_build::build::media::ffmpeg::FFmpegManager;

fn ffmpeg() -> Option<String> {
    let out = std::process::Command::new("ffmpeg").arg("-version").output().ok()?;
    out.status.success().then(|| "ffmpeg".to_string())
}

/// A 3 s video of `size`: a busy test pattern with film-grain noise on top, which
/// compresses about as badly as a photograph does, so a size bound that holds
/// here is not an artefact of a flat test card.
fn testsrc(dir: &std::path::Path, bin: &str, name: &str, size: &str) -> Option<std::path::PathBuf> {
    let out = dir.join(name);
    let ok = std::process::Command::new(bin)
        .args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", &format!("testsrc2=size={size}:rate=30:duration=3,noise=alls=12:allf=t"),
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
        ])
        .arg(&out)
        .status()
        .ok()?;
    ok.success().then_some(out)
}

fn poster_of(bin: &str, src: &std::path::Path, out: &std::path::Path) -> (u32, u32, u64) {
    let manager = FFmpegManager::from_bin_path(bin.to_string());
    assert_eq!(manager.generate_poster(src, out, None, None), Ok(true));
    let (w, h) = image::image_dimensions(out).expect("the poster is a decodable image");
    (w, h, std::fs::metadata(out).unwrap().len())
}

#[test]
fn a_poster_is_the_sources_display_size_fitted_into_the_top_rung_and_stays_light() {
    let Some(bin) = ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    for (size, expect) in [
        ("1920x1080", (1920, 1080)),
        // Never enlarged.
        ("1280x720", (1280, 720)),
        // Portrait: the height binds.
        ("1080x1920", (608, 1080)),
        // Fitted down to the box.
        ("3840x2160", (1920, 1080)),
    ] {
        let Some(src) = testsrc(dir.path(), &bin, &format!("src-{size}.mp4"), size) else {
            eprintln!("skipping: could not synthesise a {size} source");
            return;
        };
        let out = dir.path().join(format!("poster-{size}.jpg"));
        let (w, h, bytes) = poster_of(&bin, &src, &out);
        assert_eq!((w, h), expect, "poster of a {size} source");
        eprintln!("poster of a {size} source: {w}x{h}, {bytes} bytes");
        assert!(bytes < 400 * 1024, "{size} poster is {bytes} bytes");
    }
}

#[test]
fn a_poster_is_the_frame_the_thumbnail_shows() {
    let Some(bin) = ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    // Red for the first half second, blue after: the 3 s clip's poster frame is
    // taken at 1 s, so a still cut at the wrong time (the first frame, say)
    // would be red.
    let src = dir.path().join("src.mp4");
    let made = std::process::Command::new(&bin)
        .args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", "color=c=red:s=640x360:r=30:d=0.5",
            "-f", "lavfi", "-i", "color=c=blue:s=640x360:r=30:d=2.5",
            "-filter_complex", "[0:v][1:v]concat=n=2:v=1:a=0,format=yuv420p",
            "-c:v", "libx264", "-preset", "ultrafast",
        ])
        .arg(&src)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !made {
        eprintln!("skipping: could not synthesise a two-colour source");
        return;
    }
    let manager = FFmpegManager::from_bin_path(bin.clone());
    let poster = dir.path().join("poster.jpg");
    let thumb = dir.path().join("thumb.jpg");
    assert_eq!(manager.generate_poster(&src, &poster, None, None), Ok(true));
    assert_eq!(manager.generate_thumbnail(&src, &thumb, None, None), Ok(true));
    for still in [&poster, &thumb] {
        let img = image::open(still).expect("decodable").to_rgb8();
        let [r, g, b] = img.get_pixel(img.width() / 2, img.height() / 2).0;
        assert!(b > 150 && r < 100 && g < 100, "{} shows ({r},{g},{b}), not blue", still.display());
    }
}
