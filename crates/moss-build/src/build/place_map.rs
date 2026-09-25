//! Immutable decoder for the checked-in `MOSSPLM1` place-map pack.

mod context;
mod geometry;

pub use context::{PlaceMapContext, PlaceMapTarget, ResolvedPlace};
pub use geometry::{marker_radius, privacy_floor, Frame, FrameTier, ProjectedPoint, Projection, TileSelection};

const MAGIC: &[u8; 8] = b"MOSSPLM1";
const HEADER_LEN: usize = 92;
const SCHEMA: u16 = 1;
const EXPECTED_LAYERS: u16 = 10;
const EXPECTED_TIERS: u16 = 2;
const MAX_PARTS_PER_FEATURE: usize = 4096;
const MAX_POINTS_PER_PART: usize = 1_000_000;
const MAX_POINTS_PER_FEATURE: usize = 2_000_000;
const MAX_FEATURES_PER_LAYER: u32 = 100_000;
const MAX_FEATURES_PER_TIER: u32 = 200_000;

pub const PACK: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/data/place-map/place-map-v1.bin"
));

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    Truncated(&'static str),
    InvalidMagic,
    UnknownSchema(u16),
    InvalidHeader(&'static str),
    UnknownLayer(u8),
    DuplicateLayer(u8),
    MissingLayer(u8),
    InvalidTier(u8),
    InvalidLength(&'static str),
    InvalidCoordinate,
    InvalidVarint,
    InvalidIndex(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub schema: u16,
    pub quantisation: u32,
    pub extent: [i32; 4],
    pub layer_count: u16,
    pub tier_count: u16,
    pub tile_degrees: u16,
    pub manifest_sha256: [u8; 32],
    pub source_masks: [u16; 10],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feature {
    pub bounds: [i32; 4],
    pub band: i16,
    pub parts: Vec<Vec<(i32, i32)>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub id: u8,
    pub feature_count: u32,
    pub features: Vec<Feature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tier {
    pub id: u8,
    pub feature_count: u32,
    pub layers: Vec<Layer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub x: i16,
    pub y: i16,
    pub features: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pack {
    pub header: Header,
    pub tiers: Vec<Tier>,
    pub tiles: Vec<Tile>,
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, length: usize, label: &'static str) -> Result<&'a [u8], DecodeError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(DecodeError::InvalidLength(label))?;
        if end > self.bytes.len() {
            return Err(DecodeError::Truncated(label));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self, label: &'static str) -> Result<u8, DecodeError> {
        Ok(self.take(1, label)?[0])
    }
    fn u16(&mut self, label: &'static str) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take(2, label)?.try_into().unwrap()))
    }
    fn i16(&mut self, label: &'static str) -> Result<i16, DecodeError> {
        Ok(i16::from_le_bytes(self.take(2, label)?.try_into().unwrap()))
    }
    fn u32(&mut self, label: &'static str) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take(4, label)?.try_into().unwrap()))
    }
    fn i32(&mut self, label: &'static str) -> Result<i32, DecodeError> {
        Ok(i32::from_le_bytes(self.take(4, label)?.try_into().unwrap()))
    }

    fn bounded_slice(
        &mut self,
        length: u32,
        label: &'static str,
    ) -> Result<Reader<'a>, DecodeError> {
        let bytes = self.take(
            usize::try_from(length).map_err(|_| DecodeError::InvalidLength(label))?,
            label,
        )?;
        Ok(Reader::new(bytes))
    }

    fn varint(&mut self) -> Result<u32, DecodeError> {
        let mut value = 0u32;
        for index in 0..5 {
            let shift = index * 7;
            let byte = self.u8("varint")?;
            let payload = byte & 0x7f;
            if index == 4 && payload > 0x0f {
                return Err(DecodeError::InvalidVarint);
            }
            value |= u32::from(payload) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(DecodeError::InvalidVarint)
    }
}

fn zigzag(value: u32) -> i32 {
    ((value >> 1) as i32) ^ -((value & 1) as i32)
}

fn valid_layer(id: u8) -> bool {
    (1..=EXPECTED_LAYERS as u8).contains(&id)
}

fn decode_feature(
    reader: &mut Reader<'_>,
    extent: [i32; 4],
    bounds: [i32; 4],
    band: i16,
) -> Result<Feature, DecodeError> {
    if bounds[0] < extent[0]
        || bounds[1] < extent[2]
        || bounds[2] > extent[1]
        || bounds[3] > extent[3]
        || bounds[0] > bounds[2]
        || bounds[1] > bounds[3]
    {
        return Err(DecodeError::InvalidCoordinate);
    }
    let parts = reader.u16("part count")? as usize;
    if parts == 0 || parts > MAX_PARTS_PER_FEATURE || parts > reader.remaining() / 8 {
        return Err(DecodeError::InvalidLength("part count"));
    }
    let mut decoded = Vec::with_capacity(parts as usize);
    let mut previous = (0i32, 0i32);
    let mut total_points = 0usize;
    for _ in 0..parts {
        let count = reader.u32("point count")?;
        let count = usize::try_from(count).map_err(|_| DecodeError::InvalidLength("point count"))?;
        if count < 2 || count > MAX_POINTS_PER_PART || count > reader.remaining() / 2 {
            return Err(DecodeError::InvalidLength("point count"));
        }
        total_points = total_points
            .checked_add(count)
            .ok_or(DecodeError::InvalidLength("point count"))?;
        if total_points > MAX_POINTS_PER_FEATURE {
            return Err(DecodeError::InvalidLength("point count"));
        }
        let mut points = Vec::with_capacity(count);
        for _ in 0..count {
            let x = previous
                .0
                .checked_add(zigzag(reader.varint()?))
                .ok_or(DecodeError::InvalidCoordinate)?;
            let y = previous
                .1
                .checked_add(zigzag(reader.varint()?))
                .ok_or(DecodeError::InvalidCoordinate)?;
            if x < extent[0]
                || x > extent[1]
                || y < extent[2]
                || y > extent[3]
                || x < bounds[0]
                || x > bounds[2]
                || y < bounds[1]
                || y > bounds[3]
            {
                return Err(DecodeError::InvalidCoordinate);
            }
            points.push((x, y));
            previous = (x, y);
        }
        decoded.push(points);
    }
    Ok(Feature {
        bounds,
        band,
        parts: decoded,
    })
}

fn decode_tier(
    reader: &mut Reader<'_>,
    extent: [i32; 4],
    expected_id: u8,
) -> Result<Tier, DecodeError> {
    let id = reader.u8("tier id")?;
    let _reserved = reader.u8("tier reserved")?;
    if id != expected_id {
        return Err(DecodeError::InvalidTier(id));
    }
    let feature_count = reader.u32("tier feature count")?;
    if feature_count > MAX_FEATURES_PER_TIER {
        return Err(DecodeError::InvalidLength("tier feature count"));
    }
    let payload_len = reader.u32("tier length")?;
    let mut payload = reader.bounded_slice(payload_len, "tier payload")?;
    let mut layers = Vec::new();
    let mut seen = 0u16;
    let mut total = 0u32;
    while payload.position < payload.bytes.len() {
        let id = payload.u8("layer id")?;
        let _kind = payload.u8("layer kind")?;
        if !valid_layer(id) {
            return Err(DecodeError::UnknownLayer(id));
        }
        let bit = 1u16 << (id - 1);
        if seen & bit != 0 {
            return Err(DecodeError::DuplicateLayer(id));
        }
        seen |= bit;
        let count = payload.u32("layer feature count")?;
        if count > MAX_FEATURES_PER_LAYER || usize::try_from(count).unwrap_or(usize::MAX) > payload.remaining() / 22 {
            return Err(DecodeError::InvalidLength("layer feature count"));
        }
        let length = payload.u32("layer length")?;
        let mut body = payload.bounded_slice(length, "layer payload")?;
        let mut features = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let byte_length = body.u32("feature length")?;
            let bounds = [
                body.i32("feature bounds")?,
                body.i32("feature bounds")?,
                body.i32("feature bounds")?,
                body.i32("feature bounds")?,
            ];
            let band = body.i16("band threshold")?;
            let mut feature_body = body.bounded_slice(byte_length, "feature payload")?;
            let feature = decode_feature(&mut feature_body, extent, bounds, band)?;
            if feature_body.position != feature_body.bytes.len() {
                return Err(DecodeError::InvalidLength("feature trailing bytes"));
            }
            features.push(feature);
        }
        if body.position != body.bytes.len() {
            return Err(DecodeError::InvalidLength("layer trailing bytes"));
        }
        total = total
            .checked_add(count)
            .ok_or(DecodeError::InvalidLength("feature count"))?;
        if total > MAX_FEATURES_PER_TIER {
            return Err(DecodeError::InvalidLength("feature count"));
        }
        layers.push(Layer {
            id,
            feature_count: count,
            features,
        });
    }
    if seen.count_ones() != u32::from(EXPECTED_LAYERS) {
        for id in 1..=EXPECTED_LAYERS as u8 {
            if seen & (1 << (id - 1)) == 0 {
                return Err(DecodeError::MissingLayer(id));
            }
        }
    }
    if total != feature_count {
        return Err(DecodeError::InvalidLength("tier feature count"));
    }
    Ok(Tier {
        id,
        feature_count,
        layers,
    })
}

pub fn decode(bytes: &[u8]) -> Result<Pack, DecodeError> {
    let mut reader = Reader::new(bytes);
    if reader.take(8, "magic")? != MAGIC {
        return Err(DecodeError::InvalidMagic);
    }
    let schema = reader.u16("schema")?;
    if schema != SCHEMA {
        return Err(DecodeError::UnknownSchema(schema));
    }
    let header_len = reader.u16("header length")? as usize;
    if header_len != HEADER_LEN {
        return Err(DecodeError::InvalidHeader("header length"));
    }
    let quantisation = reader.u32("quantisation")?;
    if quantisation == 0 {
        return Err(DecodeError::InvalidHeader("quantisation"));
    }
    let longitude_extent = quantisation
        .checked_mul(180)
        .filter(|value| *value <= i32::MAX as u32)
        .ok_or(DecodeError::InvalidHeader("quantisation"))? as i32;
    let latitude_extent = quantisation
        .checked_mul(90)
        .filter(|value| *value <= i32::MAX as u32)
        .ok_or(DecodeError::InvalidHeader("quantisation"))? as i32;
    let extent = [
        reader.i32("extent")?,
        reader.i32("extent")?,
        reader.i32("extent")?,
        reader.i32("extent")?,
    ];
    if extent
        != [-longitude_extent, longitude_extent, -latitude_extent, latitude_extent]
    {
        return Err(DecodeError::InvalidHeader("extent"));
    }
    let layer_count = reader.u16("layer count")?;
    let tier_count = reader.u16("tier count")?;
    let tile_degrees = reader.u16("tile degrees")?;
    let _reserved = reader.u16("reserved")?;
    if layer_count != EXPECTED_LAYERS || tier_count != EXPECTED_TIERS || tile_degrees != 10 {
        return Err(DecodeError::InvalidHeader("counts"));
    }
    let manifest_sha256 = reader.take(32, "manifest digest")?.try_into().unwrap();
    let mut source_masks = [0u16; 10];
    for source_mask in &mut source_masks {
        *source_mask = reader.u16("source mapping")?;
    }
    let tiers = vec![
        decode_tier(&mut reader, extent, 0)?,
        decode_tier(&mut reader, extent, 1)?,
    ];
    let tile_count = reader.u32("tile count")?;
    if tile_count > 36 * 18 {
        return Err(DecodeError::InvalidLength("tile count"));
    }
    let mut tiles = Vec::with_capacity(tile_count as usize);
    let mut previous_tile = None;
    for _ in 0..tile_count {
        let x = reader.i16("tile x")?;
        let y = reader.i16("tile y")?;
        if !(0..36).contains(&x) || !(0..18).contains(&y) {
            return Err(DecodeError::InvalidCoordinate);
        }
        if let Some(previous) = previous_tile {
            if (x, y) <= previous {
                return Err(DecodeError::InvalidIndex("tile order"));
            }
        }
        previous_tile = Some((x, y));
        let count = reader.u32("tile feature count")?;
        if count > tiers[1].feature_count || usize::try_from(count).unwrap_or(usize::MAX) > reader.remaining() / 4 {
            return Err(DecodeError::InvalidLength("tile feature count"));
        }
        let mut features = Vec::with_capacity(count as usize);
        let mut previous_feature = None;
        for _ in 0..count {
            let feature = reader.u32("tile feature")?;
            if feature >= tiers[1].feature_count {
                return Err(DecodeError::InvalidIndex("feature reference"));
            }
            if let Some(previous) = previous_feature {
                if feature <= previous {
                    return Err(DecodeError::InvalidIndex("feature order"));
                }
            }
            previous_feature = Some(feature);
            features.push(feature);
        }
        tiles.push(Tile { x, y, features });
    }
    if reader.position != bytes.len() {
        return Err(DecodeError::InvalidLength("trailing pack bytes"));
    }
    Ok(Pack {
        header: Header {
            schema,
            quantisation,
            extent,
            layer_count,
            tier_count,
            tile_degrees,
            manifest_sha256,
            source_masks,
        },
        tiers,
        tiles,
    })
}

pub fn embedded() -> Result<Pack, DecodeError> {
    decode(PACK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_pack_has_both_tiers_and_every_layer_once() {
        let pack = embedded().expect("checked-in place-map pack must decode");
        assert_eq!(pack.tiers.len(), 2);
        for tier in &pack.tiers {
            assert_eq!(
                tier.layers.iter().map(|layer| layer.id).collect::<Vec<_>>(),
                (1..=10).collect::<Vec<_>>()
            );
            for layer in &tier.layers {
                assert!(layer.feature_count > 0);
                assert!(
                    layer
                        .features
                        .iter()
                        .map(|feature| feature.parts.iter().map(Vec::len).sum::<usize>())
                        .sum::<usize>()
                        > layer.feature_count as usize
                );
            }
        }
        assert!(!pack.tiles.is_empty());
    }

    #[test]
    fn header_embeds_the_source_manifest_digest() {
        use sha2::{Digest, Sha256};
        let mut digest = Sha256::new();
        digest.update(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/data/place-map/source-manifest.toml"
        )));
        let expected: [u8; 32] = digest.finalize().into();
        assert_eq!(embedded().unwrap().header.manifest_sha256, expected);
    }

    #[test]
    fn feature_bands_and_source_masks_are_opaque_to_the_decoder() {
        let mut bytes = PACK.to_vec();
        bytes[72..74].copy_from_slice(&0xdead_u16.to_le_bytes());
        let pack = decode(&bytes).expect("source masks are not a threshold ladder");
        assert_eq!(pack.header.source_masks[0], 0xdead);

        let mut reader = Reader::new(&[1, 0, 2, 0, 0, 0, 0, 0, 2, 2]);
        let feature = decode_feature(
            &mut reader,
            [-10_000, 10_000, -10_000, 10_000],
            [0, 0, 10_000, 10_000],
            1234,
        )
        .expect("arbitrary signed feature bands are valid payload data");
        assert_eq!(feature.band, 1234);
    }

    #[test]
    fn embedded_pack_contains_the_approved_contour_coverage() {
        let pack = embedded().expect("checked-in place-map pack must decode");
        let bands = |layer_id| {
            pack.tiers
                .iter()
                .flat_map(|tier| tier.layers.iter())
                .find(|layer| layer.id == layer_id)
                .unwrap()
                .features
                .iter()
                .map(|feature| feature.band)
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(
            bands(9),
            [100, 200, 400, 700, 1000, 1500, 2000, 2500, 3000, 4000, 5000, 6000]
                .into_iter()
                .collect()
        );
        let sea_floor = bands(10);
        for threshold in [-6000, -4000, -2000, -1000, -500, -250, -100, -10] {
            assert!(sea_floor.contains(&threshold));
        }
    }

    #[test]
    fn truncated_header_is_rejected() {
        assert_eq!(
            decode(&PACK[..8]).unwrap_err(),
            DecodeError::Truncated("schema")
        );
    }

    #[test]
    fn unknown_schema_is_rejected() {
        let mut bytes = PACK.to_vec();
        bytes[8] = 9;
        assert_eq!(decode(&bytes).unwrap_err(), DecodeError::UnknownSchema(9));
    }

    #[test]
    fn unknown_layer_is_rejected() {
        let mut bytes = PACK.to_vec();
        bytes[HEADER_LEN + 10] = 99;
        assert_eq!(decode(&bytes).unwrap_err(), DecodeError::UnknownLayer(99));
    }

    #[test]
    fn invalid_length_is_rejected() {
        let mut bytes = PACK.to_vec();
        // The first tier starts immediately after the 92-byte header. Its
        // payload length is the u32 after the id, reserved byte, and count.
        let tier_length = HEADER_LEN + 1 + 1 + 4;
        bytes[tier_length..tier_length + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            decode(&bytes),
            Err(DecodeError::Truncated("tier payload"))
                | Err(DecodeError::InvalidLength("tier payload"))
        ));
    }

    #[test]
    fn out_of_range_coordinate_is_rejected() {
        let mut bytes = PACK.to_vec();
        let offset = HEADER_LEN + 10 + 10 + 4;
        bytes[offset..offset + 4].copy_from_slice(&i32::MAX.to_le_bytes());
        assert_eq!(decode(&bytes).unwrap_err(), DecodeError::InvalidCoordinate);
    }

    #[test]
    fn feature_points_must_stay_inside_feature_bounds() {
        let mut reader = Reader::new(&[1, 0, 2, 0, 0, 0, 0, 0, 4, 0]);
        assert_eq!(
            decode_feature(&mut reader, [-10, 10, -10, 10], [0, 0, 1, 1], 0).unwrap_err(),
            DecodeError::InvalidCoordinate
        );
    }

    #[test]
    fn malformed_varints_and_part_counts_are_rejected_before_allocation() {
        let mut varint_reader = Reader::new(&[0xff, 0xff, 0xff, 0xff, 0x10]);
        assert_eq!(varint_reader.varint().unwrap_err(), DecodeError::InvalidVarint);
        let mut parts_reader = Reader::new(&[0xff, 0xff]);
        assert_eq!(
            decode_feature(&mut parts_reader, [-10, 10, -10, 10], [-1, -1, 1, 1], 0)
                .unwrap_err(),
            DecodeError::InvalidLength("part count")
        );
    }

    #[test]
    fn quantisation_extent_multiplication_is_checked() {
        let mut bytes = PACK.to_vec();
        bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            decode(&bytes).unwrap_err(),
            DecodeError::InvalidHeader("quantisation")
        );
    }

    #[test]
    fn out_of_range_tile_reference_is_rejected() {
        let mut bytes = PACK.to_vec();
        let first_tier_length =
            u32::from_le_bytes(bytes[HEADER_LEN + 6..HEADER_LEN + 10].try_into().unwrap())
                as usize;
        let second = HEADER_LEN + 10 + first_tier_length;
        let second_payload =
            u32::from_le_bytes(bytes[second + 6..second + 10].try_into().unwrap()) as usize;
        let tiles = second + 10 + second_payload;
        let first_feature = tiles + 12;
        bytes[first_feature..first_feature + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            decode(&bytes).unwrap_err(),
            DecodeError::InvalidIndex("feature reference")
        );
    }
}
