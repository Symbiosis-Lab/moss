use std::collections::BTreeMap;
use std::fmt::Write;

use super::{serialize_path, Feature, Projection, Writer, BY_AREA};

/// Stroke width for a river of Natural Earth `scalerank` (the pack keeps it
/// in the feature's band): 1.6 for the largest rivers, 0.14 thinner per
/// rank, never under 0.5. This is the approved design's taper. It draws
/// every rank and lets the taper, not a cutoff, keep small rivers quiet.
pub(super) fn river_width(rank: i16) -> f64 {
    (1.6 - 0.14 * f64::from(rank)).clamp(0.5, 1.6)
}

impl Writer<'_> {
    /// One stroke per width, thinnest first so trunk rivers draw on top.
    pub(super) fn emit_river_layer(
        &mut self,
        quantisation: u32,
        projection: &Projection,
        grouped: &[Vec<&Feature>],
        color: &str,
    ) {
        let mut by_width: BTreeMap<u32, Vec<Vec<(f64, f64)>>> = BTreeMap::new();
        for feature in &grouped[4] {
            let centi_px = (river_width(feature.band) * 100.0).round() as u32;
            let paths = by_width.entry(centi_px).or_default();
            for part in &feature.parts {
                paths.extend(projection.project_part(part, quantisation));
            }
        }
        write!(self.output, "<g id=\"{}\" data-map-layer=\"rivers\">", self.ids.get("layer-rivers"))
            .expect("writing to String cannot fail");
        for (centi_px, paths) in by_width {
            if let Some(path) = serialize_path(&paths, false, BY_AREA) {
                write!(
                    self.output,
                    "<path d=\"{path}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"{:.2}\" stroke-linecap=\"round\"/>",
                    f64::from(centi_px) / 100.0
                )
                .expect("writing to String cannot fail");
            }
        }
        self.output.push_str("</g>");
    }
}
