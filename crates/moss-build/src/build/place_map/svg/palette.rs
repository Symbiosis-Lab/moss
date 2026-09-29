//! Elevation-to-color lookups for the relief/sea-floor bands: the CSS
//! custom-property call sites `band_color` builds, and the lighting
//! filter's own grey height-field ramp. Pure functions of a band value,
//! deliberately free of the emitter's SVG-writing state.

/// The lighting filter's height-field source color for a relief band:
/// `lerp(#202020, #f2f2f2)` at position `(index+1)/12` over the same twelve
/// fixed relief steps `band_color`'s relief arm uses. `luminanceToAlpha`
/// reads this as "how tall is this point"; a constant fill modulated by
/// per-feature *opacity* (what this replaced) tops out at the fill's own
/// brightness, so the near-white peak (6000m) read as 50% grey instead.
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

pub(super) fn band_color(name: &str, band: i16) -> String {
    let (token, fallback) = match name {
        "relief" => {
            // threeStopLerp(landLow #d7d5c9, landMid #e3dcc8, landHigh
            // #f6f1e4) over the pack's fixed 100-6000m bands, stretched with
            // a 1500m floor — the approved design's own formula and
            // endpoints, not eyeballed. Matches --moss-place-relief-* in
            // site.css; 6000 is the catch-all since it is the top of the
            // fixed band list. Token suffix from this same match, never the
            // raw band value — see the seafloor arm for why that matters.
            let (suffix, fallback) = match band {
                100 => ("100", "#d7d5c9"),
                200 => ("200", "#d7d5c9"),
                400 => ("400", "#d8d6c9"),
                700 => ("700", "#d9d6c9"),
                1000 => ("1000", "#dbd7c9"),
                1500 => ("1500", "#ddd8c9"),
                2000 => ("2000", "#dfdac8"),
                2500 => ("2500", "#e1dbc8"),
                3000 => ("3000", "#e3dcc8"),
                4000 => ("4000", "#e9e3d1"),
                5000 => ("5000", "#f0eadb"),
                _ => ("6000", "#f6f1e4"),
            };
            (format!("--moss-place-relief-{suffix}"), fallback)
        }
        "seafloor" => {
            // lerp(seaShallow #e9eff2, seaDeep #bfd0dc), gamma-0.45 stretch
            // (floor 1000m), over the pack's twelve shelf/abyssal bands
            // (10/20/30/50/100/200/1000/2000/3000/4000/5000/6000). Matches
            // --moss-place-seafloor-* in site.css; -6000 is the catch-all.
            // Token suffix from this same match, never the raw band value:
            // an out-of-list depth used to name a CSS property site.css
            // never defined, so its var() silently fell back to this
            // literal color in every theme -- the light shade in dark mode.
            let (suffix, fallback) = match band {
                -10 => ("10", "#e9eff2"),
                -20 => ("20", "#e7edf1"),
                -30 => ("30", "#e6edf0"),
                -50 => ("50", "#e5ecf0"),
                -100 => ("100", "#e3eaef"),
                -200 => ("200", "#e0e8ed"),
                -1000 => ("1000", "#d6e1e8"),
                -2000 => ("2000", "#cfdce5"),
                -3000 => ("3000", "#cad8e2"),
                -4000 => ("4000", "#c6d5e0"),
                -5000 => ("5000", "#c2d2de"),
                _ => ("6000", "#bfd0dc"),
            };
            (format!("--moss-place-seafloor-{suffix}"), fallback)
        }
        _ => ("--moss-place-relief".to_string(), "#b8a878"),
    };
    format!("var({token}, {fallback})")
}
