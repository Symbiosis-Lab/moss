use super::geometry::ProjectedPoint;

pub const CENTER_X: f64 = 648.0;
pub const CENTER_Y: f64 = 72.0;
pub const RADIUS: f64 = 63.0;

pub fn globe_marker(point: ProjectedPoint, center: ProjectedPoint) -> Option<(f64, f64)> {
    project((point.longitude, point.latitude), center)
}

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
    globe_rings(points, center, quantisation).into_iter().next()
}

pub fn globe_rings(
    points: &[(i32, i32)],
    center: ProjectedPoint,
    quantisation: u32,
) -> Vec<Vec<(f64, f64)>> {
    let samples = sampled_path(points, quantisation, true);
    if samples.len() < 3 {
        return Vec::new();
    }
    let visible = samples
        .iter()
        .map(|&point| project(point, center))
        .collect::<Vec<_>>();
    if visible.iter().all(Option::is_some) {
        return vec![visible.into_iter().flatten().collect()];
    }
    let mut runs = Vec::new();
    let Some(invisible) = visible.iter().position(Option::is_none) else {
        return Vec::new();
    };
    // Rotate at an invisible sample so a visible run never gets split at the
    // arbitrary beginning of a source ring. This handles concave rings with
    // multiple visible runs and rings crossing the dateline.
    let n = samples.len();
    let mut index = (invisible + 1) % n;
    let mut previous = invisible;
    let mut in_run = false;
    let mut ring = Vec::new();
    for _ in 0..n {
        let is_visible = visible[index].is_some();
        if is_visible && !in_run {
            let entry =
                project_unchecked(boundary(samples[previous], samples[index], center), center);
            ring.clear();
            push_unique(&mut ring, entry);
            in_run = true;
        }
        if is_visible {
            push_unique(&mut ring, visible[index].unwrap());
        } else if in_run {
            let exit =
                project_unchecked(boundary(samples[previous], samples[index], center), center);
            push_unique(&mut ring, exit);
            let entry = ring[0];
            super::geometry::append_horizon_arc(
                &mut ring,
                (CENTER_X, CENTER_Y),
                exit,
                entry,
                RADIUS,
            );
            if ring.len() >= 3 {
                runs.push(std::mem::take(&mut ring));
            }
            in_run = false;
        }
        previous = index;
        index = (index + 1) % n;
    }
    if in_run {
        let exit = project_unchecked(
            boundary(samples[previous], samples[invisible], center),
            center,
        );
        push_unique(&mut ring, exit);
        let entry = ring[0];
        super::geometry::append_horizon_arc(
            &mut ring,
            (CENTER_X, CENTER_Y),
            exit,
            entry,
            RADIUS,
        );
        if ring.len() >= 3 {
            runs.push(ring);
        }
    }
    runs
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

    #[test]
    fn concave_rings_keep_multiple_visible_runs_and_polar_horizons_bounded() {
        let concave = [
            (-1200000, 200000),
            (-200000, 200000),
            (-200000, 400000),
            (-1200000, 400000),
            (-1200000, -400000),
            (-200000, -400000),
            (-200000, -200000),
            (-1200000, -200000),
        ];
        let runs = globe_rings(&concave, ProjectedPoint::new(0.0, 0.0).unwrap(), 10000);
        assert!(
            runs.len() >= 2,
            "concave globe ring must retain each visible run"
        );
        assert!(runs
            .iter()
            .flatten()
            .all(|&(x, y)| { (x - CENTER_X).hypot(y - CENTER_Y) <= RADIUS + 1.0 }));

        let polar = [
            (1700000, 800000),
            (-1700000, 800000),
            (-1700000, 880000),
            (1700000, 880000),
        ];
        let polar_runs = globe_rings(&polar, ProjectedPoint::new(180.0, 89.0).unwrap(), 10000);
        assert!(!polar_runs.is_empty());
        assert!(polar_runs
            .iter()
            .flatten()
            .all(|&(x, y)| { (x - CENTER_X).hypot(y - CENTER_Y) <= RADIUS + 1.0 }));
    }

    /// A ring that grazes the globe's visibility horizon more than once in
    /// almost the same place (a coastline wiggling right at the 90-degree
    /// cutoff, the way a real one does) produces two runs whose shared
    /// boundary is a near-duplicate point. The horizon-arc closer must
    /// still connect each run with a near-zero arc, not loop the long way
    /// around the disc — the crescent/full-disc-land artifact this
    /// regresses is defect 3's globe-inset half.
    #[test]
    fn globe_rings_closes_a_near_duplicate_crossing_tight_not_a_crescent() {
        // Near longitude 90 (the horizon at this center), a tiny jog in
        // latitude (1 unit at quantisation 10000 is 0.0001 degree) crosses
        // the visibility boundary twice more, right next to the main
        // crossing at latitude -30.
        let ring = [
            (800000, -300000),
            (1000000, -300000),
            (1000000, -299999),
            (800000, -299999),
            (800000, -299998),
            (1000000, -299998),
            (1000000, 300000),
            (800000, 300000),
        ];
        let center = ProjectedPoint::new(0.0, 0.0).unwrap();
        let runs = globe_rings(&ring, center, 10000);
        assert!(!runs.is_empty());
        let disc_area = std::f64::consts::PI * RADIUS * RADIUS;
        for run in &runs {
            let area: f64 = run
                .iter()
                .zip(run.iter().cycle().skip(1))
                .take(run.len())
                .map(|(&(x1, y1), &(x2, y2))| x1 * y2 - x2 * y1)
                .sum::<f64>()
                .abs()
                / 2.0;
            assert!(
                area < 0.5 * disc_area,
                "a run covered {area} of the {disc_area} globe disc — a near-duplicate \
                 crossing closed the long way around instead of tight"
            );
        }
    }

    /// A ring straddling the dateline must close the same way regardless of
    /// which direction it's wound: the closing arc's direction now comes
    /// from `signed_area` on each run's own *projected* (screen-space)
    /// points, which needs no antimeridian unwrapping (the projection
    /// already resolved that per point), unlike the old geographic-space
    /// winding this replaced.
    #[test]
    fn dateline_ring_closes_the_same_way_for_either_winding() {
        let clockwise = [
            (1790000, -100000),
            (-1790000, -100000),
            (-1790000, 100000),
            (1790000, 100000),
        ];
        let counterclockwise = [
            (1790000, 100000),
            (-1790000, 100000),
            (-1790000, -100000),
            (1790000, -100000),
        ];
        let center = ProjectedPoint::new(180.0, 0.0).unwrap();
        for ring in [&clockwise[..], &counterclockwise[..]] {
            let runs = globe_rings(ring, center, 10000);
            assert!(!runs.is_empty());
            let area: f64 = runs
                .iter()
                .map(|r| {
                    r.iter()
                        .zip(r.iter().cycle().skip(1))
                        .take(r.len())
                        .map(|(&(x1, y1), &(x2, y2))| x1 * y2 - x2 * y1)
                        .sum::<f64>()
                        .abs()
                        / 2.0
                })
                .sum();
            let disc_area = std::f64::consts::PI * RADIUS * RADIUS;
            assert!(
                area < 0.5 * disc_area,
                "dateline ring covered {area} of the {disc_area} disc — closed the long way"
            );
        }
    }
}
