//! Decoder for the schema-4 label payload appended to the `MOSSPLM1` pack
//! after the tile index: named cities, mountain ranges, peaks and rivers in
//! `en` and `zh-Hant`, for the places-explorer's map labels.
//!
//! This is data only. Nothing in this crate yet reads [`Labels`] to place a
//! label on a rendered map -- that is a separate, later landing. What reads
//! it today is `emit::place_map_labels`, which turns it into
//! `labels.<hash>.json` for the site's own published language(s).
//!
//! The wire format is produced by `scripts/place-map/generate.mjs`'s
//! `encodeLabels` (see that function's own doc for the byte layout) and is
//! intentionally opaque to the rest of [`super`]: everything here reads
//! through [`super::Reader`] and [`super::zigzag`], the same primitives the
//! tier/feature decoder above uses, so a label record's bytes decode with
//! the identical bounds-checked-before-allocation discipline.

use super::{DecodeError, Reader};

/// The label payload's own internal format version (distinct from the
/// pack's `SCHEMA`): bumped only if this byte layout itself changes, not
/// when the label SET changes (more/fewer cities, say) -- an ordinary
/// re-generation keeps emitting schema 1 with different record counts.
const SCHEMA: u8 = 1;
/// Generous upper bounds on record counts, purely defense in depth: the
/// generator's own u16 count fields already cap each group at 65,535, and
/// the real pack carries low thousands at most (see
/// `scripts/place-map/README.md`).
const MAX_RECORDS_PER_GROUP: usize = 20_000;
const MAX_POINTS_PER_RIVER: usize = 10_000;
/// Natural Earth's longest Traditional-Chinese display name is nowhere near
/// this; it exists so a malformed length byte can't be read as "the rest of
/// the buffer is one name".
const MAX_NAME_BYTES: usize = 255;

/// One named point label: a city, a mountain range (labeled at its largest
/// ring's centroid -- see the generator's `ringCentroid`) or a peak. Which
/// of the three it is comes from which [`Labels`] field holds it; the wire
/// format carries no separate per-record kind byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointLabel {
    /// Thousandths of a degree -- the same quantisation
    /// [`super::Header::quantisation`] names for the rest of the pack.
    pub lng: i32,
    pub lat: i32,
    /// Natural Earth's own `scalerank`/`SCALERANK` (0 = most important).
    pub rank: i16,
    pub name_en: String,
    pub name_zht: String,
}

/// One named river label: a short, independently-simplified polyline (not
/// a reference into the pack's own river geometry -- see this crate's
/// `scripts/place-map/README.md` for why) for a renderer to orient and
/// curve the name along.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiverLabel {
    pub rank: i16,
    pub name_en: String,
    pub name_zht: String,
    /// Thousandths of a degree, in the line's natural order (not closed --
    /// a river is an open course, never a ring).
    pub line: Vec<(i32, i32)>,
}

/// Every label this build's pack carries. See the module doc for how a site
/// turns this into its own `labels.<hash>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Labels {
    pub cities: Vec<PointLabel>,
    pub ranges: Vec<PointLabel>,
    pub peaks: Vec<PointLabel>,
    pub rivers: Vec<RiverLabel>,
}

fn read_name(reader: &mut Reader<'_>, label: &'static str) -> Result<String, DecodeError> {
    let len = reader.u8(label)? as usize;
    if len > MAX_NAME_BYTES {
        return Err(DecodeError::InvalidLength(label));
    }
    let bytes = reader.take(len, label)?;
    String::from_utf8(bytes.to_vec()).map_err(|_| DecodeError::InvalidString(label))
}

fn read_point_group(reader: &mut Reader<'_>, label: &'static str) -> Result<Vec<PointLabel>, DecodeError> {
    let count = reader.u16(label)? as usize;
    if count > MAX_RECORDS_PER_GROUP {
        return Err(DecodeError::InvalidLength(label));
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let lng = reader.i32("label lng")?;
        let lat = reader.i32("label lat")?;
        let rank = reader.i16("label rank")?;
        let name_en = read_name(reader, "label name_en")?;
        let name_zht = read_name(reader, "label name_zht")?;
        out.push(PointLabel { lng, lat, rank, name_en, name_zht });
    }
    Ok(out)
}

fn read_river_group(reader: &mut Reader<'_>) -> Result<Vec<RiverLabel>, DecodeError> {
    let count = reader.u16("rivers")? as usize;
    if count > MAX_RECORDS_PER_GROUP {
        return Err(DecodeError::InvalidLength("rivers"));
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let rank = reader.i16("river rank")?;
        let name_en = read_name(reader, "river name_en")?;
        let name_zht = read_name(reader, "river name_zht")?;
        let point_count = reader.u16("river points")? as usize;
        if point_count < 2 || point_count > MAX_POINTS_PER_RIVER {
            return Err(DecodeError::InvalidLength("river points"));
        }
        let mut line = Vec::with_capacity(point_count);
        let mut x = 0i32;
        let mut y = 0i32;
        for _ in 0..point_count {
            x = x
                .checked_add(super::zigzag(reader.varint()?))
                .ok_or(DecodeError::InvalidCoordinate)?;
            y = y
                .checked_add(super::zigzag(reader.varint()?))
                .ok_or(DecodeError::InvalidCoordinate)?;
            line.push((x, y));
        }
        out.push(RiverLabel { rank, name_en, name_zht, line });
    }
    Ok(out)
}

/// Decode a schema-4 pack's label payload, called from [`super::decode`]
/// with exactly the slice its own length-prefix names -- so a trailing-byte
/// mismatch here always means the payload itself is malformed, never that
/// the caller handed over too much or too little of the pack.
pub(super) fn decode(bytes: &[u8]) -> Result<Labels, DecodeError> {
    let mut reader = Reader::new(bytes);
    let schema = reader.u8("labels schema")?;
    if schema != SCHEMA {
        return Err(DecodeError::UnknownSchema(schema.into()));
    }
    let cities = read_point_group(&mut reader, "cities")?;
    let ranges = read_point_group(&mut reader, "ranges")?;
    let peaks = read_point_group(&mut reader, "peaks")?;
    let rivers = read_river_group(&mut reader)?;
    if reader.position != bytes.len() {
        return Err(DecodeError::InvalidLength("labels trailing bytes"));
    }
    Ok(Labels { cities, ranges, peaks, rivers })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u8(v: u8) -> Vec<u8> { vec![v] }
    fn u16(v: u16) -> Vec<u8> { v.to_le_bytes().to_vec() }
    fn i16(v: i16) -> Vec<u8> { v.to_le_bytes().to_vec() }
    fn i32(v: i32) -> Vec<u8> { v.to_le_bytes().to_vec() }
    fn name(s: &str) -> Vec<u8> {
        let bytes = s.as_bytes();
        let mut out = u8(bytes.len() as u8);
        out.extend_from_slice(bytes);
        out
    }
    fn varint(mut value: u32) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
        out
    }
    fn zigzag(v: i32) -> u32 {
        ((v << 1) ^ (v >> 31)) as u32
    }

    fn point_record(lng: i32, lat: i32, rank: i16, name_en: &str, name_zht: &str) -> Vec<u8> {
        let mut out = i32(lng);
        out.extend(i32(lat));
        out.extend(i16(rank));
        out.extend(name(name_en));
        out.extend(name(name_zht));
        out
    }

    fn empty_payload() -> Vec<u8> {
        let mut bytes = u8(SCHEMA);
        for _ in 0..3 {
            bytes.extend(u16(0)); // cities, ranges, peaks
        }
        bytes.extend(u16(0)); // rivers
        bytes
    }

    #[test]
    fn an_empty_payload_decodes_to_four_empty_groups() {
        let labels = decode(&empty_payload()).unwrap();
        assert_eq!(labels, Labels::default());
    }

    #[test]
    fn a_city_round_trips_through_encode_and_decode() {
        let mut bytes = u8(SCHEMA);
        bytes.extend(u16(1));
        bytes.extend(point_record(139_767, 35_681, 0, "Tokyo", "東京"));
        bytes.extend(u16(0)); // ranges
        bytes.extend(u16(0)); // peaks
        bytes.extend(u16(0)); // rivers
        let labels = decode(&bytes).unwrap();
        assert_eq!(
            labels.cities,
            vec![PointLabel { lng: 139_767, lat: 35_681, rank: 0, name_en: "Tokyo".into(), name_zht: "東京".into() }]
        );
        assert!(labels.ranges.is_empty() && labels.peaks.is_empty() && labels.rivers.is_empty());
    }

    #[test]
    fn a_river_polyline_decodes_through_the_shared_delta_zigzag_varint_reader() {
        let mut bytes = u8(SCHEMA);
        bytes.extend(u16(0)); // cities
        bytes.extend(u16(0)); // ranges
        bytes.extend(u16(0)); // peaks
        bytes.extend(u16(1)); // rivers
        bytes.extend(i16(1));
        bytes.extend(name("Nile"));
        bytes.extend(name("尼羅河"));
        bytes.extend(u16(3));
        // (0,0) -> (10,20) -> (5,25), each point delta/zig-zag encoded from
        // the previous one (the first point's delta is from the origin).
        bytes.extend(varint(zigzag(0)));
        bytes.extend(varint(zigzag(0)));
        bytes.extend(varint(zigzag(10)));
        bytes.extend(varint(zigzag(20)));
        bytes.extend(varint(zigzag(-5)));
        bytes.extend(varint(zigzag(5)));

        let labels = decode(&bytes).unwrap();
        assert_eq!(labels.rivers.len(), 1);
        let river = &labels.rivers[0];
        assert_eq!(river.name_en, "Nile");
        assert_eq!(river.name_zht, "尼羅河");
        assert_eq!(river.rank, 1);
        assert_eq!(river.line, vec![(0, 0), (10, 20), (5, 25)]);
    }

    #[test]
    fn a_river_with_fewer_than_two_points_is_rejected() {
        let mut bytes = u8(SCHEMA);
        bytes.extend(u16(0));
        bytes.extend(u16(0));
        bytes.extend(u16(0));
        bytes.extend(u16(1));
        bytes.extend(i16(0));
        bytes.extend(name("Too Short"));
        bytes.extend(name(""));
        bytes.extend(u16(1)); // a single point is not a line
        bytes.extend(varint(zigzag(0)));
        bytes.extend(varint(zigzag(0)));
        assert_eq!(decode(&bytes).unwrap_err(), DecodeError::InvalidLength("river points"));
    }

    #[test]
    fn an_unknown_label_schema_is_rejected() {
        let mut bytes = empty_payload();
        bytes[0] = SCHEMA + 1;
        assert_eq!(decode(&bytes).unwrap_err(), DecodeError::UnknownSchema((SCHEMA + 1).into()));
    }

    #[test]
    fn trailing_bytes_after_the_last_group_are_rejected() {
        let mut bytes = empty_payload();
        bytes.push(0xff);
        assert_eq!(decode(&bytes).unwrap_err(), DecodeError::InvalidLength("labels trailing bytes"));
    }

    #[test]
    fn invalid_utf8_in_a_name_is_rejected_rather_than_lossily_decoded() {
        let mut bytes = u8(SCHEMA);
        bytes.extend(u16(1));
        bytes.extend(i32(0));
        bytes.extend(i32(0));
        bytes.extend(i16(0));
        // A single 0x80 byte is not valid UTF-8 on its own.
        bytes.extend(u8(1));
        bytes.push(0x80);
        bytes.extend(name(""));
        bytes.extend(u16(0));
        bytes.extend(u16(0));
        bytes.extend(u16(0));
        assert_eq!(decode(&bytes).unwrap_err(), DecodeError::InvalidString("label name_en"));
    }

    #[test]
    fn a_truncated_payload_is_rejected_before_any_allocation_sized_by_its_own_claim() {
        // A count that claims one city but supplies no record bytes.
        let mut bytes = u8(SCHEMA);
        bytes.extend(u16(1));
        assert!(matches!(decode(&bytes), Err(DecodeError::Truncated(_))));
    }
}
