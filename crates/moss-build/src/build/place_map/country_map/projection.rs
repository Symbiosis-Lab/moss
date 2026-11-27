//! Equal-area projections: Equal Earth (Savric, Patterson and Jenny 2018) for
//! the world, constants and formula as published, and a conic equal-area one
//! (Albers) fitted to one country for its regions.

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

/// Conic equal-area (Albers) for a box `[west, south, east, north]` in degrees:
/// centred on the box, standard parallels a sixth of its latitude range in from
/// each edge. The generator's `conicFor` simplifies the region packs in this
/// projection, so the two must agree; both tests pin the same fixture.
pub(crate) fn conic_equal_area([west, south, east, north]: [f64; 4]) -> impl Fn(f64, f64) -> (f64, f64) {
    let span = north - south;
    let (p1, p2) = ((south + span / 6.0).to_radians(), (north - span / 6.0).to_radians());
    let n = (p1.sin() + p2.sin()) / 2.0;
    let c = p1.cos().powi(2) + 2.0 * n * p1.sin();
    let rho = move |lat: f64| (c - 2.0 * n * lat.sin()).sqrt() / n;
    let rho0 = rho(((south + north) / 2.0).to_radians());
    let lon0 = (west + east) / 2.0;
    move |lon, lat| {
        let (r, t) = (rho(lat.to_radians()), n * (lon - lon0).to_radians());
        (r * t.sin(), rho0 - r * t.cos())
    }
}

/// One country's regions framed as an SVG viewBox of the given width, with a
/// margin of 2% of the width: returns `(height, project)` as [`frame`] does.
pub(crate) fn fitted_frame<'a>(width: f64, rings: impl Iterator<Item = &'a [(f64, f64)]> + Clone) -> (f64, impl Fn(f64, f64) -> (f64, f64)) {
    let bounds = |b: [f64; 4], (x, y): (f64, f64)| [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
    let empty = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let degrees = rings.clone().flatten().copied().fold(empty, bounds);
    let conic = conic_equal_area(degrees);
    let [x0, y0, x1, y1] = rings.flatten().map(|&(lon, lat)| conic(lon, lat)).fold(empty, bounds);
    let scale = width / ((x1 - x0) * 1.04);
    let (left, top) = (x0 - (x1 - x0) * 0.02, y1 + (x1 - x0) * 0.02);
    let height = (y1 - y0 + (x1 - x0) * 0.04) * scale;
    (height, move |lon, lat| {
        let (x, y) = conic(lon, lat);
        ((x - left) * scale, (top - y) * scale)
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

    #[test]
    fn the_conic_keeps_area_true_across_its_box_and_centres_it() {
        let project = conic_equal_area([5.9, 47.3, 15.0, 55.1]);
        let (x, y) = project(10.45, 51.2);
        assert!(x.abs() < 1e-12 && y.abs() < 1e-12, "{x},{y}");
        let cell = |lat: f64| {
            let ((ax, ay), (bx, _), (_, cy)) = (project(10.0, lat), project(11.0, lat), project(10.0, lat + 1.0));
            ((bx - ax) * (cy - ay)).abs() / (lat + 0.5).to_radians().cos()
        };
        let ratio = cell(48.0) / cell(54.0);
        assert!((ratio - 1.0).abs() < 0.01, "ratio {ratio}");
        let (fx, fy) = project(8.0, 50.0);
        assert!((fx + 0.027459197).abs() < 1e-9 && (fy + 0.02050663).abs() < 1e-9, "the generator's twin pins the same point: {fx},{fy}");
    }

    #[test]
    fn a_fitted_frame_holds_the_rings_inside_its_margin() {
        let ring = [(6.0, 47.0), (15.0, 47.0), (15.0, 55.0), (6.0, 55.0)];
        let (height, project) = fitted_frame(1000.0, std::iter::once(&ring[..]));
        for &(lon, lat) in &ring {
            let (x, y) = project(lon, lat);
            assert!((0.0..=1000.0).contains(&x) && (0.0..=height).contains(&y), "{lon},{lat} -> {x},{y}");
        }
        let west = ring.iter().map(|&(lon, lat)| project(lon, lat).0).fold(f64::MAX, f64::min);
        assert!((west - 1000.0 * 0.02 / 1.04).abs() < 1e-6, "{west}");
        assert!(height > 900.0 && height < 1400.0, "Germany is taller than wide: {height}");
    }
}
