use super::{emit_svg_with_mode, Feature, PlaceMapContext, PlaceMapTarget, Projection, SvgMapOptions, Writer};
use crate::vault::places::Precision;

/// q11 Brotli budget checked by the dev/CI matrix; production does not link a
/// compressor solely for this contract.
pub const LOCATOR_Q11_BROTLI_LIMIT: usize = 16 * 1024;
/// The same budget for a locator whose frame spans continents. It covers
/// tens of times a local frame's ground, and even drawn for its display
/// size, with every other elevation step, it costs about twice as much:
/// the whole world, the most a locator shows, comes to about 32 KiB, and
/// this leaves a quarter's headroom.
pub const WIDE_LOCATOR_Q11_BROTLI_LIMIT: usize = 40 * 1024;
/// Deterministic production safety ceiling for the sparse locator profile.
pub const LOCATOR_RAW_SAFETY_LIMIT: usize = 256 * 1024;

/// A locator is shown at most about 350 CSS px wide, under half the
/// 720-unit viewBox it is drawn in. A local frame's detail is bounded by
/// the pack, simplified for that frame; a wider frame's by the screen, so
/// a wide locator is simplified for the size it is shown at.
pub(super) const LOCATOR_DISPLAY_SCALE: f64 = 2.0;

/// A wide locator's elevation and depth steps: every other one, counted
/// from the highest and the deepest, and the shallowest sea step, which
/// runs along the coastline. At half the width, over a frame ten times a
/// local one's, neighbouring steps crowd within a pixel of each other, and
/// their outlines were most of a continent-wide locator's bytes.
pub(super) fn keep_alternate_bands(grouped: &mut [Vec<&Feature>]) {
    for (layer, highest_first) in [(9, true), (10, false)] {
        let mut bands: Vec<i16> = grouped[layer].iter().map(|feature| feature.band).collect();
        bands.sort_unstable();
        bands.dedup();
        if highest_first {
            bands.reverse();
        }
        let mut kept: Vec<i16> = bands.iter().copied().step_by(2).collect();
        if layer == 10 {
            kept.extend(bands.last());
        }
        grouped[layer].retain(|feature| kept.contains(&feature.band));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocatorProfile {
    ExactCity,
    Region,
    Country,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatorSvg {
    pub svg: String,
    pub profile: LocatorProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatorSafetyError {
    pub raw_bytes: usize,
}

/// Emit a deterministic sparse locator profile. Production enforces only the
/// raw safety ceiling; the q11 Brotli budget is enforced by the dev/CI matrix
/// without adding a compressor to the shipped build.
pub fn emit_locator_svg(
    context: &PlaceMapContext,
    target: &PlaceMapTarget,
    options: SvgMapOptions<'_>,
) -> Result<LocatorSvg, LocatorSafetyError> {
    let profile = match options.precision {
        Precision::Exact | Precision::City => LocatorProfile::ExactCity,
        Precision::Region => LocatorProfile::Region,
        Precision::Country => LocatorProfile::Country,
    };
    let mut svg = emit_svg_with_mode(context, target, options, true);
    let profile_name = match profile {
        LocatorProfile::ExactCity => "exact-city",
        LocatorProfile::Region => "region",
        LocatorProfile::Country => "country",
    };
    svg = svg.replacen(
        "<figure ",
        &format!("<figure data-map-locator-profile=\"{profile_name}\" "),
        1,
    );
    if svg.len() > LOCATOR_RAW_SAFETY_LIMIT {
        return Err(LocatorSafetyError {
            raw_bytes: svg.len(),
        });
    }
    Ok(LocatorSvg { svg, profile })
}

pub use emit_locator_svg as emit_locator;

impl Writer<'_> {
    /// A country-level locator's layers: land and its coast halo alone, with
    /// the other layer groups present but empty so every map keeps the same
    /// structure.
    pub(super) fn emit_sparse_layers(
        &mut self,
        quantisation: u32,
        projection: &Projection,
        grouped: &[Vec<&Feature>],
    ) {
        self.land_defs(quantisation, projection, grouped);
        self.coast();
        self.empty_named_layer("seafloor");
        self.land();
        self.empty_named_layer("relief");
    }

    fn empty_named_layer(&mut self, name: &str) {
        self.output.push_str(&format!(
            "<g id=\"{}\" data-map-layer=\"{name}\"/>",
            self.ids.get(&format!("layer-{name}"))
        ));
    }
}
