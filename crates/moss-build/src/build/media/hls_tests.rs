//! Tests for the HLS ladder muxing arguments.
//!
//! These assert the command line rather than spawning ffmpeg. The shape they
//! encode was verified empirically first: the full 6-rung (1280x720 source) + 2-audio-group
//! invocation produced 17 files, every rung segmenting at 6.000s, with each
//! `#EXT-X-STREAM-INF` bound to the group it named.

use super::*;
use crate::build::cache::{ObjectStore, TransformCache, TransformEntry, TransformRecord};
use crate::build::media::ffmpeg::{
    real_ffmpeg, synthesise, synthesise_silent, synthesise_with_silent_audio_track,
};
use moss_core::asset_paths::{AudioGroup, VIDEO_LADDER};

fn args() -> Vec<String> {
    build_hls_args(
        "in.mov",
        "/out/clip.hls",
        &VIDEO_LADDER,
        (1920, 1080),
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
fn every_rung_aims_at_a_quality_capped_at_its_own_bitrate() {
    // A rung's rate is a bound, not a target: aimed at, it is spent on a
    // picture that does not need it. The bound stays because a high-motion
    // passage can otherwise burst far above what the link was promised.
    let a = args();
    for (i, rung) in VIDEO_LADDER.iter().enumerate() {
        let val = |flag: String| {
            let at = a.iter().position(|x| *x == flag).unwrap();
            a[at + 1].clone()
        };
        assert_eq!(
            val(format!("-crf:v:{i}")),
            crate::build::media::ffmpeg::VIDEO_QUALITY_CRF.to_string()
        );
        assert!(!a.contains(&format!("-b:v:{i}")), "a target rate is spent even on a still picture");
        assert_eq!(val(format!("-maxrate:v:{i}")), format!("{}k", rung.video_kbps));
        assert_eq!(
            val(format!("-bufsize:v:{i}")),
            format!("{}k", rung.video_kbps * 2)
        );
    }
}

#[test]
fn a_rung_is_capped_at_its_own_ceiling_not_a_table_rows() {
    // `build_hls_args` reads `rung.video_kbps` off whatever it's handed. A
    // ladder's top rung is the source's own size, and a size between two
    // table rows has a ceiling of its own: 1600x900 is 2777 kbps, where no
    // row says that. Re-deriving a rate from `VIDEO_LADDER` by size or index
    // would undo it.
    let top = moss_core::asset_paths::video_top_rung((1600, 900));
    assert_eq!(top.video_kbps, 2777);
    let rungs = [VIDEO_LADDER[0], top];
    let a = build_hls_args("in.mov", "/out/clip.hls", &rungs, (1600, 900), 30.0, true, "faster", 4);
    let val = |flag: &str| {
        let at = a.iter().position(|x| x == flag).expect(flag);
        a[at + 1].clone()
    };
    assert_eq!(val("-maxrate:v:1"), "2777k");
    assert_eq!(val("-bufsize:v:1"), "5554k", "2x the ceiling");
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
    // `split` is why a ladder's rungs share one decode of the source instead
    // of paying for one each.
    let a = args();
    let f = &a[a.iter().position(|x| x == "-filter_complex").unwrap() + 1];
    assert!(f.starts_with(&format!("[0:v]split={}", VIDEO_LADDER.len())), "one decode, fanned out: {f}");
    assert_eq!(f.matches("scale=").count(), VIDEO_LADDER.len());
}

#[test]
fn frame_rate_is_only_ever_reduced() {
    // Forcing 30 on a 24 fps source duplicates frames and spends bitrate on
    // them. Only the 15 fps rung needs an fps filter from a 30 fps source.
    let a = build_hls_args("in.mov", "/o/c.hls", &VIDEO_LADDER, (1280, 720), 30.0, true, "faster", 4);
    let f = &a[a.iter().position(|x| x == "-filter_complex").unwrap() + 1];
    assert_eq!(f.matches("fps=").count(), 1, "only the 15 fps rung is resampled");

    let slow = build_hls_args("in.mov", "/o/c.hls", &VIDEO_LADDER, (1280, 720), 12.0, true, "faster", 4);
    let sf = &slow[slow.iter().position(|x| x == "-filter_complex").unwrap() + 1];
    assert_eq!(sf.matches("fps=").count(), 0, "a 12 fps source is never sped up");
}

#[test]
fn a_silent_source_maps_no_audio() {
    // `-map a:0` against a source with no audio stream fails the whole encode.
    let a = build_hls_args("in.mov", "/o/c.hls", &VIDEO_LADDER, (1280, 720), 30.0, false, "faster", 4);
    let j = a.join(" ");
    assert!(!j.contains("-map a:0"), "no audio stream to map");
    assert!(!j.contains("agroup"), "and no rendition groups to reference");
}

/// `hls_members` itself always lists both audio groups — it only ever sees
/// `rungs`, never the source. `ladder_members` is the narrowing `produce_
/// ladder`/`cached_ladder` read instead, so a silent source's expected file
/// list matches what `build_hls_args` (above) actually tells ffmpeg to
/// write.
#[test]
fn ladder_members_drops_audio_files_for_a_silent_source() {
    let full = ladder_members(&VIDEO_LADDER, true);
    let narrowed = ladder_members(&VIDEO_LADDER, false);
    assert_eq!(full, hls_members(&VIDEO_LADDER), "has_audio=true is the unnarrowed census, unchanged");
    assert_eq!(
        narrowed.len(),
        full.len() - 4,
        "exactly the two audio groups' two files each must drop: {narrowed:?}"
    );
    for group in [AudioGroup::Lean, AudioGroup::Clean] {
        let a = group.as_str();
        assert!(!narrowed.iter().any(|m| m.starts_with(a)), "{a} must be gone: {narrowed:?}");
    }
    assert_eq!(narrowed[0], HLS_MASTER_NAME, "the gate stays first");
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
    let out = patch_master(master, &VIDEO_LADDER, |_| None);
    assert_eq!(
        out,
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-MEDIA:TYPE=AUDIO\n"
    );
}

#[test]
fn a_master_that_already_says_its_segments_are_independent_still_gets_its_figures() {
    // An ffmpeg that does emit the tag for this shape must not skip the rest:
    // the tag is not added twice, and the audio is still charged.
    let master = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n\
                  #EXT-X-STREAM-INF:BANDWIDTH=45000,RESOLUTION=320x180,AUDIO=\"group_alo\"\nv0.m3u8\n";
    let out = patch_master(master, &VIDEO_LADDER, |_| None);
    assert_eq!(out.matches("#EXT-X-INDEPENDENT-SEGMENTS").count(), 1, "{out}");
    assert!(out.contains("BANDWIDTH=77000,"), "45k + 32k lean: {out}");
}

#[test]
fn independent_segments_tag_still_precedes_variants_without_a_version_line() {
    let out = patch_master("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv0.m3u8\n", &VIDEO_LADDER, |_| None);
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
    let out = patch_master(&format!("#EXTM3U\n#EXT-X-VERSION:7\n{line}\nv0.m3u8\n"), &VIDEO_LADDER, |_| None);
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
    let out = patch_master(master, &VIDEO_LADDER, |_| None);
    assert!(out.contains("BANDWIDTH=77000"), "45k + 32k lean: {out}");
    assert!(out.contains("BANDWIDTH=557000"), "365k + 192k clean: {out}");
}

#[test]
fn a_silent_ladder_keeps_the_bandwidth_ffmpeg_measured() {
    // No `AUDIO=` means no separate rendition, so ffmpeg's figure is already
    // the whole variant and adding to it would overstate the price.
    let master = "#EXTM3U\n#EXT-X-VERSION:7\n\
                  #EXT-X-STREAM-INF:BANDWIDTH=45000,RESOLUTION=320x180\nv0.m3u8\n";
    assert!(patch_master(master, &VIDEO_LADDER, |_| None).contains("BANDWIDTH=45000,"));
}

const RUNG_PLAYLIST: &str = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:6\n\
    #EXT-X-MAP:URI=\"v6.m4s\",BYTERANGE=\"848@0\"\n\
    #EXTINF:6.000000,\n#EXT-X-BYTERANGE:150000@848\nv6.m4s\n\
    #EXTINF:6.000000,\n#EXT-X-BYTERANGE:600000@150848\nv6.m4s\n\
    #EXTINF:2.000000,\n#EXT-X-BYTERANGE:50000@750848\nv6.m4s\n#EXT-X-ENDLIST\n";

#[test]
fn a_rungs_rates_are_its_heaviest_run_of_segments_and_its_whole_file_over_its_duration() {
    // 600,000 bytes over 6 s is the peak; 800,000 bytes over 14 s the average.
    // The init segment's byte range is not media and is not counted, and the
    // 2 s tail is too short to be a run of its own.
    assert_eq!(segment_rates(RUNG_PLAYLIST), Some((800_000, 457_143)));
    assert_eq!(segment_rates("#EXTM3U\n#EXT-X-ENDLIST\n"), None);
}

#[test]
fn a_short_last_segment_holding_one_keyframe_does_not_set_the_peak() {
    // A 12.2 s clip cut at 6 s ends on 0.2 s holding a 100 kB keyframe: 4 Mbps
    // on its own, against a rung that runs at 200 kbps. Measured with the
    // segment before it, as RFC 8216 measures a peak, it is 323 kbps.
    let playlist = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n\
        #EXTINF:6.000000,\n#EXT-X-BYTERANGE:150000@848\n\
        #EXTINF:6.000000,\n#EXT-X-BYTERANGE:150000@150848\n\
        #EXTINF:0.200000,\n#EXT-X-BYTERANGE:100000@300848\n#EXT-X-ENDLIST\n";
    assert_eq!(segment_rates(playlist), Some((322_581, 262_295)));
}

#[test]
fn a_clip_shorter_than_half_a_segment_peaks_at_its_average() {
    let playlist = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:2.000000,\n#EXT-X-BYTERANGE:50000@848\n";
    assert_eq!(segment_rates(playlist), Some((200_000, 200_000)));
}

#[test]
fn a_variant_declared_at_its_ceiling_is_redeclared_at_what_it_measures_plus_its_audio() {
    // What ffmpeg before 7.1 writes for a rung capped at 4000 kbps: the
    // ceiling plus a tenth, and no average. A player on a 2 Mbps link would
    // decline this rung, which peaks at 800 kbps.
    let master = "#EXTM3U\n#EXT-X-VERSION:7\n\
        #EXT-X-STREAM-INF:BANDWIDTH=4400000,RESOLUTION=1708x1080,CODECS=\"avc1.640032,mp4a.40.2\",AUDIO=\"group_ahi\"\n\
        v6.m3u8\n";
    let out = patch_master(master, &VIDEO_LADDER, |uri| (uri == "v6.m3u8").then(|| segment_rates(RUNG_PLAYLIST)).flatten());
    assert!(
        out.contains("#EXT-X-STREAM-INF:BANDWIDTH=992000,AVERAGE-BANDWIDTH=649143,RESOLUTION=1708x1080"),
        "800k peak and 457k average, each plus the 192k audio: {out}"
    );
    assert!(out.contains(r#"CODECS="avc1.640032,mp4a.40.2",AUDIO="group_ahi""#), "{out}");
}

#[test]
fn a_variant_that_already_carries_an_average_has_both_figures_replaced() {
    let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=4400000,AVERAGE-BANDWIDTH=4000000,RESOLUTION=1708x1080\nv6.m3u8\n";
    let out = patch_master(master, &VIDEO_LADDER, |_| segment_rates(RUNG_PLAYLIST));
    assert!(out.contains("#EXT-X-STREAM-INF:BANDWIDTH=800000,AVERAGE-BANDWIDTH=457143,RESOLUTION=1708x1080\nv6.m3u8\n"), "{out}");
}

/// The encode itself redeclares the master, whatever the encoder wrote: here
/// a stand-in for an ffmpeg older than 7.1, which declares a variant at its
/// rate ceiling plus a tenth however little the rung weighs.
#[cfg(unix)]
#[test]
fn encode_ladder_declares_each_rung_at_what_its_segments_measure() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let out_dir = dir.path().join("clip.hls");
    let rung_playlist = |name: &str| RUNG_PLAYLIST.replace("v6.m4s", name);
    // The last argument is `<out_dir>/%v.m3u8`; the stand-in writes the
    // master and both rung playlists beside it.
    let script = format!(
        "#!/bin/sh\nfor a in \"$@\"; do last=\"$a\"; done\nd=$(dirname \"$last\")\n\
         cat > \"$d/master.m3u8\" <<'M'\n#EXTM3U\n#EXT-X-VERSION:7\n\
         #EXT-X-STREAM-INF:BANDWIDTH=49500,RESOLUTION=320x180\nv0.m3u8\n\
         #EXT-X-STREAM-INF:BANDWIDTH=159500,RESOLUTION=416x234\nv1.m3u8\nM\n\
         cat > \"$d/v0.m3u8\" <<'P'\n{}P\ncat > \"$d/v1.m3u8\" <<'P'\n{}P\n",
        rung_playlist("v0.m4s"),
        rung_playlist("v1.m4s"),
    );
    let bin = dir.path().join("ffmpeg");
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let probe = SourceVideo {
        duration_secs: 14.0,
        width: 416,
        height: 234,
        sar: 1.0,
        fps: 30.0,
        has_audio: false,
        total_kbps: None,
        web_playable: false,
    };

    encode_ladder(
        &bin,
        Path::new("clip.mov"),
        &out_dir,
        &VIDEO_LADDER[..2],
        &probe,
        &VideoCompressionConfig::default(),
        None,
        None,
        None,
    )
    .expect("encode");

    let master = std::fs::read_to_string(out_dir.join(HLS_MASTER_NAME)).unwrap();
    assert_eq!(
        master.matches("BANDWIDTH=800000,AVERAGE-BANDWIDTH=457143,").count(),
        2,
        "both rungs peak at 800 kbps and average 457: {master}"
    );
}

// ── produce_ladder: cache, links and the naming contract ───────────

/// A link failure partway through a ladder must never leave the gate file
/// (`master.m3u8`) behind: `hls_members`' own documented order puts it FIRST,
/// but it is also what `heal_ladder` (video.rs) and `register_existing_
/// ladders` both read as "this ladder is complete." Linking in `hls_members`'
/// order would leave the gate down the moment any LATER member's link fails
/// (an evicted/vacuumed blob, disk full, any I/O error), advertising a
/// ladder whose later segments never arrived — a permanent 404, since
/// neither reader would ever notice the hole and re-heal it. Pure and
/// ffmpeg-less: an oid that was never stored fails `link_to` deterministically,
/// no encode or real blob content needed.
#[test]
fn link_members_never_leaves_the_gate_behind_a_failed_link() {
    let dir = tempfile::tempdir().expect("tempdir");
    let transforms = TransformCache::new(
        dir.path().join("transforms"),
        ObjectStore::new(dir.path().join("objects")),
    );
    let members = hls_members(&VIDEO_LADDER);
    assert_eq!(members[0], HLS_MASTER_NAME, "sanity: hls_members puts the gate first");

    // Every member gets a real, resolvable blob EXCEPT one non-gate member
    // (`v0.m3u8`), whose oid names a blob that was never stored.
    let missing_oid = "0".repeat(64);
    let oids: Vec<String> = members
        .iter()
        .map(|name| {
            if name == "v0.m3u8" {
                missing_oid.clone()
            } else {
                transforms.objects().store_bytes(format!("blob for {name}").as_bytes(), crate::build::cache::RecordMode::Request).unwrap()
            }
        })
        .collect();

    let ladder_dir = dir.path().join("clip.hls");
    let result = link_members(transforms.objects(), &ladder_dir, &members, oids, &serde_json::json!({}));

    assert!(result.is_err(), "a missing blob must fail the link, not silently skip it");
    assert!(
        !ladder_dir.join(HLS_MASTER_NAME).exists(),
        "the gate must not be on disk when a later member's link failed — \
         a reader that trusts the gate would call this ladder complete"
    );
}

/// A realistic 4-rung, 2-audio-group master playlist — the shape ffmpeg
/// actually writes for `hls_members(&VIDEO_LADDER[..4])` — so these tests
/// exercise the real parser, not a hand-picked stand-in it happens to accept.
fn four_rung_master() -> &'static str {
    "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n\
     #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"alo\",NAME=\"alo\",AUTOSELECT=YES,DEFAULT=YES,URI=\"alo.m3u8\"\n\
     #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"ahi\",NAME=\"ahi\",AUTOSELECT=YES,DEFAULT=YES,URI=\"ahi.m3u8\"\n\
     #EXT-X-STREAM-INF:BANDWIDTH=77000,RESOLUTION=320x180,CODECS=\"avc1.42c00c,mp4a.40.2\",AUDIO=\"alo\"\n\
     v0.m3u8\n\
     #EXT-X-STREAM-INF:BANDWIDTH=150000,RESOLUTION=416x234,CODECS=\"avc1.42c00c,mp4a.40.2\",AUDIO=\"alo\"\n\
     v1.m3u8\n\
     #EXT-X-STREAM-INF:BANDWIDTH=557000,RESOLUTION=640x360,CODECS=\"avc1.4d401f,mp4a.40.2\",AUDIO=\"ahi\"\n\
     v2.m3u8\n\
     #EXT-X-STREAM-INF:BANDWIDTH=930000,RESOLUTION=768x432,CODECS=\"avc1.4d401f,mp4a.40.2\",AUDIO=\"ahi\"\n\
     v3.m3u8\n"
}

/// The bug this guards: a ladder that SHRINKS (a tighter per-file budget or a
/// table edit leaving fewer rungs than an earlier build kept) leaves the dropped rung's files sitting in
/// `ladder_dir` — nothing unlinks them, because an active unlink here would
/// race a still-draining background encode that might be mid-`link_members`
/// on the very same directory. `ladder_members_from_master` is the census
/// fix instead: it answers "what's in this ladder" from the gate's own
/// references, which a dropped rung is never one of, regardless of what a
/// bigger, earlier ladder left beside it.
#[test]
fn ladder_members_from_master_excludes_a_surplus_rung_left_by_a_bigger_ladder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ladder_dir = dir.path().join("clip.hls");
    std::fs::create_dir_all(&ladder_dir).unwrap();
    std::fs::write(ladder_dir.join(HLS_MASTER_NAME), four_rung_master()).unwrap();
    let today = hls_members(&VIDEO_LADDER[..4]);
    // `today` puts the gate first (`hls_members`' own documented order) —
    // skip it here so the real playlist just written above survives.
    for name in today.iter().filter(|n| n.as_str() != HLS_MASTER_NAME) {
        std::fs::write(ladder_dir.join(name), format!("bytes for {name}")).unwrap();
    }
    // A bigger, earlier ladder's now-dropped top rung, left behind exactly
    // as `link_members` would leave it: real files, just not named by
    // today's master playlist.
    std::fs::write(ladder_dir.join("v4.m3u8"), b"stale rung playlist").unwrap();
    std::fs::write(ladder_dir.join("v4.m4s"), b"stale rung segment").unwrap();

    let members = ladder_members_from_master(&ladder_dir).expect("a real master.m3u8 must parse");

    assert!(!members.contains(&"v4.m3u8".to_string()), "surplus playlist must not be counted: {members:?}");
    assert!(!members.contains(&"v4.m4s".to_string()), "surplus segment must not be counted: {members:?}");
    for name in &today {
        assert!(members.contains(name), "{name} must be counted: {members:?}");
    }
    assert_eq!(members.len(), today.len(), "exactly today's ladder, nothing else: {members:?}");
}

/// A read error is not evidence the ladder is gone. `is_definitely_absent`
/// only fires on a `NotFound` with no cloud placeholder standing in for it —
/// anything else (here, `master.m3u8` displaced by a directory, which fails
/// with something other than `NotFound` on every platform this runs on)
/// must fall back to a directory listing rather than `None`: reading a
/// transient failure as "no ladder" would drop every real member from the
/// manifest this build, and a later permitted sweep could then delete a
/// perfectly good ladder that nothing ever re-registers.
#[test]
fn ladder_members_from_master_falls_back_to_a_directory_listing_when_the_gate_is_unreadable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ladder_dir = dir.path().join("clip.hls");
    std::fs::create_dir_all(&ladder_dir).unwrap();
    // A directory standing where the gate file belongs: read_to_string
    // fails with something other than NotFound, not with empty content.
    std::fs::create_dir_all(ladder_dir.join(HLS_MASTER_NAME)).unwrap();
    let today = hls_members(&VIDEO_LADDER[..4]);
    let real_members: Vec<&String> = today.iter().filter(|n| n.as_str() != HLS_MASTER_NAME).collect();
    for name in &real_members {
        std::fs::write(ladder_dir.join(name), format!("bytes for {name}")).unwrap();
    }

    let members = ladder_members_from_master(&ladder_dir)
        .expect("an unreadable (not absent) gate must fall back, never read as no ladder at all");

    for name in &real_members {
        assert!(members.contains(name), "{name} must survive the fallback: {members:?}");
    }
}

/// `ladder_members_from_master` only answers "what does the gate say" — this
/// proves the other reader that used to trust a raw directory listing,
/// `register_existing_ladders`, actually uses it: a surplus rung must not be
/// registered as a ready asset, the same failure mode that let it ride into
/// the manifest and the publish upload.
#[test]
fn register_existing_ladders_excludes_a_surplus_rung_left_by_a_bigger_ladder() {
    let staging = tempfile::tempdir().expect("tempdir");
    let ladder_dir = staging.path().join("videos").join("clip.hls");
    std::fs::create_dir_all(&ladder_dir).unwrap();
    std::fs::write(ladder_dir.join(HLS_MASTER_NAME), four_rung_master()).unwrap();
    let today = hls_members(&VIDEO_LADDER[..4]);
    // `today` puts the gate first (`hls_members`' own documented order) —
    // skip it here so the real playlist just written above survives.
    for name in today.iter().filter(|n| n.as_str() != HLS_MASTER_NAME) {
        std::fs::write(ladder_dir.join(name), format!("bytes for {name}")).unwrap();
    }
    std::fs::write(ladder_dir.join("v4.m3u8"), b"stale rung playlist").unwrap();
    std::fs::write(ladder_dir.join("v4.m4s"), b"stale rung segment").unwrap();

    let registry = crate::types::assets::AssetRegistry::new();
    let found = register_existing_ladders(staging.path(), &registry);
    assert_eq!(found, 1);

    let registered = registry.assets.read().unwrap();
    assert!(
        !registered.contains_key("videos/clip.hls/v4.m3u8"),
        "surplus playlist must not be registered: {:?}",
        registered.keys().collect::<Vec<_>>()
    );
    assert!(
        !registered.contains_key("videos/clip.hls/v4.m4s"),
        "surplus segment must not be registered: {:?}",
        registered.keys().collect::<Vec<_>>()
    );
    for name in &today {
        let key = format!("videos/clip.hls/{name}");
        assert!(registered.contains_key(&key), "{key} must be registered: {:?}", registered.keys().collect::<Vec<_>>());
    }
}

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
        .expect("a 1280x720 source gets a ladder");

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
    transforms.put(&record, crate::build::cache::RecordMode::Request).unwrap();
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

/// A silent source (no audio stream at all) must still get a ladder, and
/// that ladder must carry neither audio-group files nor an audio reference
/// in the master playlist. The defect this guards: `hls_members` lists
/// `alo.*`/`ahi.*` for every rung count regardless of the source, so
/// `produce_ladder` tried to read files ffmpeg correctly never wrote, and
/// the whole ladder failed every build.
#[test]
fn hls_ladder_for_a_silent_source_has_no_audio_groups() {
    let Some(bin) = real_ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("quiet.mov");
    if !synthesise_silent(&bin, &source, "1280x720") {
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
    let ladder_dir = staging.join("quiet.hls");

    let entries = produce_ladder(
        &ffmpeg,
        &source,
        "oid-quiet",
        &ladder_dir,
        dir.path(),
        &transforms,
        &VideoCompressionConfig::default(),
        None,
        None,
        None,
    )
    .expect("a silent source must not fail the ladder")
    .expect("a 1280x720 source gets a ladder");

    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    for group in [AudioGroup::Lean, AudioGroup::Clean] {
        let a = group.as_str();
        assert!(
            !names.iter().any(|n| n.contains(a)),
            "a silent source must not produce a {a} member: {names:?}"
        );
    }

    let master = std::fs::read_to_string(ladder_dir.join(HLS_MASTER_NAME)).unwrap();
    assert!(
        !master.contains("TYPE=AUDIO"),
        "master playlist for a silent source must not declare an audio group:\n{master}"
    );
    assert!(
        !master.contains("AUDIO=\""),
        "master playlist for a silent source must not bind a variant to an audio group:\n{master}"
    );
}

/// A source whose audio track is present but carries silence (`anullsrc`)
/// is NOT a silent source — it still has an audio stream, so it must keep
/// its audio groups exactly like a normal clip.
#[test]
fn hls_ladder_for_a_source_with_a_silent_audio_track_keeps_audio_groups() {
    let Some(bin) = real_ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("hush.mov");
    if !synthesise_with_silent_audio_track(&bin, &source, "1280x720") {
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
    let ladder_dir = staging.join("hush.hls");

    let entries = produce_ladder(
        &ffmpeg,
        &source,
        "oid-hush",
        &ladder_dir,
        dir.path(),
        &transforms,
        &VideoCompressionConfig::default(),
        None,
        None,
        None,
    )
    .expect("a source with a silent audio track must not fail the ladder")
    .expect("a 1280x720 source gets a ladder");

    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    for group in [AudioGroup::Lean, AudioGroup::Clean] {
        let a = group.as_str();
        assert!(
            names.iter().any(|n| n.contains(a)),
            "a source with an audio stream must keep its {a} member: {names:?}"
        );
    }

    let master = std::fs::read_to_string(ladder_dir.join(HLS_MASTER_NAME)).unwrap();
    assert!(
        master.contains("TYPE=AUDIO"),
        "master playlist for a source with an audio stream must declare an audio group:\n{master}"
    );
}

/// A silent source's cached ladder must hit the cache on a second build, the
/// same as a source with audio does. The defect this guards: a cache lookup
/// that assumes every rung carries an audio group counts the record's own
/// files against a list too long by four, reads that as a partial ladder,
/// and re-encodes a source that never changed — on every single build,
/// forever, never erroring but never caching either.
///
/// Proved the same way the audio-source cache-reuse test above is: the
/// source is deleted between the two calls, so a hit that still succeeds
/// cannot have re-probed or re-encoded anything.
#[test]
fn silent_source_hls_ladder_is_cached_not_reencoded_on_the_second_build() {
    let Some(bin) = real_ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("quiet.mov");
    if !synthesise_silent(&bin, &source, "1280x720") {
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
    let ladder_dir = staging.join("quiet.hls");

    let first = produce_ladder(
        &ffmpeg,
        &source,
        "oid-quiet-cache",
        &ladder_dir,
        dir.path(),
        &transforms,
        &VideoCompressionConfig::default(),
        None,
        None,
        None,
    )
    .expect("a silent source must not fail the ladder")
    .expect("a 1280x720 source gets a ladder");

    // `produce_ladder` only reads the cache; persisting what a run produced
    // is the caller's job (video.rs, after the whole conversion lands) —
    // mirrored here the same way the audio-source cache-reuse test above
    // does it.
    let mut record = TransformRecord {
        source_oid: "oid-quiet-cache".to_string(),
        source_size: 0,
        transforms: std::collections::HashMap::new(),
    };
    record.transforms.extend(first.iter().cloned());
    transforms.put(&record, crate::build::cache::RecordMode::Request).unwrap();

    // Take the source away: a second call that still succeeds must be a
    // cache hit, since there is nothing left to probe or encode.
    std::fs::remove_file(&source).unwrap();
    std::fs::remove_dir_all(&ladder_dir).unwrap();

    let second = produce_ladder(
        &ffmpeg,
        &source,
        "oid-quiet-cache",
        &ladder_dir,
        dir.path(),
        &transforms,
        &VideoCompressionConfig::default(),
        None,
        None,
        None,
    )
    .expect("cache hit")
    .expect("still a ladder");

    assert_eq!(
        first.iter().map(|(n, e)| (n.clone(), e.oid.clone())).collect::<Vec<_>>(),
        second.iter().map(|(n, e)| (n.clone(), e.oid.clone())).collect::<Vec<_>>(),
        "the cached run returns the same files"
    );
}

/// A source wide and short enough that the per-file budget never narrows it
/// under any config this file uses — the full table, deterministically,
/// everywhere it's fed to `effective_rungs`.
fn wide_source() -> SourceVideo {
    source_facts(1920, 1080)
}

/// A minute-long `width`x`height` source that is not web-playable (so its
/// web file is always encoded).
fn source_facts(width: u32, height: u32) -> SourceVideo {
    SourceVideo {
        width,
        height,
        sar: 1.0,
        duration_secs: 60.0,
        fps: 30.0,
        total_kbps: None,
        web_playable: false,
        has_audio: true,
    }
}

/// A partial ladder is a miss, not a hit: a ladder's files that reference each
/// other are only valid together, and one evicted blob makes the rest garbage.
#[test]
fn a_missing_rung_invalidates_the_whole_cached_ladder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let transforms = TransformCache::new(
        dir.path().join("transforms"),
        ObjectStore::new(dir.path().join("objects")),
    );
    let config = VideoCompressionConfig::default();
    let params = ladder_params(&config, &VIDEO_LADDER, &wide_source());
    // A real blob: `find_cached_output` checks the object store, so a record
    // pointing at nothing is a miss for a reason this test is not about.
    let blob = dir.path().join("blob");
    std::fs::write(&blob, b"ladder file").unwrap();
    let oid = transforms.objects().store_file(&blob, crate::build::cache::RecordMode::Request).unwrap();
    let members = hls_members(&video_ladder_rungs((1920, 1080)));
    assert_eq!(members.len(), 19, "seven rungs and two renditions");

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
        transforms.put(&record, crate::build::cache::RecordMode::Request).unwrap();
    };

    write_record(None);
    assert!(
        cached_ladder(&transforms, "oid", &config).is_some(),
        "a complete record is a hit"
    );

    write_record(Some("v3.m4s"));
    assert!(
        cached_ladder(&transforms, "oid", &config).is_none(),
        "one missing segment invalidates the whole ladder"
    );
}

/// `cached_ladder` must never probe the source — its whole point is to answer
/// a hit/miss question from the record alone. A pure, ffmpeg-less test is the
/// strongest proof of that: nothing in this test's reach even links an
/// `FFmpegManager`.
#[test]
fn cached_ladder_hits_a_new_form_record_with_no_probe_needed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let transforms = TransformCache::new(
        dir.path().join("transforms"),
        ObjectStore::new(dir.path().join("objects")),
    );
    let config = VideoCompressionConfig::default();
    let source = wide_source();
    let params = ladder_params(&config, &VIDEO_LADDER, &source);
    let blob = dir.path().join("blob");
    std::fs::write(&blob, b"ladder file").unwrap();
    let oid = transforms.objects().store_file(&blob, crate::build::cache::RecordMode::Request).unwrap();
    let members = hls_members(&VIDEO_LADDER);
    let mut record = TransformRecord {
        source_oid: "oid-no-probe".to_string(),
        source_size: 0,
        transforms: std::collections::HashMap::new(),
    };
    for name in &members {
        record
            .transforms
            .insert(format!("video/hls/{name}"), TransformEntry { oid: oid.clone(), size: 1, params: params.clone() });
    }
    transforms.put(&record, crate::build::cache::RecordMode::Request).unwrap();

    let (hit_members, oids, _) =
        cached_ladder(&transforms, "oid-no-probe", &config).expect("a new-form record is a hit with no probe");
    assert_eq!(hit_members, members);
    assert!(oids.iter().all(|o| o == &oid));
}

/// A full ladder record for `source_oid` in `transforms`, every member keyed
/// by `params`. The blobs are placeholders: these tests ask whether a record
/// is a hit, never what its files hold.
fn seed_ladder_record(transforms: &TransformCache, scratch: &Path, source_oid: &str, params: &serde_json::Value) {
    let blob = scratch.join("blob");
    std::fs::write(&blob, b"ladder file").unwrap();
    let oid = transforms.objects().store_file(&blob, crate::build::cache::RecordMode::Request).unwrap();
    let mut record = TransformRecord {
        source_oid: source_oid.to_string(),
        source_size: 0,
        transforms: std::collections::HashMap::new(),
    };
    for name in hls_members(&VIDEO_LADDER) {
        record
            .transforms
            .insert(format!("video/hls/{name}"), TransformEntry { oid: oid.clone(), size: 1, params: params.clone() });
    }
    transforms.put(&record, crate::build::cache::RecordMode::Request).unwrap();
}

/// A ladder from the format before today's is a miss, so the next build
/// re-encodes it once (format 2 aimed every rung at its rate).
#[test]
fn cached_ladder_misses_a_ladder_from_the_previous_format() {
    let dir = tempfile::tempdir().expect("tempdir");
    let transforms = TransformCache::new(
        dir.path().join("transforms"),
        ObjectStore::new(dir.path().join("objects")),
    );
    let config = VideoCompressionConfig::default();
    let mut params = ladder_params(&config, &VIDEO_LADDER, &wide_source());
    seed_ladder_record(&transforms, dir.path(), "oid-current", &params);
    assert!(cached_ladder(&transforms, "oid-current", &config).is_some(), "today's key is a hit");
    params["format"] = serde_json::json!(cache::LADDER_FORMAT - 1);
    seed_ladder_record(&transforms, dir.path(), "oid-previous", &params);
    assert!(cached_ladder(&transforms, "oid-previous", &config).is_none());
}

#[test]
fn cached_ladder_misses_a_record_keyed_before_aspect_fitted_frames() {
    let dir = tempfile::tempdir().expect("tempdir");
    let transforms = TransformCache::new(
        dir.path().join("transforms"),
        ObjectStore::new(dir.path().join("objects")),
    );
    let config = VideoCompressionConfig::default();
    let mut params = ladder_params(&config, &VIDEO_LADDER, &wide_source());
    assert!(params.as_object_mut().unwrap().remove("format").is_some(), "the key carries a format version");
    // ...and its stored source facts predate height and pixel aspect ratio.
    let facts = params["source"].as_object_mut().unwrap();
    assert!(facts.remove("height").is_some() && facts.remove("sar").is_some());
    seed_ladder_record(&transforms, dir.path(), "oid-old-frames", &params);

    assert!(cached_ladder(&transforms, "oid-old-frames", &config).is_none());
    // The no-probe heal path must not relink it either: it asks for a real
    // dispatch, where the source is probed afresh (height and all) and encoded.
    let heal = heal_cached_ladder(&transforms, "oid-old-frames", &dir.path().join("ladder"), &config)
        .expect("heal");
    assert_eq!(heal, LadderHeal::NeedsDispatch);
}

/// A budget edit that does not change what a PARTICULAR video's effective
/// ladder would be must not re-encode it — the whole point of keying on the
/// effect rather than the inputs. This source is nowhere near either budget's
/// ceiling (a 60 s, unknown-bitrate clip is a few MB a rung), so a looser
/// `hls_max_file_mb` changes nothing about it.
#[test]
fn cached_ladder_ignores_a_looser_budget_that_doesnt_change_the_effective_ladder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let transforms = TransformCache::new(
        dir.path().join("transforms"),
        ObjectStore::new(dir.path().join("objects")),
    );
    let recorded_under = VideoCompressionConfig { hls_max_file_mb: 150, ..VideoCompressionConfig::default() };
    let source = wide_source();
    let params = ladder_params(&recorded_under, &VIDEO_LADDER, &source);
    let blob = dir.path().join("blob");
    std::fs::write(&blob, b"ladder file").unwrap();
    let oid = transforms.objects().store_file(&blob, crate::build::cache::RecordMode::Request).unwrap();
    let members = hls_members(&VIDEO_LADDER);
    let mut record = TransformRecord {
        source_oid: "oid-loose".to_string(),
        source_size: 0,
        transforms: std::collections::HashMap::new(),
    };
    for name in &members {
        record
            .transforms
            .insert(format!("video/hls/{name}"), TransformEntry { oid: oid.clone(), size: 1, params: params.clone() });
    }
    transforms.put(&record, crate::build::cache::RecordMode::Request).unwrap();

    let read_back_under = VideoCompressionConfig { hls_max_file_mb: 500, ..VideoCompressionConfig::default() };
    assert!(
        cached_ladder(&transforms, "oid-loose", &read_back_under).is_some(),
        "a budget change this video's own ladder never felt must stay a hit"
    );
}

/// The other half: a budget edit that DOES narrow this source's effective
/// ladder must invalidate the cache — a 903.47 s source drops to 5 rungs
/// under the default 150 MiB budget (pinned in asset_paths' own tests), so
/// reading a record stored under a much looser budget back with the default
/// one must miss.
#[test]
fn cached_ladder_misses_when_a_tighter_budget_narrows_the_effective_ladder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let transforms = TransformCache::new(
        dir.path().join("transforms"),
        ObjectStore::new(dir.path().join("objects")),
    );
    let recorded_under = VideoCompressionConfig { hls_max_file_mb: 100_000, ..VideoCompressionConfig::default() };
    let source = SourceVideo { duration_secs: 903.47, ..source_facts(1920, 1080) };
    let params = ladder_params(&recorded_under, &VIDEO_LADDER, &source);
    let blob = dir.path().join("blob");
    std::fs::write(&blob, b"ladder file").unwrap();
    let oid = transforms.objects().store_file(&blob, crate::build::cache::RecordMode::Request).unwrap();
    let members = hls_members(&VIDEO_LADDER);
    let mut record = TransformRecord {
        source_oid: "oid-tight".to_string(),
        source_size: 0,
        transforms: std::collections::HashMap::new(),
    };
    for name in &members {
        record
            .transforms
            .insert(format!("video/hls/{name}"), TransformEntry { oid: oid.clone(), size: 1, params: params.clone() });
    }
    transforms.put(&record, crate::build::cache::RecordMode::Request).unwrap();

    let tight = VideoCompressionConfig::default();
    assert!(
        cached_ladder(&transforms, "oid-tight", &tight).is_none(),
        "a budget change that narrows this video's own effective ladder must miss"
    );
}

/// A ladder that SHRINKS — a tighter `hls_max_file_mb`, or a table edit
/// dropping the top rung — must be a cache HIT on the very next lookup, not a
/// permanent miss.
///
/// Reproduces exactly what a plain merge-only write left behind: a record
/// seeded as a complete 7-rung ladder recorded under a looser budget (`stale_
/// params`), then a fresh 5-rung ladder recorded through `video::record_
/// ladder` under today's tighter one. The 15 keys the two ladders share get
/// overwritten either way; what only `record_ladder`'s drop-before-insert
/// gets right is the four keys that do NOT overlap — the old top rungs'
/// `video/hls/v5.*` and `v6.*`. Left behind, `cached_ladder` counts 7 rungs
/// where 5 actually exist, recomputes 5 from the fresh entries' own stored
/// facts, and the length mismatch would be a miss, forever.
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
    // One physical source (903.47 s, 1920x1080): under a huge budget nothing
    // narrows it (7 rungs); under the default 150 MiB budget it narrows to 5
    // (pinned in asset_paths' own tests).
    let source_facts = SourceVideo { duration_secs: 903.47, ..source_facts(1920, 1080) };

    // The complete 7-rung ladder a looser budget left in the record.
    let stale_config =
        VideoCompressionConfig { hls_max_file_mb: 100_000, ..VideoCompressionConfig::default() };
    let stale_params = ladder_params(&stale_config, &VIDEO_LADDER, &source_facts);
    let stale_blob = dir.path().join("stale-blob");
    std::fs::write(&stale_blob, b"stale ladder file").unwrap();
    let stale_oid = transforms.objects().store_file(&stale_blob, crate::build::cache::RecordMode::Request).unwrap();
    let stale_members = hls_members(&video_ladder_rungs((1920, 1080)));
    assert_eq!(stale_members.len(), 19, "seven rungs and two renditions");
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
    transforms.put(&record, crate::build::cache::RecordMode::Request).unwrap();

    // The fresh 5-rung ladder `produce_ladder` records after the budget
    // narrowed it, the way `convert_single_video`'s `Ok(Some(entries))` arm
    // does: through `record_ladder`, not a plain merge.
    let config = VideoCompressionConfig::default();
    let fresh_members = hls_members(&VIDEO_LADDER[..5]);
    assert_eq!(fresh_members.len(), 15, "five rungs and two renditions");
    let fresh_params = ladder_params(&config, &VIDEO_LADDER[..5], &source_facts);
    let fresh_blob = dir.path().join("fresh-blob");
    std::fs::write(&fresh_blob, b"fresh ladder file").unwrap();
    let fresh_oid = transforms.objects().store_file(&fresh_blob, crate::build::cache::RecordMode::Request).unwrap();
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

    let (members, oids, _) = cached_ladder(&transforms, source_oid, &config)
        .expect("a shrunk ladder must stay a cache hit on the very next lookup");
    assert_eq!(members, fresh_members, "the shrunk ladder's own 5-rung file set, not the stale 6-rung one");
    assert!(
        oids.iter().all(|o| o == &fresh_oid),
        "every resolved oid must come from the fresh record, not the stale one left behind"
    );
}

/// A tiny per-file budget reaches all the way through a real encode: fewer
/// files land in staging than the same 1280x720 source gets from width
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

    // The clip is 4 s. At 4 s, the top rung's own video file (2000 kbps * 4 s
    // * 1.05 margin = 1,050,000 bytes) is just over a 1 MiB (1,048,576 byte)
    // cap while every rung below it clears that cap comfortably — a budget
    // picked to drop exactly the top rung, proving the truncation without
    // relying on width (1280 qualifies every rung by width alone). The web
    // file's own budget is 1 MB too, as it is for any video long enough for
    // the per-file one to bind: that places it at 960x540, which the
    // truncated ladder still reaches.
    let config = VideoCompressionConfig { hls_max_file_mb: 1, max_size_mb: 1, ..VideoCompressionConfig::default() };
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

// ── produce_ladder: a pre-rekey (legacy) cached ladder is re-encoded ──
//
// `legacy_form_params`/`LegacyShape` live in `hls::cache` (`#[cfg(test)]`,
// re-exported here via `super::*`) rather than as a copy in this file: the
// staging-heal tests in video.rs need the same real legacy shapes to seed a
// genuinely legacy-form record, and a second reconstruction of git history
// in a second file is exactly the kind of copy that drifts from the first.

/// A legacy record is never reused, whatever the source: its bytes predate
/// aspect-fitted frames and own-size top rungs, so `produce_ladder`
/// re-encodes it once — different oids, and the two rungs a 416x234 source
/// gets today instead of the legacy record's six.
#[test]
fn produce_ladder_re_encodes_a_legacy_record() {
    let Some(bin) = real_ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("legacy.mov");
    if !synthesise(&bin, &source, "416x234") {
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

    let config = VideoCompressionConfig::default();
    let source_oid = "oid-legacy";
    // The older of the two real legacy shapes — see `legacy_form_params`.
    let legacy_params = legacy_form_params(&config, LegacyShape::NoBudget);
    let members = hls_members(&VIDEO_LADDER);
    let fabricated_oids: Vec<String> = members
        .iter()
        .map(|name| transforms.objects().store_bytes(format!("legacy blob for {name}").as_bytes(), crate::build::cache::RecordMode::Request).unwrap())
        .collect();
    let mut record = TransformRecord {
        source_oid: source_oid.to_string(),
        source_size: 0,
        transforms: std::collections::HashMap::new(),
    };
    for (name, oid) in members.iter().zip(&fabricated_oids) {
        record.transforms.insert(
            format!("video/hls/{name}"),
            TransformEntry { oid: oid.clone(), size: 1, params: legacy_params.clone() },
        );
    }
    transforms.put(&record, crate::build::cache::RecordMode::Request).unwrap();

    let entries = produce_ladder(
        &ffmpeg,
        &source,
        source_oid,
        &staging.join("legacy.hls"),
        dir.path(),
        &transforms,
        &config,
        None,
        None,
        None,
    )
    .expect("produce_ladder")
    .expect("a 416x234 source has two rungs");

    let rung_count = entries
        .iter()
        .filter(|(n, _)| n.starts_with("video/hls/v") && n.ends_with(".m3u8"))
        .count();
    assert_eq!(
        rung_count, 2,
        "the 320x180 row and the source's own size: {entries:?}"
    );
    assert!(
        entries.iter().all(|(_, e)| !fabricated_oids.contains(&e.oid)),
        "none of the legacy record's placeholder blobs were reused"
    );
}

/// A lean recording between two table sizes gets a ladder that ends on its
/// own picture. A 1280x810 screen recording at 150 kbps used to get none,
/// because its bitrate could not fill the rung its width allowed, and before
/// that a ladder whose best picture was 416 pixels wide.
#[test]
fn a_lean_source_between_two_sizes_gets_a_ladder_ending_on_its_own_picture() {
    let Some(bin) = real_ffmpeg() else {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("lean.mp4");
    let ok = std::process::Command::new(&bin)
        .args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", "testsrc=size=1280x810:rate=30:duration=3",
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
            "-b:v", "150k", "-maxrate", "150k", "-bufsize", "300k",
        ])
        .arg(&source)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("skipping: could not synthesise a low-bitrate source");
        return;
    }
    let dir_t = dir.path();
    let transforms = TransformCache::new(dir_t.join("transforms"), ObjectStore::new(dir_t.join("objects")));
    let ladder_dir = dir_t.join("staging").join("lean.hls");
    produce_ladder(
        &FFmpegManager::from_bin_path(bin),
        &source,
        "oid-lean",
        &ladder_dir,
        dir_t,
        &transforms,
        &VideoCompressionConfig { preset: "ultrafast".to_string(), ..Default::default() },
        None,
        None,
        None,
    )
    .expect("produce_ladder")
    .expect("a ladder");
    let master = std::fs::read_to_string(ladder_dir.join(HLS_MASTER_NAME)).unwrap();
    let sizes: Vec<&str> = master
        .lines()
        .filter_map(|l| l.split(',').find_map(|f| f.strip_prefix("RESOLUTION=")))
        .collect();
    assert_eq!(
        sizes,
        ["284x180", "370x234", "568x360", "682x432", "854x540", "1280x810"],
        "the 1280x720 box would be 1138x720, too close to the source to be a rung: {master}"
    );
}

/// A ladder must reach the web file it is played instead of. Twenty minutes
/// of a lean 1280x810 recording is kept as its own file, and the per-file
/// budget leaves its ladder no rung that size, so it gets none. The same
/// recording as a `.mov` is encoded, and the size budget pulls that file
/// down to 568x360, which the ladder passes.
#[test]
fn a_ladder_is_built_only_when_it_reaches_the_web_files_size() {
    let config = VideoCompressionConfig::default();
    let kept = SourceVideo { duration_secs: 1200.0, total_kbps: Some(220.0), web_playable: true, ..source_facts(1280, 810) };
    assert_eq!(effective_rungs(&kept, &config).len(), 1);
    let encoded = SourceVideo { web_playable: false, ..kept };
    assert_eq!(effective_rungs(&encoded, &config), VIDEO_LADDER[..4]);
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

#[test]
fn each_rungs_scale_is_the_sources_shape_fitted_into_its_box_with_square_pixels() {
    // 3:2 source: a fixed 16:9 frame would squash it; each rung must keep 3:2.
    let a = build_hls_args("in.mov", "/o/c.hls", &VIDEO_LADDER, (1500, 1000), 30.0, true, "faster", 4);
    let filter = &a[a.iter().position(|x| x == "-filter_complex").unwrap() + 1];
    for (i, rung) in VIDEO_LADDER.iter().enumerate() {
        let (w, h) = rung.fitted_frame((1500, 1000));
        assert_ne!((w, h), (rung.width, rung.height), "3:2 must not land on the 16:9 box");
        assert!(
            filter.contains(&format!("[s{i}]scale={w}:{h},setsar=1")),
            "rung {i} wants {w}x{h} square-pixel: {filter}"
        );
    }
}

#[test]
fn master_patching_folds_audio_into_bandwidth_for_a_non_16_9_ladder() {
    // RESOLUTION no longer matches any table row, so the rung is found by the
    // variant's own playlist name; matching on size would silently skip this.
    let (w0, h0) = VIDEO_LADDER[0].fitted_frame((1500, 1000));
    let (w2, h2) = VIDEO_LADDER[2].fitted_frame((1500, 1000));
    let master = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n\
         #EXT-X-STREAM-INF:BANDWIDTH=45000,RESOLUTION={w0}x{h0},AUDIO=\"group_alo\"\nv0.m3u8\n\
         #EXT-X-STREAM-INF:BANDWIDTH=365000,RESOLUTION={w2}x{h2},AUDIO=\"group_ahi\"\nv2.m3u8\n"
    );
    let out = patch_master(&master, &VIDEO_LADDER, |_| None);
    assert!(out.contains("BANDWIDTH=77000"), "45k + 32k lean: {out}");
    assert!(out.contains("BANDWIDTH=557000"), "365k + 192k clean: {out}");
}
