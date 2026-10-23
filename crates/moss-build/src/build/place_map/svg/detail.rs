use std::fmt::Write as _;

use super::{length, Writer};

impl Writer<'_> {
    /// The ground under detail layers: the land tone over the water, since
    /// the world's data is too coarse at this depth to be drawn.
    pub(super) fn flat_ground(&mut self) {
        if self.layers.is_empty() {
            return;
        }
        write!(
            self.output,
            "<g id=\"{}\" data-map-layer=\"ground\"><rect width=\"{}\" height=\"{}\" fill=\"var(--moss-place-land, #e3e6d5)\"/></g>",
            self.ids.get("layer-ground"),
            length(self.canvas_width),
            length(self.canvas_height),
        )
        .expect("writing to String cannot fail");
    }

    /// One picture per layer in declaration order, each clipped to its own
    /// plates' footprints, over the ground and under routes and markers.
    pub(super) fn detail_layers(&mut self) {
        for (n, image) in self.layers.iter().enumerate() {
            let clip = self.ids.get(&format!("detail-clip-{n}"));
            write!(self.output, "<g id=\"{}\" data-map-layer=\"detail\"><clipPath id=\"{clip}\">", self.ids.get(&format!("layer-detail-{n}")))
                .expect("writing to String cannot fail");
            for footprint in &image.footprints {
                let points: Vec<String> = footprint.iter().map(|&(x, y)| format!("{},{}", length(x), length(y))).collect();
                write!(self.output, "<polygon points=\"{}\"/>", points.join(" ")).expect("writing to String cannot fail");
            }
            write!(
                self.output,
                "</clipPath><image href=\"{}\" x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" clip-path=\"url(#{clip})\"/></g>",
                image.href,
                length(self.canvas_width),
                length(self.canvas_height),
            )
            .expect("writing to String cannot fail");
        }
    }
}
