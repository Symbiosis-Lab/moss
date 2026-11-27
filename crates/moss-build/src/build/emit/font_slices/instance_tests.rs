use super::*;
use read_fonts::tables::gpos::{PairPos as ReadPairPos, PositionSubtables, SinglePos as ReadSinglePos, ValueRecord as ReadValueRecord};

const SLICE: &[u8] = include_bytes!("../../../../tests/fixtures/fonts/noto-serif-tc-slice.ttf");
const FONTTOOLS_400: &[u8] = include_bytes!("../../../../tests/fixtures/fonts/noto-serif-tc-slice-400.ttf");
const FONTTOOLS_700: &[u8] = include_bytes!("../../../../tests/fixtures/fonts/noto-serif-tc-slice-700.ttf");
const STATIC: &[u8] = include_bytes!("../../../../tests/fixtures/fonts/han-boxes.ttf");

fn wght(weight: f32) -> Vec<u8> {
    instance(SLICE, &[(Tag::new(b"wght"), weight)]).expect("the slice instances")
}

/// Each glyph's points with their on-curve bits, then its four metrics.
fn glyphs(bytes: &[u8]) -> Vec<(Vec<(i32, i32, bool)>, [i32; 4])> {
    let font = FontRef::new(bytes).unwrap();
    let (glyf, loca, hmtx, vmtx) = (font.glyf().unwrap(), font.loca(None).unwrap(), font.hmtx().unwrap(), font.vmtx().unwrap());
    (0..font.maxp().unwrap().num_glyphs())
        .map(GlyphId::from)
        .map(|gid| {
            let points = match loca.get_glyf(gid, &glyf).unwrap() {
                Some(ReadGlyph::Simple(s)) => s.points().map(|p| (i32::from(p.x), i32::from(p.y), p.on_curve)).collect(),
                Some(ReadGlyph::Composite(_)) => panic!("the slice has no composite glyphs"),
                None => Vec::new(),
            };
            let metric = |v: Option<i16>| i32::from(v.unwrap());
            let advance = |v: Option<u16>| i32::from(v.unwrap());
            (points, [advance(hmtx.advance(gid)), metric(hmtx.side_bearing(gid)), advance(vmtx.advance(gid)), metric(vmtx.side_bearing(gid))])
        })
        .collect()
}

/// Every single and pair adjustment's four values, per covered glyph (a pair
/// lookup by class, per glyph pair), so an instancer that renumbers classes or
/// unwraps extension lookups still compares equal. A value the record leaves
/// out is 0.
fn adjustments(bytes: &[u8]) -> Vec<i32> {
    let font = FontRef::new(bytes).unwrap();
    let glyph_count = font.maxp().unwrap().num_glyphs();
    let mut out = Vec::new();
    let mut push = |r: &ReadValueRecord| out.extend([r.x_placement(), r.y_placement(), r.x_advance(), r.y_advance()].map(|v| i32::from(v.unwrap_or(0))));
    for lookup in font.gpos().unwrap().lookup_list().unwrap().lookups().iter().map(|r| r.unwrap()) {
        match lookup.subtables().unwrap() {
            PositionSubtables::Single(s) => s.iter().map(|r| r.unwrap()).for_each(|s| match s {
                ReadSinglePos::Format1(s) => s.coverage().unwrap().iter().for_each(|_| push(&s.value_record())),
                ReadSinglePos::Format2(s) => s.value_records().iter().map(|r| r.unwrap()).for_each(|r| push(&r)),
                _ => panic!("the slice has 16-bit glyph ids"),
            }),
            PositionSubtables::Pair(s) => s.iter().map(|r| r.unwrap()).for_each(|p| match p {
                ReadPairPos::Format1(p) => p.pair_sets().iter().map(|r| r.unwrap()).flat_map(|set| set.pair_value_records().iter().map(|r| r.unwrap()).collect::<Vec<_>>()).for_each(|r| {
                    push(r.value_record1());
                    push(r.value_record2());
                }),
                ReadPairPos::Format2(p) => {
                    let (first, second) = (p.class_def1().unwrap(), p.class_def2().unwrap());
                    for g1 in p.coverage().unwrap().iter() {
                        for g2 in (0..glyph_count).map(GlyphId::from) {
                            let [r1, r2] = p.value_record_refs(first.get(g1) as u16, second.get(g2) as u16).unwrap();
                            push(&r1);
                            push(&r2);
                        }
                    }
                }
                _ => panic!("the slice has 16-bit glyph ids"),
            }),
            _ => {}
        }
    }
    out
}

/// Every baseline coordinate of both axes, in table order.
fn baselines(bytes: &[u8]) -> Vec<i32> {
    let font = FontRef::new(bytes).unwrap();
    let base = font.base().unwrap();
    let mut out = Vec::new();
    for axis in [base.horiz_axis(), base.vert_axis()].into_iter().flatten().map(|r| r.unwrap()) {
        for record in axis.base_script_list().unwrap().base_script_records() {
            let script = record.base_script(axis.base_script_list().unwrap().offset_data()).unwrap();
            if let Some(values) = script.base_values().transpose().unwrap() {
                out.extend(values.base_coords().iter().map(|c| i32::from(c.unwrap().coordinate())));
            }
        }
    }
    out
}

fn within_one(ours: &[i32], theirs: &[i32], what: &str) {
    assert_eq!(ours.len(), theirs.len(), "{what}: count");
    for (i, (a, b)) in ours.iter().zip(theirs).enumerate() {
        assert!((a - b).abs() <= 1, "{what} {i}: moss {a}, fontTools {b}");
    }
}

#[test]
fn the_slice_at_400_and_700_matches_fonttools_instancer() {
    for (weight, reference) in [(400.0, FONTTOOLS_400), (700.0, FONTTOOLS_700)] {
        let ours = wght(weight);
        let (mine, theirs) = (glyphs(&ours), glyphs(reference));
        assert_eq!(mine.len(), theirs.len());
        for (gid, ((p, m), (q, n))) in mine.iter().zip(&theirs).enumerate() {
            assert_eq!(p.iter().map(|x| x.2).collect::<Vec<_>>(), q.iter().map(|x| x.2).collect::<Vec<_>>(), "glyph {gid} at {weight}: on-curve bits");
            within_one(&p.iter().flat_map(|x| [x.0, x.1]).collect::<Vec<_>>(), &q.iter().flat_map(|x| [x.0, x.1]).collect::<Vec<_>>(), &format!("glyph {gid} at {weight}"));
            within_one(m, n, &format!("glyph {gid} metrics at {weight}"));
        }
        within_one(&adjustments(&ours), &adjustments(reference), &format!("GPOS at {weight}"));
        within_one(&baselines(&ours), &baselines(reference), &format!("BASE at {weight}"));
        let font = FontRef::new(&ours).unwrap();
        assert_eq!(font.os2().unwrap().us_weight_class(), weight as u16);
        for tag in DROPPED {
            assert!(font.table_data(Tag::new(tag)).is_none(), "{} is dropped", String::from_utf8_lossy(tag));
        }
        assert!(font.gdef().unwrap().item_var_store().is_none());
    }
}

/// Guards the comparison above against passing on a font that never moved.
#[test]
fn the_reference_instances_differ_from_the_default_master() {
    for reference in [FONTTOOLS_400, FONTTOOLS_700] {
        assert_ne!(glyphs(reference).into_iter().map(|g| g.0).collect::<Vec<_>>(), glyphs(SLICE).into_iter().map(|g| g.0).collect::<Vec<_>>());
        assert_ne!(adjustments(reference), adjustments(SLICE));
        assert_ne!(baselines(reference), baselines(SLICE));
    }
}

#[test]
fn an_instance_is_smaller_than_the_variable_slice() {
    assert!(wght(400.0).len() < SLICE.len(), "{} against {}", wght(400.0).len(), SLICE.len());
}

#[test]
fn a_font_with_no_axes_is_not_instanced() {
    assert_eq!(instance(STATIC, &[(Tag::new(b"wght"), 700.0)]), Err(Unsupported::NotVariable));
}

#[test]
fn a_pair_table_whose_slice_emptied_a_class_is_renumbered_and_writes() {
    use read_fonts::types::GlyphId16;
    use write_fonts::tables::gpos::ValueRecord as Value;
    use write_fonts::tables::layout::CoverageTable;
    let g = GlyphId16::new;
    let cell = |v: i16| Class2Record::new(Value::default().with_x_advance(v), Value::default());
    // Class 1 on each side lost its glyphs; its row and column remain.
    let mut p = PairPosFormat2::new(
        CoverageTable::from(vec![g(1), g(2)]),
        [(g(2), 2u16)].into_iter().collect(),
        [(g(3), 2u16)].into_iter().collect(),
        (0..3).map(|row| Class1Record::new((0..3).map(|col| cell(10 * row + col)).collect())).collect(),
    );
    assert!(write_fonts::dump_table(&p).is_err());
    dense_classes(&mut p).unwrap();
    write_fonts::dump_table(&p).expect("the renumbered table writes");
    let advances: Vec<Vec<i16>> = p.class1_records.iter().map(|r| r.class2_records.iter().map(|c| c.value_record1.x_advance.unwrap()).collect()).collect();
    assert_eq!(advances, [[0, 2], [20, 22]]);
    assert_eq!(p.class_def1.iter().collect::<Vec<_>>(), [(GlyphId::new(2), 1)]);
    assert_eq!(p.class_def2.iter().collect::<Vec<_>>(), [(GlyphId::new(3), 1)]);
}

#[test]
fn a_pair_table_the_slice_left_with_no_records_becomes_zeros_and_writes() {
    use read_fonts::types::GlyphId16;
    use write_fonts::tables::layout::CoverageTable;
    let g = GlyphId16::new;
    let mut p = PairPosFormat2::new(CoverageTable::from(vec![g(1), g(2)]), [(g(2), 1u16)].into_iter().collect(), [(g(3), 2u16)].into_iter().collect(), Vec::new());
    assert!(write_fonts::dump_table(&p).is_err());
    dense_classes(&mut p).unwrap();
    write_fonts::dump_table(&p).expect("the zero table writes");
    assert_eq!(p.class1_records.len(), 2);
    assert!(p.class1_records.iter().all(|r| r.class2_records.len() == 2 && r.class2_records.iter().all(|c| c.value_record1.x_advance.is_none())));
}


/// A slice moss cut from Noto Serif TC whose pair tables the subsetter left
/// with class tables and no records.
const EMPTY_PAIRS: &[u8] = include_bytes!("../../../../tests/fixtures/fonts/noto-serif-tc-slice-empty-pairs.ttf");

#[test]
fn a_slice_whose_pair_tables_lost_every_record_is_instanced() {
    let pinned = instance(EMPTY_PAIRS, &[(Tag::new(b"wght"), 400.0)]).expect("the slice instances");
    assert!(FontRef::new(&pinned).unwrap().fvar().is_err());
}
