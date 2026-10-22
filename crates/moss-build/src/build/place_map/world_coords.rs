//! The places explorer's world coordinates: the Patterson projection
//! `world.svg` is drawn in, forward and inverse. The inverse mirrors
//! `unproject` in the explorer's `projection.ts`.

use super::geometry::{patterson_y, world_viewbox_width, PATTERSON_K1, PATTERSON_K2, PATTERSON_K3, PATTERSON_K4};

pub(crate) const WORLD_HEIGHT: f64 = 480.0;

fn scale() -> f64 {
    WORLD_HEIGHT / 2.0 / patterson_y(std::f64::consts::FRAC_PI_2)
}

pub(crate) fn world_width() -> f64 {
    world_viewbox_width()
}

fn patterson_y_derivative(phi: f64) -> f64 {
    let p2 = phi * phi;
    let p4 = p2 * p2;
    PATTERSON_K1 + 5.0 * PATTERSON_K2 * p4 + 7.0 * PATTERSON_K3 * p4 * p2 + 9.0 * PATTERSON_K4 * p4 * p4
}

/// World units per radian of latitude at `latitude` degrees.
pub(crate) fn north_units_per_radian(latitude: f64) -> f64 {
    scale() * patterson_y_derivative(latitude.to_radians())
}

/// World units per radian of longitude.
pub(crate) fn east_units_per_radian() -> f64 {
    scale()
}

/// `(x, y)` in world units of a point given in degrees.
pub(crate) fn project(latitude: f64, longitude: f64) -> (f64, f64) {
    (
        world_width() / 2.0 + scale() * longitude.to_radians(),
        WORLD_HEIGHT / 2.0 - scale() * patterson_y(latitude.to_radians()),
    )
}

/// `(latitude, longitude)` in degrees of a world point.
#[cfg(test)]
pub(crate) fn unproject(x: f64, y: f64) -> (f64, f64) {
    (unproject_latitude(y), unproject_longitude(x))
}

pub(crate) fn unproject_longitude(x: f64) -> f64 {
    ((x - world_width() / 2.0) / scale()).to_degrees()
}

/// Newton-Raphson on the latitude, which has no closed form.
pub(crate) fn unproject_latitude(y: f64) -> f64 {
    let target = (WORLD_HEIGHT / 2.0 - y) / scale();
    let mut phi = target;
    for _ in 0..8 {
        phi -= (patterson_y(phi) - target) / patterson_y_derivative(phi);
    }
    phi.to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unproject_inverts_project() {
        for (lat, lng) in [(40.7048, -74.014), (-33.9, 151.2), (0.0, 0.0), (64.1, -21.9)] {
            let (x, y) = project(lat, lng);
            let (la, lo) = unproject(x, y);
            assert!((la - lat).abs() < 1e-9 && (lo - lng).abs() < 1e-9, "{lat},{lng} -> {la},{lo}");
        }
    }

    #[test]
    fn the_world_spans_its_height_pole_to_pole() {
        assert!((project(90.0, 0.0).1).abs() < 1e-9);
        assert!((project(-90.0, 0.0).1 - WORLD_HEIGHT).abs() < 1e-9);
    }
}
