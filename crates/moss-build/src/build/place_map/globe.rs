use super::geometry::ProjectedPoint;

pub const CENTER_X: f64 = 648.0;
pub const CENTER_Y: f64 = 72.0;
pub const RADIUS: f64 = 63.0;

pub fn globe_line(
    points: &[(i32, i32)],
    center: ProjectedPoint,
    quantisation: u32,
) -> Vec<Vec<(f64, f64)>> {
    let samples = sampled_path(points, quantisation, false);
    let mut paths = Vec::new();
    let mut current = Vec::new();
    for (index, &geo) in samples.iter().enumerate() {
        if let Some(projected) = project(geo, center) {
            if current.is_empty() && index > 0 && project(samples[index - 1], center).is_none() {
                current.push(project_unchecked(
                    boundary(samples[index - 1], geo, center),
                    center,
                ));
            }
            push_unique(&mut current, projected);
        } else if !current.is_empty() {
            let point = project_unchecked(boundary(samples[index - 1], geo, center), center);
            push_unique(&mut current, point);
            if current.len() >= 2 {
                paths.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if current.len() >= 2 {
        paths.push(current);
    }
    paths
}

pub fn globe_ring(
    points: &[(i32, i32)],
    center: ProjectedPoint,
    quantisation: u32,
) -> Option<Vec<(f64, f64)>> {
    let samples = sampled_path(points, quantisation, true);
    if samples.len() < 3 {
        return None;
    }
    let visible_count = samples
        .iter()
        .filter(|&&geo| project(geo, center).is_some())
        .count();
    if visible_count == samples.len() {
        return Some(
            samples
                .iter()
                .filter_map(|&geo| project(geo, center))
                .collect(),
        );
    }
    let start = samples
        .iter()
        .position(|&geo| project(geo, center).is_some())?;
    let mut ring = Vec::new();
    let mut index = start;
    let mut previous = samples[(start + samples.len() - 1) % samples.len()];
    let mut previous_visible = project(previous, center).is_some();
    for _ in 0..samples.len() {
        let geo = samples[index];
        let is_visible = project(geo, center).is_some();
        if is_visible && !previous_visible {
            push_unique(
                &mut ring,
                project_unchecked(boundary(previous, geo, center), center),
            );
        }
        if is_visible {
            push_unique(&mut ring, project(geo, center).unwrap());
        } else if previous_visible {
            let exit = project_unchecked(boundary(previous, geo, center), center);
            push_unique(&mut ring, exit);
            let entry = ring[0];
            append_horizon_arc(&mut ring, exit, entry);
            break;
        }
        previous = geo;
        previous_visible = is_visible;
        index = (index + 1) % samples.len();
    }
    (ring.len() >= 3).then_some(ring)
}

fn geo(point: (i32, i32), quantisation: u32) -> (f64, f64) {
    (
        f64::from(point.0) / f64::from(quantisation),
        f64::from(point.1) / f64::from(quantisation),
    )
}

fn interpolate(start: (f64, f64), end: (f64, f64), fraction: f64) -> (f64, f64) {
    let delta = (end.0 - start.0 + 180.0).rem_euclid(360.0) - 180.0;
    (
        start.0 + delta * fraction,
        start.1 + (end.1 - start.1) * fraction,
    )
}

fn sampled_path(points: &[(i32, i32)], quantisation: u32, closed: bool) -> Vec<(f64, f64)> {
    let mut samples = Vec::new();
    if points.len() < 2 {
        return samples;
    }
    let edge_count = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    for edge in 0..edge_count {
        let start = geo(points[edge], quantisation);
        let end = geo(points[(edge + 1) % points.len()], quantisation);
        for step in 0..8 {
            samples.push(interpolate(start, end, f64::from(step) / 8.0));
        }
    }
    if !closed {
        samples.push(geo(*points.last().unwrap(), quantisation));
    }
    samples
}

fn project(point: (f64, f64), center: ProjectedPoint) -> Option<(f64, f64)> {
    let longitude = point.0.to_radians();
    let latitude = point.1.to_radians();
    let center_longitude = center.longitude.to_radians();
    let center_latitude = center.latitude.to_radians();
    let delta = longitude - center_longitude;
    let visible = center_latitude.sin() * latitude.sin()
        + center_latitude.cos() * latitude.cos() * delta.cos();
    (visible >= 0.0).then_some(project_unchecked(point, center))
}

fn project_unchecked(point: (f64, f64), center: ProjectedPoint) -> (f64, f64) {
    let longitude = point.0.to_radians();
    let latitude = point.1.to_radians();
    let center_longitude = center.longitude.to_radians();
    let center_latitude = center.latitude.to_radians();
    let delta = longitude - center_longitude;
    (
        CENTER_X + RADIUS * latitude.cos() * delta.sin(),
        CENTER_Y
            - RADIUS
                * (center_latitude.cos() * latitude.sin()
                    - center_latitude.sin() * latitude.cos() * delta.cos()),
    )
}

fn boundary(start: (f64, f64), end: (f64, f64), center: ProjectedPoint) -> (f64, f64) {
    let start_visible = project(start, center).is_some();
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..40 {
        let middle = (low + high) / 2.0;
        if project(interpolate(start, end, middle), center).is_some() == start_visible {
            low = middle;
        } else {
            high = middle;
        }
    }
    interpolate(start, end, (low + high) / 2.0)
}

fn append_horizon_arc(ring: &mut Vec<(f64, f64)>, start: (f64, f64), end: (f64, f64)) {
    let start_angle = (start.1 - CENTER_Y).atan2(start.0 - CENTER_X);
    let end_angle = (end.1 - CENTER_Y).atan2(end.0 - CENTER_X);
    let tau = 2.0 * std::f64::consts::PI;
    let delta =
        (end_angle - start_angle + std::f64::consts::PI).rem_euclid(tau) - std::f64::consts::PI;
    let steps = ((delta.abs() / (std::f64::consts::PI / 16.0)).ceil() as usize).clamp(1, 64);
    for step in 1..=steps {
        let angle = start_angle + delta * step as f64 / steps as f64;
        let point = if step == steps {
            end
        } else {
            (
                CENTER_X + RADIUS * angle.cos(),
                CENTER_Y + RADIUS * angle.sin(),
            )
        };
        push_unique(ring, point);
    }
}

fn push_unique(points: &mut Vec<(f64, f64)>, point: (f64, f64)) {
    if points.last().is_none_or(|previous| {
        (previous.0 - point.0).abs() >= 1e-7 || (previous.1 - point.1).abs() >= 1e-7
    }) {
        points.push(point);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dateline_and_horizon_intersections_stay_on_the_globe() {
        let center = ProjectedPoint::new(180.0, 0.0).unwrap();
        let line = globe_line(&[(1790000, 0), (-1790000, 0)], center, 10000);
        assert!(!line.is_empty());
        assert!(line
            .iter()
            .flatten()
            .all(|&(x, y)| (x - CENTER_X).hypot(y - CENTER_Y) <= RADIUS + 1.0));
        let ring = globe_ring(
            &[(0, 0), (1200000, 700000), (1200000, -700000), (0, 0)],
            ProjectedPoint::new(0.0, 0.0).unwrap(),
            10000,
        );
        assert!(ring.is_some());
    }
}
