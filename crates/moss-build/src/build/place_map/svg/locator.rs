use super::{emit_svg_with_mode, Feature, PlaceMapContext, PlaceMapTarget, Projection, SvgMapOptions, Writer};
use crate::vault::places::Precision;

/// q11 Brotli budget checked by the dev/CI matrix; production does not link a
/// compressor solely for this contract.
pub const LOCATOR_Q11_BROTLI_LIMIT: usize = 16 * 1024;
/// Deterministic production safety ceiling for the sparse locator profile.
pub const LOCATOR_RAW_SAFETY_LIMIT: usize = 256 * 1024;

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
    pub(super) fn emit_locator_layers(
        &mut self,
        quantisation: u32,
        projection: &Projection,
        grouped: &[Vec<&Feature>],
        profile: LocatorProfile,
    ) {
        match profile {
            LocatorProfile::ExactCity => {
                self.emit_band_layer(quantisation, projection, grouped, 10, "seafloor", true);
                self.emit_filled_layer(
                    quantisation,
                    projection,
                    grouped,
                    2,
                    "land",
                    "var(--moss-place-land, #d6c89c)",
                );
                self.emit_band_layer(quantisation, projection, grouped, 9, "relief", false);
            }
            LocatorProfile::Region | LocatorProfile::Country => {
                self.empty_named_layer("seafloor");
                self.emit_filled_layer(
                    quantisation,
                    projection,
                    grouped,
                    2,
                    "land",
                    "var(--moss-place-land, #d6c89c)",
                );
                self.empty_named_layer("relief");
            }
        }
    }

    pub(super) fn emit_locator_surface_layers(
        &mut self,
        quantisation: u32,
        projection: &Projection,
        grouped: &[Vec<&Feature>],
        _profile: LocatorProfile,
    ) {
        self.emit_line_layer(
            quantisation,
            projection,
            grouped,
            1,
            "coast",
            "var(--moss-place-coast, #7d684b)",
        );
        for name in ["lakes", "rivers", "ice", "reefs", "salt", "built-up"] {
            self.empty_named_layer(name);
        }
    }

    fn empty_named_layer(&mut self, name: &str) {
        self.output.push_str(&format!(
            "<g id=\"{}\" data-map-layer=\"{name}\"/>",
            self.ids.get(&format!("layer-{name}"))
        ));
    }
}
