use super::*;

/// Equirectangular crop of a Local/Wide frame, longitude scaled by
/// cos(center_latitude) (a local standard parallel) so a small crop's
/// shapes stay close to true instead of stretching east-west away from the
/// equator. There is no visibility/horizon concept the way there was under
/// the azimuthal projection this replaced: every point projects, and a
/// ring or line is clipped to the frame's screen rectangle afterward with
/// plain Sutherland–Hodgman / segment clipping, not horizon-arc closing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FlatProjection {
    pub(super) center_longitude: f64,
    pub(super) center_latitude: f64,
    pub(super) cos_center_latitude: f64,
    pub(super) scale: f64,
}

impl FlatProjection {
    pub(super) fn new(frame: &Frame) -> Self {
        let center_latitude = frame.center_latitude;
        let cos_center_latitude = center_latitude.to_radians().cos();
        let half_width = (frame.longitude_span / 2.0).to_radians() * cos_center_latitude;
        let half_height = (frame.latitude_span / 2.0).to_radians();
        let scale = (VIEWBOX_WIDTH / 2.0 / half_width.max(f64::EPSILON))
            .min(VIEWBOX_HEIGHT / 2.0 / half_height.max(f64::EPSILON));
        Self {
            center_longitude: frame.center_longitude,
            center_latitude,
            cos_center_latitude,
            scale,
        }
    }

    pub(super) fn project_point(&self, point: ProjectedPoint) -> (f64, f64) {
        let longitude = unwrap_longitude(point.longitude, self.center_longitude);
        self.project_unwrapped(longitude, point.latitude)
    }

    pub(super) fn project_unwrapped(&self, longitude: f64, latitude: f64) -> (f64, f64) {
        let x = (longitude - self.center_longitude).to_radians() * self.cos_center_latitude;
        let y = (latitude - self.center_latitude).to_radians();
        (
            VIEWBOX_WIDTH / 2.0 + self.scale * x,
            VIEWBOX_HEIGHT / 2.0 - self.scale * y,
        )
    }

    /// West, east, south and north edges, in degrees, of everything this
    /// projection can draw: the viewBox plus the clip margin. The frame's
    /// own span is only a floor; fitting a 3:2 canvas usually shows more
    /// longitude than the frame asked for.
    pub(super) fn visible_bounds(&self) -> (f64, f64, f64, f64) {
        let half_width = (CLIP_MAX_X - VIEWBOX_WIDTH / 2.0)
            / (self.scale * self.cos_center_latitude.max(f64::EPSILON));
        let half_height = (CLIP_MAX_Y - VIEWBOX_HEIGHT / 2.0) / self.scale;
        let (half_width, half_height) = (half_width.to_degrees(), half_height.to_degrees());
        (
            self.center_longitude - half_width,
            self.center_longitude + half_width,
            (self.center_latitude - half_height).max(-90.0),
            (self.center_latitude + half_height).min(90.0),
        )
    }

    /// The pack's rings are already planar in [-180, 180] (see
    /// `raw_screen_copies`), so a frame that reaches past the antimeridian
    /// draws the copy of each ring shifted a turn east or west.
    pub(super) fn copies(&self, points: &[(i32, i32)], quantisation: u32) -> Vec<f64> {
        let (west, east, ..) = self.visible_bounds();
        let quantisation = f64::from(quantisation);
        let min = points.iter().map(|point| f64::from(point.0)).fold(f64::INFINITY, f64::min) / quantisation;
        let max = points.iter().map(|point| f64::from(point.0)).fold(f64::NEG_INFINITY, f64::max) / quantisation;
        [-360.0, 0.0, 360.0]
            .into_iter()
            .filter(|offset| min + offset <= east && max + offset >= west)
            .collect()
    }

    pub(super) fn project_ring(&self, points: &[(i32, i32)], quantisation: u32) -> Vec<Vec<(f64, f64)>> {
        raw_screen_copies(points, quantisation, &self.copies(points, quantisation), |longitude, latitude| {
            self.project_unwrapped(longitude, latitude)
        })
        .into_iter()
        .map(|screen| clip_closed_ring(&screen, CLIP_MIN_X, CLIP_MAX_X, CLIP_MIN_Y, CLIP_MAX_Y))
        .filter(|ring| ring.len() >= 4)
        .collect()
    }

    pub(super) fn project_part(&self, points: &[(i32, i32)], quantisation: u32) -> Vec<Vec<(f64, f64)>> {
        raw_screen_copies(points, quantisation, &self.copies(points, quantisation), |longitude, latitude| {
            self.project_unwrapped(longitude, latitude)
        })
        .iter()
        .flat_map(|screen| clip_polyline(screen, CLIP_MIN_X, CLIP_MAX_X, CLIP_MIN_Y, CLIP_MAX_Y))
        .collect()
    }
}
