use std::fmt::Write as _;

use super::{length, Writer};
use crate::build::place_map::layer_crop::{LayerImage, View};

impl<'a> Writer<'a> {
    pub(super) fn layers(&self) -> &'a [LayerImage] {
        self.ground.map_or(&[], |ground| &ground.images)
    }

    /// The part of the canvas this figure shows: all of it, unless detail layers cover less.
    pub(super) fn window(&self) -> View {
        self.ground.map_or(View { x: 0.0, y: 0.0, width: self.canvas_width, height: self.canvas_height }, |ground| ground.view)
    }

    fn cut(&self) -> Option<View> {
        self.ground.map(|ground| ground.view).filter(|view| *view != View::CANVAS)
    }

    /// A figure cut to its layers carries its shape as one number, for the stylesheet to size it by.
    pub(super) fn cut_figure_attrs(&self) -> String {
        self.cut().map_or_else(String::new, |view| format!(" data-map-cut style=\"--moss-place-map-ar:{:.3}\"", view.width / view.height))
    }

    /// The globe inset is drawn in the canvas's top-right corner; a cut figure moves it to the window's.
    pub(super) fn globe_shift(&self) -> String {
        self.cut().map_or_else(String::new, |view| format!(" transform=\"translate({} {})\"", length(view.x + view.width - View::CANVAS.width), length(view.y)))
    }

    /// The ground under detail layers: the land tone over the water, since
    /// the world's data is too coarse at this depth to be drawn.
    pub(super) fn flat_ground(&mut self) {
        if self.layers().is_empty() {
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
        let window = self.window();
        for (n, image) in self.layers().iter().enumerate() {
            let clip = self.ids.get(&format!("detail-clip-{n}"));
            write!(self.output, "<g id=\"{}\" data-map-layer=\"detail\"><clipPath id=\"{clip}\">", self.ids.get(&format!("layer-detail-{n}")))
                .expect("writing to String cannot fail");
            for footprint in &image.footprints {
                let points: Vec<String> = footprint.iter().map(|&(x, y)| format!("{},{}", length(x), length(y))).collect();
                write!(self.output, "<polygon points=\"{}\"/>", points.join(" ")).expect("writing to String cannot fail");
            }
            write!(
                self.output,
                "</clipPath><image href=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" clip-path=\"url(#{clip})\"/></g>",
                image.href,
                length(window.x),
                length(window.y),
                length(window.width),
                length(window.height),
            )
            .expect("writing to String cannot fail");
        }
    }
}
