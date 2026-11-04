//! Equal Earth (Savric, Patterson and Jenny 2018): an equal-area world
//! projection. Constants and formula as published.

const A1: f64 = 1.340264;
const A2: f64 = -0.081106;
const A3: f64 = 0.000893;
const A4: f64 = 0.003796;

/// The frame's latitude range in degrees: Antarctica is not drawn.
pub(crate) const NORTH: f64 = 84.0;
pub(crate) const SOUTH: f64 = -57.0;

/// Projected coordinates of a longitude and latitude in degrees, x east and y
/// north, in the projection's own units (the equator spans about 5.41).
pub(crate) fn equal_earth(lon_deg: f64, lat_deg: f64) -> (f64, f64) {
    let m = 3f64.sqrt() / 2.0;
    let theta = (m * lat_deg.to_radians().sin()).asin();
    let (t2, t6) = (theta * theta, theta.powi(6));
    let x = 2.0 * 3f64.sqrt() * lon_deg.to_radians() * theta.cos()
        / (3.0 * (9.0 * A4 * t6 * t2 + 7.0 * A3 * t6 + 3.0 * A2 * t2 + A1));
    let y = theta * (A1 + A2 * t2 + t6 * (A3 + A4 * t2));
    (x, y)
}

/// The framed world as an SVG viewBox of the given width: returns
/// `(height, project)` where `project` maps degrees to SVG coordinates.
pub(crate) fn frame(width: f64) -> (f64, impl Fn(f64, f64) -> (f64, f64)) {
    let half = equal_earth(180.0, 0.0).0;
    let scale = width / (2.0 * half);
    let top = equal_earth(0.0, NORTH).1;
    let bottom = equal_earth(0.0, SOUTH).1;
    let height = (top - bottom) * scale;
    (height, move |lon, lat| {
        let (x, y) = equal_earth(lon, lat);
        ((x + half) * scale, (top - y) * scale)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_published_reference_values() {
        assert_eq!(equal_earth(0.0, 0.0), (0.0, 0.0));
        let (x, _) = equal_earth(180.0, 0.0);
        assert!((x - 2.7066).abs() < 1e-3, "{x}");
        let (_, y) = equal_earth(0.0, 90.0);
        assert!((y - 1.3173).abs() < 1e-3, "{y}");
    }

    #[test]
    fn equal_area_holds_between_latitudes() {
        // A 1x1 degree cell near the equator and one at 60N hold the same
        // share of the sphere's area as they cover on the map, within the
        // projection's own approximation: the map cell is not inflated the way
        // a Mercator-family cell is (which would be about 4x here).
        let cell = |lat: f64| {
            let (x1, y1) = equal_earth(0.0, lat);
            let (x2, y2) = equal_earth(1.0, lat + 1.0);
            ((x2 - x1) * (y2 - y1)).abs() / (lat + 0.5).to_radians().cos()
        };
        let ratio = cell(0.0) / cell(60.0);
        assert!((ratio - 1.0).abs() < 0.08, "ratio {ratio}");
    }

    #[test]
    fn the_frame_spans_84_north_to_57_south() {
        let (height, project) = frame(1000.0);
        assert!(project(-180.0, 0.0).0.abs() < 1e-9);
        assert!(project(0.0, NORTH).1.abs() < 1e-9);
        assert!((project(0.0, SOUTH).1 - height).abs() < 1e-9);
        assert!((project(180.0, 0.0).0 - 1000.0).abs() < 1e-9);
        assert!(height > 400.0 && height < 600.0, "{height}");
    }
}
