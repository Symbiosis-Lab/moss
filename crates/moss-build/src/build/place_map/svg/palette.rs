//! Elevation-to-colour lookups for the relief/sea-floor bands: the band
//! tint, stretched to the bands present in each frame, and the lighting
//! filter's own grey height-field ramp. Pure functions of band values,
//! deliberately free of the emitter's SVG-writing state.

/// The lighting filter's height-field source color for a relief band:
/// `lerp(#202020, #f2f2f2)` at position `(index+1)/12` over the twelve fixed
/// relief steps, not stretched per frame the way the tints are: the lighting
/// reads absolute height. `luminanceToAlpha` reads this as "how tall is this
/// point"; a constant fill modulated by per-feature *opacity* (what this
/// replaced) tops out at the fill's own brightness, so the near-white peak
/// (6000) read as 50% grey instead.
pub(super) fn relief_height_grey(band: i16) -> &'static str {
    match band {
        100 => "#323232",
        200 => "#434343",
        400 => "#555555",
        700 => "#666666",
        1000 => "#787878",
        1500 => "#898989",
        2000 => "#9b9b9b",
        2500 => "#acacac",
        3000 => "#bebebe",
        4000 => "#cfcfcf",
        5000 => "#e1e1e1",
        _ => "#f2f2f2",
    }
}

/// Every threshold the pack stores for a layer, in the order they nest: the
/// stretch a detail tile tints against, so one band has one
/// colour in every tile.
pub(super) fn band_ladder(name: &str) -> &'static [i16] {
    if name == "relief" {
        &[100, 200, 400, 700, 1000, 1500, 2000, 2500, 3000, 4000, 5000, 6000]
    } else {
        &[-10, -20, -30, -50, -100, -200, -1000, -2000, -3000, -4000, -5000, -6000]
    }
}

/// The smallest range a frame's land ramp is stretched over, in band units.
const LAND_FLOOR: f64 = 1500.0;
/// The smallest range a frame's sea ramp is stretched over, in band units.
const SEA_FLOOR: f64 = 1000.0;
/// The sea ramp's gamma: under 1 it spreads the shelf steps (10 to 200)
/// apart, which a linear ramp from 10 down to thousands would crowd into
/// one tone.
const SEA_GAMMA: f64 = 0.45;

/// Where a band sits on its ramp, 0 to 1, stretched to the bands present in
/// the frame: 0 is the lowest (shallowest) band present, and 1 is the
/// highest (deepest) one, or `floor` above the lowest when the present
/// bands span less than that. This is the approved design's per-frame
/// stretch, so a lowland frame still reaches its light tints and a frame
/// with only a shallow shelf still shows its steps.
pub(super) fn band_position(name: &str, band: i16, present: &[i16]) -> f64 {
    let (floor, gamma, key): (f64, f64, fn(i16) -> f64) = if name == "relief" {
        (LAND_FLOOR, 1.0, |band| f64::from(band))
    } else {
        (SEA_FLOOR, SEA_GAMMA, |band| -f64::from(band))
    };
    let low = present.iter().copied().map(key).fold(f64::INFINITY, f64::min);
    let high = present.iter().copied().map(key).fold(f64::NEG_INFINITY, f64::max);
    if !low.is_finite() {
        return 0.0;
    }
    let range = floor.max(high - low);
    ((key(band) - low) / range).clamp(0.0, 1.0).powf(gamma)
}

/// The band's tint: a mix of the theme's ramp endpoints at the band's
/// stretched position, as a CSS colour so each theme supplies its own
/// endpoints. Land runs low, mid, high (a three-stop ramp); the sea runs
/// shallow to deep.
pub(super) fn band_tint(name: &str, band: i16, present: &[i16]) -> String {
    let position = band_position(name, band, present);
    let (from, to, share) = if name != "relief" {
        (
            "var(--moss-place-water, #dbe7ea)",
            "var(--moss-place-sea-deep, #b9cdd6)",
            position,
        )
    } else if position <= 0.5 {
        (
            "var(--moss-place-land, #e3e6d5)",
            "var(--moss-place-land-mid, #ece4cc)",
            position / 0.5,
        )
    } else {
        (
            "var(--moss-place-land-mid, #ece4cc)",
            "var(--moss-place-land-high, #f7f2e6)",
            (position - 0.5) / 0.5,
        )
    };
    let percent = (share * 1000.0).round() / 10.0;
    format!("color-mix(in srgb, {from}, {to} {percent}%)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The pack stores a band per feature, not its ladder, so `band_ladder`
    /// repeats the thresholds the pack was built with. This compares the two
    /// as sets of bands; `band_position` uses only the ladder's lowest and
    /// highest, so a pack regenerated at other thresholds would leave a flat
    /// tile tinting against a ladder that no longer matches the bands it
    /// draws.
    #[test]
    fn band_ladder_is_the_set_of_bands_the_pack_stores() {
        let pack = crate::build::place_map::embedded().expect("checked-in place-map pack must decode");
        for (name, layer_id) in [("relief", 9), ("seafloor", 10)] {
            let stored: BTreeSet<i16> = pack
                .tiers
                .iter()
                .flat_map(|tier| tier.layers.iter())
                .filter(|layer| layer.id == layer_id)
                .flat_map(|layer| layer.features.iter().map(|feature| feature.band))
                .collect();
            let ladder: BTreeSet<i16> = band_ladder(name).iter().copied().collect();
            assert_eq!(ladder, stored, "{name} ladder differs from the pack's bands");
        }
    }

    /// Land stretches over the bands present or 1500 units, whichever is
    /// larger; the sea over the bands present or 1000, at gamma 0.45.
    #[test]
    fn positions_stretch_to_the_bands_present_with_the_design_floors() {
        let lowland = [100, 200, 400, 700, 1000];
        assert!((band_position("relief", 1000, &lowland) - 0.6).abs() < 1e-9);
        let highland = [100, 400, 1000, 2000, 3000, 4000];
        assert!((band_position("relief", 4000, &highland) - 1.0).abs() < 1e-9);
        assert!((band_position("relief", 2000, &highland) - 1900.0 / 3900.0).abs() < 1e-9);
        let shelf = [-10, -20, -30, -50, -100, -200];
        assert!((band_position("seafloor", -200, &shelf) - 0.19_f64.powf(0.45)).abs() < 1e-9);
        assert_eq!(band_position("seafloor", -10, &shelf), 0.0);
    }

    #[test]
    fn tints_mix_the_theme_endpoints() {
        let present = [100, 1600];
        assert_eq!(
            band_tint("relief", 1600, &present),
            "color-mix(in srgb, var(--moss-place-land-mid, #ece4cc), var(--moss-place-land-high, #f7f2e6) 100%)"
        );
        assert_eq!(
            band_tint("seafloor", -10, &[-10, -2000]),
            "color-mix(in srgb, var(--moss-place-water, #dbe7ea), var(--moss-place-sea-deep, #b9cdd6) 0%)"
        );
    }
}
