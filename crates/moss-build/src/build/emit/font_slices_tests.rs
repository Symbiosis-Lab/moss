use super::*;

const HAN: &[u8] = include_bytes!("../../../tests/fixtures/fonts/han-boxes.ttf");
const HAN_NO_SUBSET: &[u8] = include_bytes!("../../../tests/fixtures/fonts/han-boxes-no-subset.ttf");
const HAN_WGHT: &[u8] = include_bytes!("../../../tests/fixtures/fonts/han-boxes-wght.ttf");
const HAN_WGHT_COMPOSITE: &[u8] = include_bytes!("../../../tests/fixtures/fonts/han-boxes-wght-composite.ttf");
const HAN_BAD_GSUB: &[u8] = include_bytes!("../../../tests/fixtures/fonts/han-boxes-bad-gsub.ttf");
const LATIN: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.otf");

#[test]
fn every_table_decodes_to_disjoint_subsets() {
    assert_eq!(TABLES.iter().map(|t| (t.subsets.len(), t.frequency)).collect::<Vec<_>>(), [(120, 20), (120, 20), (120, 20), (120, 60), (120, 20)]);
    for table in TABLES.iter() {
        let mut all: Vec<u32> = table.subsets.iter().flatten().copied().collect();
        let listed = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), listed, "a code point is listed twice");
        assert!(table.subsets[..2].iter().any(|s| s.contains(&u32::from('A'))), "ASCII is among the slices most pages load");
    }
}

#[test]
fn the_language_alone_picks_the_table() {
    let cases = [("zh-Hant", 0), ("zh-TW", 0), ("zh_hant_tw", 0), ("zh-HK", 1), ("zh-Hant-HK", 1), ("zh-MO", 1), ("zh", 2), ("zh-CN", 2), ("zh-Hans", 2), ("ja", 3), ("ja-JP", 3), ("ko", 4), ("en", DEFAULT_STRATEGY), ("", DEFAULT_STRATEGY)];
    for (lang, want) in cases {
        assert_eq!(strategy_for_language(lang), want, "{lang}");
    }
}

#[test]
fn every_code_point_lands_in_exactly_one_slice() {
    let font = sliceable_codepoints(HAN).unwrap();
    let slices = plan(&font, 0);
    let mut all: Vec<u32> = slices.iter().flat_map(|s| s.codepoints.clone()).collect();
    all.sort_unstable();
    assert_eq!(all, font);
    assert!(slices.windows(2).all(|w| w[0].id < w[1].id));
}

#[test]
fn the_space_is_in_the_first_slice_whatever_the_table_says() {
    for strategy in 0..STRATEGIES.len() {
        let slices = plan(&[0x20, 0x4e00], strategy);
        assert_eq!(slices[0].id, 0);
        assert!(slices[0].codepoints.contains(&0x20), "table {strategy}");
    }
}

#[test]
fn a_code_point_no_table_lists_goes_to_an_overflow_slice_by_code_point() {
    let base = TABLES[0].subsets.len() as u16;
    let unlisted: Vec<u32> = (0x10FF00..0x10FF00 + OVERFLOW_SLICE_LEN as u32 + 1).collect();
    let slices = plan(&unlisted, 0);
    assert_eq!(slices.iter().map(|s| (s.id, s.codepoints.len())).collect::<Vec<_>>(), [(base, OVERFLOW_SLICE_LEN), (base + 1, 1)]);
}

/// 攁 sits inside the traditional-chinese bins' span and in none of them;
/// the table leaves out thousands like it, which joined to the bin before
/// them made slices of up to 1,700 characters.
#[test]
fn an_unlisted_code_point_inside_the_bins_goes_to_overflow_not_the_bin_before_it() {
    let listed_before = *TABLES[0].subsets.iter().flatten().filter(|&&cp| cp < 0x6501).max().unwrap();
    let slices = plan(&[listed_before, 0x6501], 0);
    let base = TABLES[0].subsets.len() as u16;
    assert_eq!(slices.iter().map(|s| s.codepoints.len()).collect::<Vec<_>>(), [1, 1]);
    assert_eq!(slices[1], Slice { id: base, codepoints: vec![0x6501] });
}

#[test]
fn a_feature_a_stylesheet_turns_on_is_kept_and_one_it_does_not_is_dropped() {
    use read_fonts::TableProvider;
    const RUBY: &[u8] = include_bytes!("../../../tests/fixtures/fonts/noto-serif-tc-ruby.ttf");
    let tags = |sfnt: &[u8]| -> Vec<[u8; 4]> {
        let font = read_fonts::FontRef::new(sfnt).unwrap();
        let gsub = font.gsub().unwrap();
        gsub.feature_list().unwrap().feature_records().iter().map(|r| r.feature_tag().to_be_bytes()).collect()
    };
    let asked = kept_features("rt{font-variant-east-asian: ruby}");
    assert!(asked.contains(b"ruby"));
    assert!(tags(&subset(RUBY, &[0x2C7, 0x2CA], &asked).unwrap()).contains(b"ruby"));
    let plain = subset(RUBY, &[0x2C7, 0x2CA], &kept_features("p{margin:0}")).unwrap();
    assert!(!tags(&plain).contains(b"ruby"));
    assert!(tags(&plain).contains(b"vert"), "vertical forms are in the default list");
    assert!(!tags(&plain).contains(b"aalt"));
}

#[test]
fn a_latin_font_a_restricted_font_and_a_non_font_are_published_whole() {
    assert_eq!(sliceable_codepoints(LATIN), Err(Whole::NotHan));
    assert_eq!(sliceable_codepoints(HAN_NO_SUBSET), Err(Whole::LicenceForbids));
    assert_eq!(sliceable_codepoints(b"not a font"), Err(Whole::Unreadable));
}

#[test]
fn a_slice_encodes_to_woff2_holding_only_its_characters() {
    let woff2 = encode(HAN, &[0x20, 0x4e00, 0x4e01], None, &[]).unwrap();
    assert_eq!(&woff2[..4], b"wOF2");
    let sfnt = subset(HAN, &[0x20, 0x4e00, 0x4e01], &[]).unwrap();
    let cmap = ttf_parser::Face::parse(&sfnt, 0).unwrap().tables().cmap.unwrap();
    let mut mapped = Vec::new();
    for table in cmap.subtables {
        table.codepoints(|cp| mapped.push(cp));
    }
    mapped.sort_unstable();
    mapped.dedup();
    assert_eq!(mapped, [0x20, 0x4e00, 0x4e01]);
    assert_eq!(encode(HAN, &[0x4e00], None, &[]).unwrap(), encode(HAN, &[0x4e00], None, &[]).unwrap(), "the same inputs give the same bytes");
}

#[test]
fn a_slice_drops_the_fonts_hinting() {
    use read_fonts::types::Tag;
    let sfnt = subset(HAN, &[0x20, 0x4e00], &[]).unwrap();
    let font = read_fonts::FontRef::new(&sfnt).unwrap();
    for tag in [b"fpgm", b"prep", b"cvt "] {
        assert!(read_fonts::FontRef::new(HAN).unwrap().table_data(Tag::new(tag)).is_some(), "the fixture is hinted");
        assert!(font.table_data(Tag::new(tag)).is_none(), "{} kept", String::from_utf8_lossy(tag));
    }
}

/// The transform version WOFF2's table directory gives `glyf` (0: transformed).
fn glyf_transform_version(woff2: &[u8]) -> u8 {
    let mut at = 48;
    for _ in 0..u16::from_be_bytes([woff2[12], woff2[13]]) {
        let flags = woff2[at];
        at += if flags & 0x3f == 63 { 5 } else { 1 };
        let (tag, version) = (flags & 0x3f, flags >> 6);
        let mut base128 = || while { at += 1; woff2[at - 1] & 0x80 != 0 } {};
        base128();
        if tag == 10 {
            return version;
        }
        if (tag == 10 || tag == 11) == (version == 0) {
            base128();
        }
    }
    panic!("no glyf table");
}

#[test]
fn a_slice_whose_offsets_fit_takes_the_glyf_transform() {
    assert_eq!(glyf_transform_version(&encode(HAN, &[0x20, 0x4e00, 0x4e01], None, &[]).unwrap()), 0);
    assert!(short_loca_holds(0x1FFFE - 300, 100));
    assert!(!short_loca_holds(0x1FFFE - 299, 100));
}

/// The code points a `unicode-range` value covers.
fn covered(range: &str) -> Vec<std::ops::RangeInclusive<u32>> {
    let hex = |h: &str| u32::from_str_radix(h, 16).unwrap();
    range.split(',').map(|r| { let r = r.trim().trim_start_matches("U+"); match r.split_once('-') { Some((a, b)) => hex(a)..=hex(b), None => hex(r)..=hex(r) } }).collect()
}

/// A browser fetches, for each character, the last-written face whose range
/// covers it (CSS Fonts 4, overlapping ranges are checked in reverse order).
#[test]
fn the_last_written_face_covering_a_code_point_is_its_slice_and_no_range_covers_one_the_font_lacks() {
    let sliced = slice_faces("@font-face{font-family:H;src:url(fonts/Han.ttf)}", 0, read_fixture);
    let font = sliceable_codepoints(HAN).unwrap();
    let rules: Vec<(&SliceJob, Vec<std::ops::RangeInclusive<u32>>)> = faces(&sliced.css)
        .into_iter()
        .zip(sliced.css.match_indices("woff2\") format"))
        .map(|((_, range), (at, _))| {
            let url = &sliced.css[..at];
            let job = sliced.jobs.iter().find(|j| url.ends_with(&format!("{}.", j.name))).unwrap();
            (job, covered(&range))
        })
        .collect();
    assert_eq!(rules.len(), sliced.jobs.len());
    assert!(rules.iter().any(|(_, r)| r.iter().any(|r| r.start() != r.end())), "no range folds");
    for (job, ranges) in &rules {
        for cp in ranges.iter().flat_map(|r| r.clone()) {
            assert!(font.binary_search(&cp).is_ok(), "{} covers U+{cp:X}, which the font lacks", job.name);
        }
    }
    for &cp in &font {
        let (job, _) = rules.iter().rev().find(|(_, r)| r.iter().any(|r| r.contains(&cp))).unwrap();
        assert!(job.codepoints.contains(&cp), "U+{cp:X} would load {}, which lacks it", job.name);
    }
}

#[test]
fn a_slice_is_named_by_its_inputs() {
    let a = slice_name("f00d", 0, 3, None, &[]);
    assert_eq!(a, slice_name("f00d", 0, 3, None, &[]));
    assert_ne!(a, slice_name("f00d", 0, 4, None, &[]));
    assert_ne!(a, slice_name("f00d", 2, 3, None, &[]));
    assert_ne!(a, slice_name("beef", 0, 3, None, &[]));
    assert_ne!(a, slice_name("f00d", 0, 3, Some(400), &[]));
    assert_ne!(slice_name("f00d", 0, 3, Some(400), &[]), slice_name("f00d", 0, 3, Some(700), &[]));
    assert_ne!(a, slice_name("f00d", 0, 3, None, &[*b"ruby"]));
}

#[test]
fn the_slices_of_one_font_repeat_one_hash_so_their_rules_compress() {
    let hash = |name: String| name.split('-').next().unwrap().to_owned();
    assert_eq!(hash(slice_name("f00d", 0, 3, Some(400), &[])), hash(slice_name("f00d", 0, 4, Some(700), &[])));
    assert_eq!(hash(slice_name("f00d", 0, 3, None, &[])), hash(slice_name("f00d", 0, 9, None, &[])));
}

fn read_fixture(rel: &str) -> Option<Vec<u8>> {
    match rel {
        "fonts/Han.ttf" => Some(HAN.to_vec()),
        "fonts/Locked.ttf" => Some(HAN_NO_SUBSET.to_vec()),
        "fonts/Inter.otf" => Some(LATIN.to_vec()),
        "fonts/Var.ttf" => Some(HAN_WGHT.to_vec()),
        "fonts/VarComposite.ttf" => Some(HAN_WGHT_COMPOSITE.to_vec()),
        "fonts/BadGsub.ttf" => Some(HAN_BAD_GSUB.to_vec()),
        _ => None,
    }
}

#[test]
fn a_whole_han_face_becomes_one_rule_per_slice_keeping_its_descriptors() {
    let css = "body{color:red}\n@font-face { font-family: \"Han\"; font-weight: 200 900; font-display: swap; src: url(\"fonts/Han.ttf\") format(\"truetype\"); }\np{margin:0}";
    let sliced = slice_faces(css, 0, read_fixture);
    let slices = plan(&sliceable_codepoints(HAN).unwrap(), 0);
    assert_eq!(sliced.jobs.len(), slices.len());
    assert_eq!(sliced.css.matches("@font-face").count(), slices.len());
    assert!(sliced.css.starts_with("body{color:red}\n@font-face{font-family:\"Han\";font-weight:200 900;font-display:swap;src:url(\"fonts/Han."));
    assert!(sliced.css.ends_with("}\n\np{margin:0}"), "{}", &sliced.css[sliced.css.len() - 40..]);
    let first = sliced.jobs.iter().find(|j| j.codepoints.contains(&0x20)).unwrap();
    assert_eq!(first.path.as_str(), format!("_moss/theme/fonts/Han.{}.woff2", slice_name(&crate::build::assets::paths::compute_binary_hash(HAN), 0, slices[0].id, None, &kept_features(css))));
    assert!(sliced.css.contains(&format!("url(\"fonts/Han.{}.woff2\") format(\"woff2\");unicode-range:", first.name)));
}

#[test]
fn a_face_with_a_range_a_latin_face_and_a_missing_file_are_left_as_written() {
    let css = "@font-face{font-family:A;src:url(fonts/Han.ttf);unicode-range:U+4E00-4FFF}\
        @font-face{font-family:B;src:url(fonts/Inter.otf)}\
        @font-face{font-family:C;src:url(fonts/Gone.ttf)}\
        @font-face{font-family:D;src:url(fonts/Han.woff2) format('woff2')}\
        @media print{@font-face{font-family:E;src:url(fonts/Han.ttf)}}";
    let sliced = slice_faces(css, 0, read_fixture);
    assert_eq!(sliced.css, css);
    assert!(sliced.jobs.is_empty() && sliced.notes.is_empty());
}

#[test]
fn a_face_whose_licence_forbids_subsetting_is_published_whole_with_a_note() {
    let css = "@font-face{font-family:L;src:url(fonts/Locked.ttf)}";
    let sliced = slice_faces(css, 0, read_fixture);
    assert_eq!(sliced.css, css);
    assert_eq!(sliced.notes, ["theme font fonts/Locked.ttf is published whole: its licence does not allow subsetting"]);
}

#[test]
fn a_slice_is_encoded_once_and_then_read_from_the_machine_cache() {
    let sliced = slice_faces("@font-face{font-family:H;src:url(fonts/Han.ttf)}", 0, read_fixture);
    let job = &sliced.jobs[0];
    let cached = cache_dir().unwrap().join(format!("{}.woff2", job.name));
    let _ = std::fs::remove_file(&cached);
    let bytes = encoded(job).unwrap();
    assert_eq!(std::fs::read(&cached).unwrap(), bytes);
    std::fs::write(&cached, b"stand-in").unwrap();
    assert_eq!(encoded(job).unwrap(), b"stand-in", "the second read comes from the cache");
    std::fs::remove_file(&cached).unwrap();
}

#[test]
fn a_font_the_subsetter_refuses_is_published_whole_with_one_note() {
    let css = "@font-face{font-family:B;src:url(fonts/BadGsub.ttf)}";
    let sliced = slice_faces(css, 0, read_fixture);
    assert_eq!(sliced.css, css);
    assert!(sliced.jobs.is_empty());
    assert_eq!(sliced.notes.len(), 1);
    assert!(sliced.notes[0].starts_with("theme font fonts/BadGsub.ttf is published whole: it could not be subset"), "{}", sliced.notes[0]);
}

#[test]
fn a_variable_font_the_instancer_cannot_pin_is_sliced_variable_with_one_note() {
    let sliced = slice_faces("@font-face{font-family:V;font-weight:700;src:url(fonts/VarComposite.ttf)}", 0, read_fixture);
    assert!(!sliced.jobs.is_empty());
    assert!(sliced.jobs.iter().all(|j| j.weight.is_none()));
    assert_eq!(sliced.notes, ["theme font fonts/VarComposite.ttf is published variable, not pinned at each weight: it has composite glyphs"]);
    assert_eq!(encode(HAN_WGHT_COMPOSITE, &[0x4e01], Some(700), &[]), Err("it has composite glyphs".to_owned()));
}

#[test]
fn a_slice_the_encoder_fails_on_publishes_nothing_and_says_why() {
    let job = SliceJob { path: crate::build::served_path::ServedPath::for_theme_asset("fonts/Bad.0000000000000000.woff2").unwrap(), name: "not-cached-bad-font".into(), font: std::sync::Arc::new(b"not a font".to_vec()), codepoints: vec![0x4e00], weight: None, features: Default::default() };
    assert!(encoded(&job).unwrap_err().starts_with("theme font slice _moss/theme/fonts/Bad.0000000000000000.woff2 could not be encoded"));
}

/// A slice's name does not see the font crates' output, so a change to it
/// must move `ENCODER_VERSION`, which renames every slice.
#[test]
fn a_pinned_slice_encodes_to_the_bytes_its_encoder_version_names() {
    let woff2 = encode(HAN_WGHT, &[0x20, 0x4e00, 0x4e01, 0x4e02], Some(700), &[]).unwrap();
    assert_eq!((ENCODER_VERSION, crate::build::assets::paths::compute_binary_hash(&woff2).as_str()), (5, "4068f40e6ded12ff"), "the encoder's output changed: move ENCODER_VERSION and this hash together");
}

/// The slice rules of `css`, as (font-weight, unicode-range) in order.
fn faces(css: &str) -> Vec<(String, String)> {
    crate::build::theme_lint::style_rules(css)
        .into_iter()
        .filter(|rule| rule.prelude.trim() == "@font-face")
        .map(|rule| {
            let d = rule.declarations();
            let get = |name: &str| d.iter().find(|(p, _)| p == name).map_or(String::new(), |(_, v)| v.clone());
            (get("font-weight"), get("unicode-range"))
        })
        .collect()
}

#[test]
fn faces_of_one_variable_file_are_pinned_at_their_weights_and_written_side_by_side() {
    let css = "@font-face{font-family:V;font-weight:400;src:url(fonts/Var.ttf)}\nh1{margin:0}\n@font-face{font-family:V;font-weight:bold;src:url(fonts/Var.ttf)}";
    let sliced = slice_faces(css, 0, read_fixture);
    let slices = plan(&sliceable_codepoints(HAN_WGHT).unwrap(), 0);
    let written = faces(&sliced.css);
    assert_eq!(written.len(), 2 * slices.len());
    for pair in written.chunks(2) {
        assert_eq!([pair[0].0.as_str(), pair[1].0.as_str()], ["400", "700"]);
        assert_eq!(pair[0].1, pair[1].1);
    }
    assert!(sliced.css.ends_with("}\n\nh1{margin:0}\n"), "{}", &sliced.css[sliced.css.len() - 40..]);
    assert_eq!(sliced.jobs.iter().map(|j| j.weight).take(2).collect::<Vec<_>>(), [Some(400), Some(700)]);
    assert_ne!(sliced.jobs[0].name, sliced.jobs[1].name);
}

#[test]
fn a_variable_face_with_no_weight_is_pinned_at_400() {
    let sliced = slice_faces("@font-face{font-family:V;src:url(fonts/Var.ttf)}", 0, read_fixture);
    assert!(sliced.jobs.iter().all(|j| j.weight == Some(400)));
    assert!(faces(&sliced.css).iter().all(|(w, _)| w == "400"));
}

#[test]
fn a_weight_range_font_variation_settings_or_a_static_font_keep_their_axes() {
    let range = slice_faces("@font-face{font-family:V;font-weight:100 900;src:url(fonts/Var.ttf)}", 0, read_fixture);
    assert!(range.jobs.iter().all(|j| j.weight.is_none()));
    assert!(faces(&range.css).iter().all(|(w, _)| w == "100 900"));
    let settings = slice_faces("@font-face{font-family:V;font-weight:700;src:url(fonts/Var.ttf)}p{font-variation-settings:'wght' 650}", 0, read_fixture);
    assert!(settings.jobs.iter().all(|j| j.weight.is_none()));
    let fixed = slice_faces("@font-face{font-family:H;font-weight:700;src:url(fonts/Han.ttf)}", 0, read_fixture);
    assert!(fixed.jobs.iter().all(|j| j.weight.is_none()));
}

#[test]
fn a_pinned_slice_is_smaller_than_the_variable_one() {
    let codepoints = [0x20, 0x4e00, 0x4e01, 0x4e02];
    let (variable, pinned) = (encode(HAN_WGHT, &codepoints, None, &[]).unwrap(), encode(HAN_WGHT, &codepoints, Some(700), &[]).unwrap());
    assert!(pinned.len() < variable.len(), "{} against {}", pinned.len(), variable.len());
}
