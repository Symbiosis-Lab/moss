//! A static instance of a variable TrueType font: its outlines, metrics and
//! layout values at one point of its design space, with the variation tables
//! dropped. A slice instanced at one weight is about half the bytes of the
//! variable slice it came from, since `gvar` holds a delta set for every point.
//!
//! Font bytes in, font bytes out. The reference is fontTools'
//! `varLib.instancer`; the tests hold its output as golden vectors.

use read_fonts::tables::glyf::{CurvePoint, Glyph as ReadGlyph};
use read_fonts::tables::variations::{DeltaSetIndex, ItemVariationStore};
use read_fonts::types::{F2Dot14, GlyphId, Tag};
use read_fonts::{FontRef, TableProvider};
use write_fonts::from_obj::ToOwnedTable;
use write_fonts::tables::base::{Base, BaseCoord, MinMax};
use write_fonts::tables::glyf::{Bbox, Contour, GlyfLocaBuilder, Glyph, SimpleGlyph};
use write_fonts::tables::gpos::{
    AnchorTable, CursivePos, ExtensionSubtable, Gpos, MarkArray, MarkBasePos, MarkLigPos, MarkMarkPos, PairPos, PairPosFormat2, PositionLookup,
    SinglePos, ValueFormat, ValueRecord, Class1Record, Class2Record,
};
use write_fonts::tables::hmtx::{Hmtx, LongMetric};
use write_fonts::tables::layout::{ClassDef, DeviceOrVariationIndex, Lookup};
use write_fonts::tables::vmtx::Vmtx;
use write_fonts::{FontBuilder, NullableOffsetMarker};

/// Why a font was not instanced. The caller publishes the variable font
/// instead, which draws the same at the weight a stylesheet asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Unsupported {
    NotVariable,
    /// Outlines in CFF2 or composite glyphs, neither handled here.
    Outlines(&'static str),
    /// A variation this does not apply: `MVAR` or feature variations.
    Variation(&'static str),
    Malformed(String),
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotVariable => f.write_str("it is not a variable font"),
            Self::Outlines(what) | Self::Variation(what) => write!(f, "it has {what}"),
            Self::Malformed(e) => write!(f, "it could not be read ({e})"),
        }
    }
}

type Result<T> = std::result::Result<T, Unsupported>;

fn malformed(e: impl std::fmt::Display) -> Unsupported {
    Unsupported::Malformed(e.to_string())
}

/// Variation tables, which a static instance has no use for. Hinting
/// variations (`cvar`) go too: a slice carries no hinting.
const DROPPED: [&[u8; 4]; 8] = [b"fvar", b"gvar", b"avar", b"HVAR", b"VVAR", b"STAT", b"cvar", b"MVAR"];

/// `font` at `location`, given in user units (`wght` 700). An axis the
/// location does not name stays at its default.
pub fn instance(font: &[u8], location: &[(Tag, f32)]) -> Result<Vec<u8>> {
    check(font)?;
    let font = FontRef::new(font).map_err(malformed)?;
    let coords = normalized(&font, location)?;
    let outlines = outlines(&font, &coords)?;
    let mut builder = FontBuilder::new();
    let (glyf, loca, format) = outlines.glyf.build();
    builder.add_table(&glyf).map_err(malformed)?;
    builder.add_table(&loca).map_err(malformed)?;
    let mut head: write_fonts::tables::head::Head = font.head().map_err(malformed)?.to_owned_table();
    let bbox = outlines.bbox.unwrap_or_default();
    (head.x_min, head.y_min, head.x_max, head.y_max) = (bbox.x_min, bbox.y_min, bbox.x_max, bbox.y_max);
    head.index_to_loc_format = format as i16;
    builder.add_table(&head).map_err(malformed)?;

    let mut hhea: write_fonts::tables::hhea::Hhea = font.hhea().map_err(malformed)?.to_owned_table();
    let h = side_metrics(&outlines.h, outlines.extents.iter().map(|e| e.map(|b| (b.x_min, b.x_max))));
    hhea.number_of_h_metrics = h.count;
    hhea.advance_width_max = h.advance_max.into();
    hhea.min_left_side_bearing = h.min_before.into();
    hhea.min_right_side_bearing = h.min_after.into();
    hhea.x_max_extent = h.max_extent.into();
    builder.add_table(&hhea).map_err(malformed)?;
    builder.add_table(&Hmtx::new(outlines.h.clone(), Vec::new())).map_err(malformed)?;
    if let (Some(v), Ok(vhea)) = (&outlines.v, font.vhea()) {
        let mut vhea: write_fonts::tables::vhea::Vhea = vhea.to_owned_table();
        // Top to bottom: the side bearing is from yMax, the extent runs down to yMin.
        let m = side_metrics(v, outlines.extents.iter().map(|e| e.map(|b| (-b.y_max, -b.y_min))));
        vhea.number_of_long_ver_metrics = m.count;
        vhea.advance_height_max = m.advance_max.into();
        vhea.min_top_side_bearing = m.min_before.into();
        vhea.min_bottom_side_bearing = m.min_after.into();
        vhea.y_max_extent = m.max_extent.into();
        builder.add_table(&vhea).map_err(malformed)?;
        builder.add_table(&Vmtx::new(v.clone(), Vec::new())).map_err(malformed)?;
    }
    if let Ok(os2) = font.os2() {
        let mut os2: write_fonts::tables::os2::Os2 = os2.to_owned_table();
        if let Some(&(_, wght)) = location.iter().find(|(tag, _)| *tag == Tag::new(b"wght")) {
            os2.us_weight_class = wght.round().clamp(1.0, 1000.0) as u16;
        }
        builder.add_table(&os2).map_err(malformed)?;
    }
    layout(&font, &coords, &mut builder)?;
    for record in font.table_directory.table_records() {
        let tag = record.tag();
        // A `vmtx` without the `vhea` that sizes it is left out, not copied unchanged.
        if builder.contains(tag) || DROPPED.iter().any(|t| Tag::new(*t) == tag) || tag == Tag::new(b"vmtx") {
            continue;
        }
        if let Some(data) = font.table_data(tag) {
            builder.add_raw(tag, data.as_bytes().to_vec());
        }
    }
    Ok(builder.build())
}

/// Whether every glyph and table of `font` is one this instances. A slice
/// keeps only some glyphs, so a caller deciding for a whole font asks here.
pub fn check(font: &[u8]) -> Result<()> {
    let font = FontRef::new(font).map_err(malformed)?;
    if font.table_data(Tag::new(b"CFF2")).is_some() {
        return Err(Unsupported::Outlines("CFF2 outlines"));
    }
    if font.table_data(Tag::new(b"MVAR")).is_some() {
        return Err(Unsupported::Variation("an MVAR table"));
    }
    let (glyf, loca) = (font.glyf().map_err(malformed)?, font.loca(None).map_err(malformed)?);
    for gid in (0..font.maxp().map_err(malformed)?.num_glyphs()).map(GlyphId::from) {
        if let Some(ReadGlyph::Composite(_)) = loca.get_glyf(gid, &glyf).map_err(malformed)? {
            return Err(Unsupported::Outlines("composite glyphs"));
        }
    }
    Ok(())
}

/// The location in normalized coordinates, `avar` applied, one per `fvar` axis.
fn normalized(font: &FontRef, location: &[(Tag, f32)]) -> Result<Vec<F2Dot14>> {
    let fvar = font.fvar().map_err(|_| Unsupported::NotVariable)?;
    let axes = fvar.axes().map_err(malformed)?;
    let avar = font.avar().ok();
    let maps = avar.as_ref().map(|avar| avar.axis_segment_maps());
    axes.iter()
        .enumerate()
        .map(|(i, axis)| {
            let (min, default, max) = (axis.min_value().to_f64(), axis.default_value().to_f64(), axis.max_value().to_f64());
            let user = location.iter().find(|(tag, _)| *tag == axis.axis_tag()).map_or(default, |&(_, v)| f64::from(v)).clamp(min, max);
            let n = if user < default { (user - default) / (default - min) } else if user > default { (user - default) / (max - default) } else { 0.0 };
            let mut coord = F2Dot14::from_f32(n as f32).to_fixed();
            if let Some(map) = maps.as_ref().and_then(|maps| maps.get(i)) {
                coord = map.map_err(malformed)?.apply(coord);
            }
            Ok(coord.to_f2dot14())
        })
        .collect()
}

struct Outlines {
    glyf: GlyfLocaBuilder,
    h: Vec<LongMetric>,
    v: Option<Vec<LongMetric>>,
    /// Each glyph's bounds; `None` for an empty glyph.
    extents: Vec<Option<Bbox>>,
    bbox: Option<Bbox>,
}

fn outlines(font: &FontRef, coords: &[F2Dot14]) -> Result<Outlines> {
    let (glyf, loca) = (font.glyf().map_err(malformed)?, font.loca(None).map_err(malformed)?);
    let hmtx = font.hmtx().map_err(malformed)?;
    let vmtx = font.vmtx().ok();
    let gvar = font.gvar().ok();
    let count = font.maxp().map_err(malformed)?.num_glyphs();
    let mut out = Outlines { glyf: GlyfLocaBuilder::new(), h: Vec::new(), v: vmtx.as_ref().map(|_| Vec::new()), extents: Vec::new(), bbox: None };
    for gid in (0..count).map(GlyphId::from) {
        let (points, ends) = match loca.get_glyf(gid, &glyf).map_err(malformed)? {
            None => (Vec::new(), Vec::new()),
            Some(ReadGlyph::Simple(simple)) => (simple.points().collect::<Vec<CurvePoint>>(), simple.end_pts_of_contours().iter().map(|e| usize::from(e.get())).collect::<Vec<_>>()),
            Some(ReadGlyph::Composite(_)) => return Err(Unsupported::Outlines("composite glyphs")),
        };
        let x_min = points.iter().map(|p| p.x).min().unwrap_or(0);
        let y_max = points.iter().map(|p| p.y).max().unwrap_or(0);
        let left = f64::from(x_min) - f64::from(hmtx.side_bearing(gid).unwrap_or(0));
        let right = left + f64::from(hmtx.advance(gid).unwrap_or(0));
        let (top, bottom) = match &vmtx {
            Some(vmtx) => {
                let top = f64::from(y_max) + f64::from(vmtx.side_bearing(gid).unwrap_or(0));
                (top, top - f64::from(vmtx.advance(gid).unwrap_or(0)))
            }
            None => (0.0, 0.0),
        };
        // The outline's points, then the four phantom points that carry the metrics.
        let mut xy: Vec<(f64, f64)> = points.iter().map(|p| (f64::from(p.x), f64::from(p.y))).collect();
        xy.extend([(left, 0.0), (right, 0.0), (0.0, top), (0.0, bottom)]);
        if let Some(data) = gvar.as_ref().map(|gvar| gvar.glyph_variation_data(gid)).transpose().map_err(malformed)?.flatten() {
            apply_deltas(&mut xy, &ends, data.tuples().filter_map(|tuple| {
                let scalar = tuple.compute_scalar(coords)?.to_f64();
                let all = tuple.has_deltas_for_all_points();
                (scalar != 0.0).then(|| (scalar, all, tuple.deltas().map(|d| (usize::from(d.position), f64::from(d.x_delta), f64::from(d.y_delta))).collect::<Vec<_>>()))
            }));
        }
        let rounded: Vec<(i16, i16)> = xy.iter().map(|&(x, y)| Ok((to_i16(x)?, to_i16(y)?))).collect::<Result<_>>()?;
        let (outline, phantom) = rounded.split_at(points.len());
        let glyph = if outline.is_empty() {
            Glyph::Empty
        } else {
            let mut start = 0;
            let contours = ends.iter().map(|&end| {
                let contour: Contour = (start..=end).map(|i| CurvePoint { x: outline[i].0, y: outline[i].1, ..points[i] }).collect::<Vec<_>>().into();
                start = end + 1;
                contour
            }).collect();
            let mut simple = SimpleGlyph { bbox: Bbox::default(), contours, instructions: Vec::new(), overlaps: true };
            simple.recompute_bounding_box();
            Glyph::Simple(simple)
        };
        let extent = match &glyph { Glyph::Simple(s) => Some(s.bbox), _ => None };
        let (b_x_min, b_y_max) = extent.map_or((0, 0), |b| (b.x_min, b.y_max));
        out.h.push(LongMetric { advance: (phantom[1].0 - phantom[0].0).max(0) as u16, side_bearing: b_x_min - phantom[0].0 });
        if let Some(v) = out.v.as_mut() {
            v.push(LongMetric { advance: (phantom[2].1 - phantom[3].1).max(0) as u16, side_bearing: phantom[2].1 - b_y_max });
        }
        if let Some(b) = extent {
            out.bbox = Some(out.bbox.map_or(b, |a| Bbox { x_min: a.x_min.min(b.x_min), y_min: a.y_min.min(b.y_min), x_max: a.x_max.max(b.x_max), y_max: a.y_max.max(b.y_max) }));
        }
        out.extents.push(extent);
        out.glyf.add_glyph(&glyph).map_err(malformed)?;
    }
    Ok(out)
}

/// Rounds half up, as fontTools does.
fn to_i16(v: f64) -> Result<i16> {
    i16::try_from((v + 0.5).floor() as i64).map_err(|_| malformed("a coordinate outside 16 bits"))
}

/// Add each tuple's deltas, scaled, to `xy`. A tuple that lists only some
/// points has the others' deltas inferred along each contour (the `IUP` step
/// of the `gvar` spec); a phantom point it does not list does not move.
fn apply_deltas(xy: &mut [(f64, f64)], ends: &[usize], tuples: impl Iterator<Item = (f64, bool, Vec<(usize, f64, f64)>)>) {
    let original = xy.to_vec();
    for (scalar, all, deltas) in tuples {
        let mut d: Vec<Option<(f64, f64)>> = vec![None; xy.len()];
        for (at, dx, dy) in deltas {
            if let Some(slot) = d.get_mut(at) {
                *slot = Some((dx, dy));
            }
        }
        if !all {
            let mut start = 0;
            for &end in ends {
                infer(&mut d[start..=end], &original[start..=end]);
                start = end + 1;
            }
        }
        for (p, delta) in xy.iter_mut().zip(d) {
            if let Some((dx, dy)) = delta {
                p.0 += dx * scalar;
                p.1 += dy * scalar;
            }
        }
    }
}

/// Fill in the deltas a contour leaves out from the two explicit neighbours
/// on either side, per axis: between them, interpolated by position; beyond
/// both, the nearer one's delta.
fn infer(d: &mut [Option<(f64, f64)>], original: &[(f64, f64)]) {
    let known: Vec<usize> = (0..d.len()).filter(|&i| d[i].is_some()).collect();
    let Some(&first) = known.first() else {
        d.iter_mut().for_each(|v| *v = Some((0.0, 0.0)));
        return;
    };
    let n = d.len();
    for (k, &from) in known.iter().enumerate() {
        let to = known.get(k + 1).copied().unwrap_or(first);
        let (df, dt) = (d[from].unwrap_or_default(), d[to].unwrap_or_default());
        let mut i = (from + 1) % n;
        while i != to {
            let axis = |o: fn(&(f64, f64)) -> f64, (a, b): (f64, f64)| {
                let (mut x1, mut x2, mut d1, mut d2) = (o(&original[from]), o(&original[to]), a, b);
                let x = o(&original[i]);
                if x1 == x2 {
                    return if d1 == d2 { d1 } else { 0.0 };
                }
                if x1 > x2 {
                    (x1, x2, d1, d2) = (x2, x1, d2, d1);
                }
                if x <= x1 { d1 } else if x >= x2 { d2 } else { d1 + (x - x1) * (d2 - d1) / (x2 - x1) }
            };
            d[i] = Some((axis(|p| p.0, (df.0, dt.0)), axis(|p| p.1, (df.1, dt.1))));
            i = (i + 1) % n;
        }
    }
}

/// What `hhea` or `vhea` records about the metrics along one direction.
struct SideMetrics {
    count: u16,
    advance_max: u16,
    min_before: i16,
    min_after: i16,
    max_extent: i16,
}

/// `extents` are each glyph's (start, end) along the direction, `None` when empty.
fn side_metrics(metrics: &[LongMetric], extents: impl Iterator<Item = Option<(i16, i16)>>) -> SideMetrics {
    let mut m = SideMetrics { count: metrics.len() as u16, advance_max: 0, min_before: i16::MAX, min_after: i16::MAX, max_extent: i16::MIN };
    for (metric, extent) in metrics.iter().zip(extents) {
        m.advance_max = m.advance_max.max(metric.advance);
        let Some((start, end)) = extent else { continue };
        let size = i32::from(end) - i32::from(start);
        let before = i32::from(metric.side_bearing);
        m.min_before = m.min_before.min(metric.side_bearing);
        m.min_after = m.min_after.min((i32::from(metric.advance) - before - size).clamp(-32768, 32767) as i16);
        m.max_extent = m.max_extent.max((before + size).clamp(-32768, 32767) as i16);
    }
    if m.max_extent == i16::MIN {
        (m.min_before, m.min_after, m.max_extent) = (0, 0, 0);
    }
    m
}

/// `GPOS`, `GDEF` and `BASE` with each variation delta added to the value it
/// varies, its variation index dropped, and the stores emptied.
fn layout(font: &FontRef, coords: &[F2Dot14], builder: &mut FontBuilder) -> Result<()> {
    let gsub = font.gsub().ok().and_then(|t| t.feature_variations_offset()).is_some_and(|o| !o.is_null());
    let gpos = font.gpos().ok().and_then(|t| t.feature_variations_offset()).is_some_and(|o| !o.is_null());
    if gsub || gpos {
        return Err(Unsupported::Variation("feature variations"));
    }
    let gdef = font.gdef().ok();
    let store = gdef.as_ref().and_then(|gdef| gdef.item_var_store()).transpose().map_err(malformed)?;
    if let Some(gdef) = &gdef {
        let mut owned: write_fonts::tables::gdef::Gdef = gdef.to_owned_table();
        if store.is_some() {
            if owned.lig_caret_list.as_ref().is_some() {
                return Err(Unsupported::Variation("ligature carets in a variable GDEF"));
            }
            owned.item_var_store = NullableOffsetMarker::new(None);
        }
        builder.add_table(&owned).map_err(malformed)?;
    }
    if let Ok(gpos) = font.gpos() {
        let mut owned: Gpos = gpos.to_owned_table();
        if owned.lookup_list2.as_ref().is_some() {
            return Err(Unsupported::Variation("a GPOS with 24-bit glyph ids"));
        }
        let cx = Deltas { store: store.as_ref(), coords };
        if let Some(list) = owned.lookup_list.as_mut() {
            for lookup in list.lookups.iter_mut() {
                match &mut **lookup {
                    PositionLookup::Single(l) => each(l, &cx)?,
                    PositionLookup::Pair(l) => each(l, &cx)?,
                    PositionLookup::Cursive(l) => each(l, &cx)?,
                    PositionLookup::MarkToBase(l) => each(l, &cx)?,
                    PositionLookup::MarkToLig(l) => each(l, &cx)?,
                    PositionLookup::MarkToMark(l) => each(l, &cx)?,
                    PositionLookup::Contextual(_) | PositionLookup::ChainContextual(_) => {}
                    PositionLookup::Extension(l) => each(l, &cx)?,
                }
            }
        }
        builder.add_table(&owned).map_err(malformed)?;
    }
    if let Ok(base) = font.base() {
        let store = base.item_var_store().transpose().map_err(malformed)?;
        let mut owned: Base = base.to_owned_table();
        if store.is_some() {
            let cx = Deltas { store: store.as_ref(), coords };
            for axis in [owned.horiz_axis.as_mut(), owned.vert_axis.as_mut()].into_iter().flatten() {
                for record in axis.base_script_list.base_script_records.iter_mut() {
                    let script = &mut *record.base_script;
                    if let Some(values) = script.base_values.as_mut() {
                        for coord in values.base_coords.iter_mut() {
                            base_coord(coord, &cx)?;
                        }
                    }
                    let lang_sys = script.base_lang_sys_records.iter_mut().map(|r| &mut *r.min_max);
                    for min_max in script.default_min_max.as_mut().into_iter().chain(lang_sys) {
                        min_max_coords(min_max, &cx)?;
                    }
                }
            }
            owned.item_var_store = NullableOffsetMarker::new(None);
        }
        builder.add_table(&owned).map_err(malformed)?;
    }
    Ok(())
}

struct Deltas<'a, 'b> {
    store: Option<&'b ItemVariationStore<'a>>,
    coords: &'b [F2Dot14],
}

impl Deltas<'_, '_> {
    /// The delta a variation index names, at the location, and the index
    /// dropped. A device table (hinting by pixel size) stays as it is.
    fn take(&self, device: &mut NullableOffsetMarker<DeviceOrVariationIndex>) -> Result<i32> {
        let Some(DeviceOrVariationIndex::VariationIndex(index)) = device.as_ref() else {
            return match device.as_ref() {
                Some(DeviceOrVariationIndex::PendingVariationIndex(_)) => Err(malformed("an unresolved variation index")),
                _ => Ok(0),
            };
        };
        let (outer, inner) = (index.delta_set_outer_index, index.delta_set_inner_index);
        let delta = if (outer, inner) == (0xFFFF, 0xFFFF) {
            0
        } else {
            let store = self.store.ok_or_else(|| malformed("a variation index with no variation store"))?;
            store.compute_delta(DeltaSetIndex { outer, inner }, self.coords).ok_or_else(|| malformed("a variation index outside the store"))?.to_i32()
        };
        *device = NullableOffsetMarker::new(None);
        Ok(delta)
    }

    /// Adds the delta to `value`, which a record that varied only through its
    /// device has to gain: the returned flag then joins the record's format.
    fn shift(&self, value: &mut Option<i16>, device: &mut NullableOffsetMarker<DeviceOrVariationIndex>, flag: ValueFormat) -> Result<ValueFormat> {
        let delta = self.take(device)?;
        if delta == 0 {
            return Ok(ValueFormat::empty());
        }
        let gained = if value.is_none() { flag } else { ValueFormat::empty() };
        *value = Some(add(value.unwrap_or(0), delta)?);
        Ok(gained)
    }

    /// The value fields the record gained.
    fn record(&self, r: &mut ValueRecord) -> Result<ValueFormat> {
        Ok(self.shift(&mut r.x_placement, &mut r.x_placement_device, ValueFormat::X_PLACEMENT)?
            | self.shift(&mut r.y_placement, &mut r.y_placement_device, ValueFormat::Y_PLACEMENT)?
            | self.shift(&mut r.x_advance, &mut r.x_advance_device, ValueFormat::X_ADVANCE)?
            | self.shift(&mut r.y_advance, &mut r.y_advance_device, ValueFormat::Y_ADVANCE)?)
    }

    /// Instances records that share one value format, then widens it to every
    /// field any of them gained, since a subtable writes one format for all.
    fn records<'r>(&self, records: impl Iterator<Item = &'r mut ValueRecord>) -> Result<()> {
        let mut records: Vec<&mut ValueRecord> = records.collect();
        let mut gained = ValueFormat::empty();
        for r in records.iter_mut() {
            gained |= self.record(r)?;
        }
        if !gained.is_empty() {
            for r in records {
                let format = r.format() | gained;
                r.set_explicit_value_format(format);
            }
        }
        Ok(())
    }

    fn anchor(&self, anchor: Option<&mut AnchorTable>) -> Result<()> {
        if let Some(AnchorTable::Format3(a)) = anchor {
            a.x_coordinate = add(a.x_coordinate, self.take(&mut a.x_device)?)?;
            a.y_coordinate = add(a.y_coordinate, self.take(&mut a.y_device)?)?;
        }
        Ok(())
    }

    fn marks(&self, marks: &mut MarkArray) -> Result<()> {
        marks.mark_records.iter_mut().try_for_each(|m| self.anchor(Some(&mut m.mark_anchor)))
    }
}

fn add(value: i16, delta: i32) -> Result<i16> {
    i16::try_from(i32::from(value) + delta).map_err(|_| malformed("a layout value outside 16 bits"))
}

fn base_coord(coord: &mut BaseCoord, cx: &Deltas) -> Result<()> {
    if let BaseCoord::Format3(c) = coord {
        c.coordinate = add(c.coordinate, cx.take(&mut c.device)?)?;
    }
    Ok(())
}

fn min_max_coords(m: &mut MinMax, cx: &Deltas) -> Result<()> {
    let features = m.feat_min_max_records.iter_mut().flat_map(|r| [r.min_coord.as_mut(), r.max_coord.as_mut()]);
    [m.min_coord.as_mut(), m.max_coord.as_mut()].into_iter().chain(features).flatten().try_for_each(|c| base_coord(c, cx))
}

/// A positioning subtable whose values may vary.
trait Varies {
    fn instance(&mut self, cx: &Deltas) -> Result<()>;
}

fn each<T: Varies>(lookup: &mut Lookup<T>, cx: &Deltas) -> Result<()> {
    lookup.subtables.iter_mut().try_for_each(|s| s.instance(cx))
}

const WIDE: Unsupported = Unsupported::Variation("a positioning subtable with 24-bit glyph ids");

impl Varies for SinglePos {
    fn instance(&mut self, cx: &Deltas) -> Result<()> {
        match self {
            SinglePos::Format1(s) => cx.records(std::iter::once(&mut s.value_record)),
            SinglePos::Format2(s) => cx.records(s.value_records.iter_mut()),
            _ => Err(WIDE),
        }
    }
}

impl Varies for PairPos {
    fn instance(&mut self, cx: &Deltas) -> Result<()> {
        match self {
            PairPos::Format1(p) => {
                let pairs: Vec<_> = p.pair_sets.iter_mut().flat_map(|set| set.pair_value_records.iter_mut()).collect();
                let (first, second): (Vec<_>, Vec<_>) = pairs.into_iter().map(|r| (&mut r.value_record1, &mut r.value_record2)).unzip();
                cx.records(first.into_iter())?;
                cx.records(second.into_iter())
            }
            PairPos::Format2(p) => {
                dense_classes(p)?;
                let pairs: Vec<_> = p.class1_records.iter_mut().flat_map(|c| c.class2_records.iter_mut()).collect();
                let (first, second): (Vec<_>, Vec<_>) = pairs.into_iter().map(|r| (&mut r.value_record1, &mut r.value_record2)).unzip();
                cx.records(first.into_iter())?;
                cx.records(second.into_iter())
            }
            _ => Err(WIDE),
        }
    }
}

/// The subsetter keeps a class table's class count when a slice empties some
/// of its classes, and write-fonts refuses a count above the classes still
/// assigned. Numbers both sets densely and keeps only the records they reach;
/// a table left with no records adjusts nothing, so it becomes all zeros.
fn dense_classes(p: &mut PairPosFormat2) -> Result<()> {
    let first = renumber(&mut p.class_def1);
    let second = renumber(&mut p.class_def2);
    if p.class1_records.is_empty() {
        let zeros = || Class1Record::new(second.iter().map(|_| Class2Record::new(ValueRecord::default(), ValueRecord::default())).collect());
        p.class1_records = first.iter().map(|_| zeros()).collect();
        return Ok(());
    }
    if first.len() == p.class1_records.len() && p.class1_records.iter().all(|r| r.class2_records.len() == second.len()) {
        return Ok(());
    }
    let mut rows: Vec<_> = std::mem::take(&mut p.class1_records).into_iter().map(Some).collect();
    for old in first {
        let mut row = rows.get_mut(old).and_then(Option::take).ok_or_else(|| malformed("a class with no pair record"))?;
        let mut cells: Vec<_> = std::mem::take(&mut row.class2_records).into_iter().map(Some).collect();
        row.class2_records = second.iter().map(|&c| cells.get_mut(c).and_then(Option::take)).collect::<Option<_>>().ok_or_else(|| malformed("a class with no pair record"))?;
        p.class1_records.push(row);
    }
    Ok(())
}

/// Renumbers `def` to classes 0.. in their old order, class 0 kept, and
/// returns the old class of each new one.
fn renumber(def: &mut ClassDef) -> Vec<usize> {
    let mut used: Vec<u32> = def.iter().map(|(_, c)| c).chain([0]).collect();
    used.sort_unstable();
    used.dedup();
    if used.last().is_some_and(|&last| last as usize + 1 != used.len()) {
        let dense: ClassDef = def.iter().map(|(g, c)| (g, used.binary_search(&c).unwrap_or(0) as u32)).collect();
        *def = dense;
    }
    used.into_iter().map(|c| c as usize).collect()
}

impl Varies for CursivePos {
    fn instance(&mut self, cx: &Deltas) -> Result<()> {
        let CursivePos::Format1(c) = self else { return Err(WIDE) };
        c.entry_exit_record.iter_mut().try_for_each(|r| {
            cx.anchor(r.entry_anchor.as_mut())?;
            cx.anchor(r.exit_anchor.as_mut())
        })
    }
}

impl Varies for MarkBasePos {
    fn instance(&mut self, cx: &Deltas) -> Result<()> {
        let MarkBasePos::Format1(m) = self else { return Err(WIDE) };
        cx.marks(&mut m.mark_array)?;
        m.base_array.base_records.iter_mut().flat_map(|r| r.base_anchors.iter_mut()).try_for_each(|a| cx.anchor(a.as_mut()))
    }
}

impl Varies for MarkLigPos {
    fn instance(&mut self, cx: &Deltas) -> Result<()> {
        let MarkLigPos::Format1(m) = self else { return Err(WIDE) };
        cx.marks(&mut m.mark_array)?;
        let components = m.ligature_array.ligature_attaches.iter_mut().flat_map(|l| l.component_records.iter_mut());
        components.flat_map(|c| c.ligature_anchors.iter_mut()).try_for_each(|a| cx.anchor(a.as_mut()))
    }
}

impl Varies for MarkMarkPos {
    fn instance(&mut self, cx: &Deltas) -> Result<()> {
        let MarkMarkPos::Format1(m) = self else { return Err(WIDE) };
        cx.marks(&mut m.mark1_array)?;
        m.mark2_array.mark2_records.iter_mut().flat_map(|r| r.mark2_anchors.iter_mut()).try_for_each(|a| cx.anchor(a.as_mut()))
    }
}

impl Varies for ExtensionSubtable {
    fn instance(&mut self, cx: &Deltas) -> Result<()> {
        match self {
            ExtensionSubtable::Single(e) => e.extension.instance(cx),
            ExtensionSubtable::Pair(e) => e.extension.instance(cx),
            ExtensionSubtable::Cursive(e) => e.extension.instance(cx),
            ExtensionSubtable::MarkToBase(e) => e.extension.instance(cx),
            ExtensionSubtable::MarkToLig(e) => e.extension.instance(cx),
            ExtensionSubtable::MarkToMark(e) => e.extension.instance(cx),
            ExtensionSubtable::Contextual(_) | ExtensionSubtable::ChainContextual(_) => Ok(()),
        }
    }
}

#[cfg(test)]
#[path = "instance_tests.rs"]
mod tests;
