//! Tests for the HLS ladder muxing arguments.
//!
//! These assert the command line rather than spawning ffmpeg. The shape they
//! encode was verified empirically first: the full 6-rung + 2-audio-group
//! invocation produced 17 files, every rung segmenting at 6.000s, with each
//! `#EXT-X-STREAM-INF` bound to the group it named.

use super::*;
use crate::build::cache::{ObjectStore, TransformCache, TransformEntry, TransformRecord};
use crate::build::media::ffmpeg::{real_ffmpeg, synthesise};
use moss_core::asset_paths::{AudioGroup, VIDEO_LADDER};

fn args() -> Vec<String> {
    build_hls_args(
        "in.mov",
        "/out/clip.hls",
        &VIDEO_LADDER,
        30.0,
        true,
        "faster",
        4,
    )
}

fn joined() -> String {
    args().join(" ")
}

#[test]
fn keyframe_cadence_is_seconds_not_frames() {
    // The defect this guards: `-g 60` is 60 FRAMES, which is 2 s at 30 fps but
    // 4 s at 15 fps, and the bottom rung is 15 fps. Measured with a flat 60,
    // the bottom rung segmented at 8.0s/2.07s against 6.0s/4.03s everywhere
    // else — boundaries that do not line up, so a player cannot switch cleanly.
    let a = args();
    let gop_for = |i: usize| -> String {
        let flag = format!("-g:v:{i}");
        let at = a.iter().position(|x| *x == flag).expect("gop flag present");
        a[at + 1].clone()
    };
    assert_eq!(gop_for(0), "30", "15 fps rung: 2 s is 30 frames");
    assert_eq!(gop_for(1), "60", "30 fps rung: 2 s is 60 frames");
    for (i, rung) in VIDEO_LADDER.iter().enumerate() {
        assert_eq!(
            gop_for(i).parse::<u32>().unwrap(),
            2 * rung.fps,
            "every rung keyframes on the same wall-clock cadence"
        );
    }
}

#[test]
fn every_rung_is_vbv_capped_at_its_own_bitrate() {
    // `-b:v` alone is an average a high-motion passage can burst far above.
    // BANDWIDTH in the master playlist is a promise the player switches on, so
    // the sustained rate has to be bounded.
    let a = args();
    for (i, rung) in VIDEO_LADDER.iter().enumerate() {
        let val = |flag: String| {
            let at = a.iter().position(|x| *x == flag).unwrap();
            a[at + 1].clone()
        };
        assert_eq!(val(format!("-b:v:{i}")), format!("{}k", rung.video_kbps));
        assert_eq!(val(format!("-maxrate:v:{i}")), format!("{}k", rung.video_kbps));
        assert_eq!(
            val(format!("-bufsize:v:{i}")),
            format!("{}k", rung.video_kbps * 2)
        );
    }
}

#[test]
fn audio_is_stored_per_group_not_per_rung() {
    // Six rungs, two audio renditions. Muxing per rung would store audio six
    // times and degrade it in lockstep with the picture.
    assert_eq!(audio_groups(&VIDEO_LADDER), vec![AudioGroup::Lean, AudioGroup::Clean]);
    let j = joined();
    assert!(j.contains("-b:a:0 32k"), "lean rendition is 32k");
    assert!(j.contains("-ac:a:0 1"), "lean rendition is mono");
    assert!(j.contains("-b:a:1 192k"), "clean rendition is 192k");
    assert!(j.contains("-ac:a:1 2"), "clean rendition is stereo");
    assert!(!j.contains("-b:a:2"), "exactly two renditions, not one per rung");
}

#[test]
fn each_rung_names_the_audio_group_it_can_pay_for() {
    let a = args();
    let at = a.iter().position(|x| x == "-var_stream_map").unwrap();
    let map = &a[at + 1];
    assert!(map.contains("v:0,agroup:alo,name:v0"), "bottom rung takes lean audio");
    assert!(map.contains("v:1,agroup:alo,name:v1"), "so does 416x234 — 145k + 192k is 370 kbps");
    assert!(map.contains("v:2,agroup:ahi,name:v2"));
    assert!(map.contains("v:5,agroup:ahi,name:v5"));
    assert!(map.contains("a:0,agroup:alo,name:alo,default:yes"));
    assert!(map.contains("a:1,agroup:ahi,name:ahi,default:yes"));
}

#[test]
fn the_lowest_rung_is_declared_first() {
    // Native HLS picks the first #EXT-X-STREAM-INF before it has any estimate.
    // A fast viewer starting low sees a soft picture for seconds then climbs;
    // a starved viewer starting high sees nothing at all, indefinitely.
    let a = args();
    let map = &a[a.iter().position(|x| x == "-var_stream_map").unwrap() + 1];
    let v0 = map.find("name:v0").unwrap();
    let v5 = map.find("name:v5").unwrap();
    assert!(v0 < v5, "variants are declared lowest rung first");
}

#[test]
fn one_decode_feeds_every_rung() {
    // `split` is why six rungs cost 1.32x the CPU of one two-pass encode
    // rather than six times it.
    let a = args();
    let f = &a[a.iter().position(|x| x == "-filter_complex").unwrap() + 1];
    assert!(f.starts_with("[0:v]split=6"), "one decode, fanned out: {f}");
    assert_eq!(f.matches("scale=").count(), 6);
}

#[test]
fn frame_rate_is_only_ever_reduced() {
    // Forcing 30 on a 24 fps source duplicates frames and spends bitrate on
    // them. Only the 15 fps rung needs an fps filter from a 30 fps source.
    let a = build_hls_args("in.mov", "/o/c.hls", &VIDEO_LADDER, 30.0, true, "faster", 4);
    let f = &a[a.iter().position(|x| x == "-filter_complex").unwrap() + 1];
    assert_eq!(f.matches("fps=").count(), 1, "only the 15 fps rung is resampled");

    let slow = build_hls_args("in.mov", "/o/c.hls", &VIDEO_LADDER, 12.0, true, "faster", 4);
    let sf = &slow[slow.iter().position(|x| x == "-filter_complex").unwrap() + 1];
    assert_eq!(sf.matches("fps=").count(), 0, "a 12 fps source is never sped up");
}

#[test]
fn a_silent_source_maps_no_audio() {
    // `-map a:0` against a source with no audio stream fails the whole encode.
    let a = build_hls_args("in.mov", "/o/c.hls", &VIDEO_LADDER, 30.0, false, "faster", 4);
    let j = a.join(" ");
    assert!(!j.contains("-map a:0"), "no audio stream to map");
    assert!(!j.contains("agroup"), "and no rendition groups to reference");
}

#[test]
fn segments_are_byte_ranges_in_one_file_per_rung() {
    let j = joined();
    assert!(j.contains("-hls_segment_type fmp4"));
    assert!(j.contains("-hls_flags single_file"));
    assert!(j.contains("-hls_playlist_type vod"));
    assert!(j.contains("-hls_segment_filename /out/clip.hls/%v.m4s"));
    assert!(j.contains("-master_pl_name master.m3u8"));
}

#[test]
fn independent_segments_tag_is_inserted_after_the_version_line() {
    // ffmpeg's own `independent_segments` flag declines to emit this for the
    // fmp4 + rendition-group shape, measured. The keyframe settings above are
    // what make the assertion true, so it is added rather than left missing.
    let master = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-MEDIA:TYPE=AUDIO\n";
    let out = patch_master(master, &VIDEO_LADDER);
    assert_eq!(
        out,
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-MEDIA:TYPE=AUDIO\n"
    );
}

#[test]
fn patching_a_master_twice_changes_nothing_the_second_time() {
    // Not a nicety: the second pass would charge every variant for its audio a
    // second time. The tag is what marks the master as already patched.
    let master = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-STREAM-INF:BANDWIDTH=45000,\
         RESOLUTION=320x180,CODECS=\"avc1.42c00c,mp4a.40.2\",AUDIO=\"group_alo\"\nv0.m3u8\n"
    );
    let once = patch_master(&master, &VIDEO_LADDER);
    assert_eq!(patch_master(&once, &VIDEO_LADDER), once, "idempotent");
}

#[test]
fn independent_segments_tag_still_precedes_variants_without_a_version_line() {
    let out = patch_master("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv0.m3u8\n", &VIDEO_LADDER);
    assert!(
        out.find("#EXT-X-INDEPENDENT-SEGMENTS").unwrap()
            < out.find("#EXT-X-STREAM-INF").unwrap(),
        "the tag governs the variants that follow it"
    );
}

#[test]
fn a_variants_bandwidth_grows_by_the_audio_rendition_it_names() {
    // ffmpeg measures a variant from its video segments alone, so the 320x180
    // rung arrived declaring 49,669 bps while the 32 kbps rendition it plays
    // cost another ~35,000. A player selects on this number and nothing else.
    let line = "#EXT-X-STREAM-INF:BANDWIDTH=49669,AVERAGE-BANDWIDTH=49601,\
                RESOLUTION=320x180,CODECS=\"avc1.42c00c,mp4a.40.2\",AUDIO=\"group_alo\"";
    let out = patch_master(&format!("#EXTM3U\n#EXT-X-VERSION:7\n{line}\nv0.m3u8\n"), &VIDEO_LADDER);
    assert!(out.contains("BANDWIDTH=81669"), "45k rung + 32k audio: {out}");
    assert!(out.contains("AVERAGE-BANDWIDTH=81601"), "{out}");
    // Everything else survives byte-for-byte, including the comma inside CODECS.
    assert!(out.contains(r#"RESOLUTION=320x180,CODECS="avc1.42c00c,mp4a.40.2",AUDIO="group_alo""#));
}

#[test]
fn each_rung_is_charged_for_its_own_group_not_a_shared_one() {
    // The two-group split only reaches a player through these numbers: if both
    // rungs were charged the same audio, the lean group would buy nothing.
    let master = "#EXTM3U\n#EXT-X-VERSION:7\n\
        #EXT-X-STREAM-INF:BANDWIDTH=45000,RESOLUTION=320x180,AUDIO=\"group_alo\"\nv0.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=365000,RESOLUTION=640x360,AUDIO=\"group_ahi\"\nv2.m3u8\n";
    let out = patch_master(master, &VIDEO_LADDER);
    assert!(out.contains("BANDWIDTH=77000"), "45k + 32k lean: {out}");
    assert!(out.contains("BANDWIDTH=557000"), "365k + 192k clean: {out}");
}

#[test]
fn a_silent_ladder_keeps_the_bandwidth_ffmpeg_measured() {
    // No `AUDIO=` means no separate rendition, so ffmpeg's figure is already
    // the whole variant and adding to it would overstate the price.
    let master = "#EXTM3U\n#EXT-X-VERSION:7\n\
                  #EXT-X-STREAM-INF:BANDWIDTH=45000,RESOLUTION=320x180\nv0.m3u8\n";
    assert!(patch_master(master, &VIDEO_LADDER).contains("BANDWIDTH=45000,"));
}

// ── produce_ladder: cache, links and the naming contract ───────────

/// The one thing the pure tests cannot see: whether the files the worker links
/// into staging carry the names the master playlist references, whether a
/// second build re-uses them, and whether that re-use survives the video being
/// renamed.
///
/// Cache re-use is proved by deleting the source between the two calls. A hit
/// that still succeeds cannot have re-encoded anything; a wall-clock comparison
/// would only be a guess about a shared four-core box.
#[test]
fn produce_ladder_links_a_self_consistent_ladder_and_reuses_it_under_a_new_name() {
    let Some(bin) = real_ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("holiday.mov");
    if !synthesise(&bin, &source, "1280x720") {
        eprintln!("skipping: could not synthesise a source");
        return;
    }

    let transforms = TransformCache::new(
        dir.path().join("transforms"),
        ObjectStore::new(dir.path().join("objects")),
    );
    let ffmpeg = FFmpegManager::from_bin_path(bin);
    let staging = dir.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();

    let run = |src: &Path, ladder_dir: &Path| {
        produce_ladder(
            &ffmpeg,
            src,
            "oid-holiday",
            ladder_dir,
            dir.path(),
            &transforms,
            &VideoCompressionConfig::default(),
            None,
            None,
            None,
        )
    };

    let holiday = staging.join("holiday.hls");
    let first = run(&source, &holiday)
        .expect("produce_ladder")
        .expect("a 1280-wide source fills the ladder");

    let self_consistent = |ladder_dir: &Path| {
        let master = std::fs::read_to_string(ladder_dir.join(HLS_MASTER_NAME)).unwrap();
        for line in master.lines() {
            let target = match line.trim() {
                l if l.is_empty() => continue,
                l if l.starts_with('#') => match l.split("URI=\"").nth(1) {
                    Some(rest) => rest.split('"').next().unwrap().to_string(),
                    None => continue,
                },
                l => l.to_string(),
            };
            assert!(
                ladder_dir.join(&target).exists(),
                "master references {target}, which is not in {}",
                ladder_dir.display()
            );
        }
    };
    self_consistent(&holiday);

    // Record what the first run produced, then take the source away.
    let mut record = TransformRecord {
        source_oid: "oid-holiday".to_string(),
        source_size: 0,
        transforms: std::collections::HashMap::new(),
    };
    record.transforms.extend(first.iter().cloned());
    transforms.put(&record).unwrap();
    std::fs::remove_file(&source).unwrap();
    std::fs::remove_dir_all(&holiday).unwrap();

    // The rename is the point: same bytes, different video name. Playlists that
    // embedded the old stem would send every child request to a 404, and a
    // <video> that has committed to a source does not fall back.
    let iceland = staging.join("iceland.hls");
    let second = run(&source, &iceland).expect("cache hit").expect("still a ladder");
    assert_eq!(
        first.iter().map(|(n, e)| (n.clone(), e.oid.clone())).collect::<Vec<_>>(),
        second.iter().map(|(n, e)| (n.clone(), e.oid.clone())).collect::<Vec<_>>(),
        "the cached run returns the same files"
    );
    self_consistent(&iceland);
}

/// A partial ladder is a miss, not a hit: seventeen files that reference each
/// other are only valid together, and one evicted blob makes the rest garbage.
#[test]
fn a_missing_rung_invalidates_the_whole_cached_ladder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let transforms = TransformCache::new(
        dir.path().join("transforms"),
        ObjectStore::new(dir.path().join("objects")),
    );
    let params = ladder_params(&VideoCompressionConfig::default());
    // A real blob: `find_cached_output` checks the object store, so a record
    // pointing at nothing is a miss for a reason this test is not about.
    let blob = dir.path().join("blob");
    std::fs::write(&blob, b"ladder file").unwrap();
    let oid = transforms.objects().store_file(&blob).unwrap();
    let members = hls_members(video_ladder_rungs(1280));
    assert_eq!(members.len(), 17, "six rungs and two renditions");

    let write_record = |skip: Option<&str>| {
        let mut record = TransformRecord {
            source_oid: "oid".to_string(),
            source_size: 0,
            transforms: std::collections::HashMap::new(),
        };
        for name in &members {
            if Some(name.as_str()) == skip {
                continue;
            }
            record.transforms.insert(
                format!("video/hls/{name}"),
                TransformEntry {
                    oid: oid.clone(),
                    size: 1,
                    params: params.clone(),
                },
            );
        }
        transforms.put(&record).unwrap();
    };

    write_record(None);
    assert!(
        cached_ladder(&transforms, "oid", &params).is_some(),
        "a complete record is a hit"
    );

    write_record(Some("v3.m4s"));
    assert!(
        cached_ladder(&transforms, "oid", &params).is_none(),
        "one missing segment invalidates the whole ladder"
    );
}

/// A change to the per-file budget must invalidate every cached ladder, the
/// same way a table edit already does — otherwise a tightened
/// `hls_max_file_mb` would never re-run and an over-budget file already on
/// disk would keep being served.
#[test]
fn ladder_params_carries_the_per_file_budget() {
    let loose = ladder_params(&VideoCompressionConfig { hls_max_file_mb: 150, ..VideoCompressionConfig::default() });
    let tight = ladder_params(&VideoCompressionConfig { hls_max_file_mb: 50, ..VideoCompressionConfig::default() });
    assert_eq!(loose["max_file_mb"], serde_json::json!(150));
    assert_eq!(tight["max_file_mb"], serde_json::json!(50));
    assert_ne!(loose, tight, "a budget-only edit must change the cache key");
}

/// A ladder that SHRINKS — a tighter `hls_max_file_mb`, or a table edit
/// dropping the top rung — must be a cache HIT on the very next lookup, not a
/// permanent miss.
///
/// Reproduces exactly what a plain merge-only write left behind: a record
/// seeded as a complete 6-rung ladder recorded under a looser budget (`stale_
/// params`), then a fresh 5-rung ladder recorded through `video::record_
/// ladder` under today's tighter one. The 15 keys the two ladders share get
/// overwritten either way; what only `record_ladder`'s drop-before-insert
/// gets right is the two keys that do NOT overlap — the old top rung's
/// `video/hls/v5.m3u8`/`.m4s`. Left behind, `cached_ladder` counts 6 rungs
/// where 5 actually exist, expects `video/hls/v5.*` under today's params, and
/// finds yesterday's — a miss, forever.
#[test]
fn a_shrunk_ladder_is_recorded_as_a_hit_not_a_stale_miss() {
    let dir = tempfile::tempdir().expect("tempdir");
    let transforms = TransformCache::new(
        dir.path().join("transforms"),
        ObjectStore::new(dir.path().join("objects")),
    );
    let source = dir.path().join("clip.mov");
    std::fs::write(&source, b"source bytes").unwrap();
    let source_oid = "oid-shrink";

    // The complete 6-rung ladder a looser budget left in the record.
    let stale_config =
        VideoCompressionConfig { hls_max_file_mb: 100_000, ..VideoCompressionConfig::default() };
    let stale_params = ladder_params(&stale_config);
    let stale_blob = dir.path().join("stale-blob");
    std::fs::write(&stale_blob, b"stale ladder file").unwrap();
    let stale_oid = transforms.objects().store_file(&stale_blob).unwrap();
    let stale_members = hls_members(video_ladder_rungs(1280));
    assert_eq!(stale_members.len(), 17, "six rungs and two renditions");
    let mut record = TransformRecord {
        source_oid: source_oid.to_string(),
        source_size: 0,
        transforms: std::collections::HashMap::new(),
    };
    for name in &stale_members {
        record.transforms.insert(
            format!("video/hls/{name}"),
            TransformEntry { oid: stale_oid.clone(), size: 1, params: stale_params.clone() },
        );
    }
    transforms.put(&record).unwrap();

    // The fresh 5-rung ladder `produce_ladder` records after the budget
    // narrowed it, the way `convert_single_video`'s `Ok(Some(entries))` arm
    // does: through `record_ladder`, not a plain merge.
    let config = VideoCompressionConfig::default();
    let fresh_params = ladder_params(&config);
    let fresh_members = hls_members(&VIDEO_LADDER[..5]);
    assert_eq!(fresh_members.len(), 15, "five rungs and two renditions");
    let fresh_blob = dir.path().join("fresh-blob");
    std::fs::write(&fresh_blob, b"fresh ladder file").unwrap();
    let fresh_oid = transforms.objects().store_file(&fresh_blob).unwrap();
    let fresh_entries: Vec<(String, TransformEntry)> = fresh_members
        .iter()
        .map(|name| {
            (
                format!("video/hls/{name}"),
                TransformEntry { oid: fresh_oid.clone(), size: 1, params: fresh_params.clone() },
            )
        })
        .collect();

    crate::build::media::video::record_ladder(&transforms, source_oid, &source, fresh_entries);

    let (members, oids) = cached_ladder(&transforms, source_oid, &fresh_params)
        .expect("a shrunk ladder must stay a cache hit on the very next lookup");
    assert_eq!(members, fresh_members, "the shrunk ladder's own 5-rung file set, not the stale 6-rung one");
    assert!(
        oids.iter().all(|o| o == &fresh_oid),
        "every resolved oid must come from the fresh record, not the stale one left behind"
    );
}

/// A tiny per-file budget reaches all the way through a real encode: fewer
/// files land in staging than the same 1280-wide source gets from width
/// alone, and it is exactly the files a 5-rung ladder promises — not the
/// unit tests in moss-core, which never invoke ffmpeg or touch staging.
#[test]
fn produce_ladder_honors_a_tiny_per_file_budget() {
    let Some(bin) = real_ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("holiday.mov");
    if !synthesise(&bin, &source, "1280x720") {
        eprintln!("skipping: could not synthesise a source");
        return;
    }

    let transforms = TransformCache::new(
        dir.path().join("transforms"),
        ObjectStore::new(dir.path().join("objects")),
    );
    let ffmpeg = FFmpegManager::from_bin_path(bin);
    let staging = dir.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();

    // `synthesise` writes a 4 s clip. At 4 s, the top rung's own video file
    // (2000 kbps * 4 s * 1.05 margin = 1,050,000 bytes) is just over a 1 MiB
    // (1,048,576 byte) cap while every rung below it clears that cap
    // comfortably — a budget picked to drop exactly the top rung, proving
    // the truncation without relying on width (1280 qualifies every rung by
    // width alone).
    let config = VideoCompressionConfig { hls_max_file_mb: 1, ..VideoCompressionConfig::default() };
    let ladder_dir = staging.join("holiday.hls");
    let entries = produce_ladder(
        &ffmpeg,
        &source,
        "oid-tiny-budget",
        &ladder_dir,
        dir.path(),
        &transforms,
        &config,
        None,
        None,
        None,
    )
    .expect("produce_ladder")
    .expect("5 rungs is still a ladder");

    let expected = hls_members(&VIDEO_LADDER[..5]);
    assert_eq!(expected.len(), 15, "five rungs and two renditions");
    let rung_count = entries
        .iter()
        .filter(|(n, _)| n.starts_with("video/hls/v") && n.ends_with(".m3u8"))
        .count();
    assert_eq!(rung_count, 5, "a 1 MiB budget must drop only the top (1280x720) rung");
    assert_eq!(entries.len(), expected.len(), "exactly the truncated ladder's own census");

    let mut on_disk: Vec<String> = std::fs::read_dir(&ladder_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    on_disk.sort();
    let mut expected_sorted = expected;
    expected_sorted.sort();
    assert_eq!(on_disk, expected_sorted, "staging holds exactly the truncated ladder's files, no v5");
}

/// A source too narrow for a second rung gets no ladder, and no ffmpeg run.
///
/// This is where `EncodePlan::KeepOriginal` lands under HLS. It is asserted
/// against a nonexistent source and a bogus encoder path on purpose: if
/// `produce_ladder` reached either, this would fail rather than return `None`.
#[test]
fn a_source_narrower_than_two_rungs_gets_no_ladder() {
    let Some(bin) = real_ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("tiny.mp4");
    assert!(std::process::Command::new(&bin)
        .args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", "testsrc=size=320x180:rate=30:duration=2",
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
        ])
        .arg(&source)
        .status()
        .expect("synthesise")
        .success());

    let transforms = TransformCache::new(
        dir.path().join("transforms"),
        ObjectStore::new(dir.path().join("objects")),
    );
    let staging = dir.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();

    let got = produce_ladder(
        &FFmpegManager::from_bin_path(bin),
        &source,
        "oid-tiny",
        &staging.join("tiny"),
        dir.path(),
        &transforms,
        &VideoCompressionConfig::default(),
        None,
        None,
        None,
    )
    .expect("produce_ladder");

    assert!(got.is_none(), "one rung is not a ladder");
    assert_eq!(
        std::fs::read_dir(&staging).unwrap().count(),
        0,
        "and nothing was written"
    );
}
