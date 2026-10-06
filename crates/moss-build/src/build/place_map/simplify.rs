//! Render-time point simplification for already-projected screen-space
//! paths.
//!
//! The checked-in pack's fine tier is simplified once, for a ~10-degree,
//! 720px-wide frame (place-map's README: coast at ~1 device px, every
//! other layer at ~2px). A map's actual frame is not always that
//! narrow — a multi-place frame spans all its places, and a country-level
//! place has a 180-degree floor (`geometry::privacy_floor`) — so the
//! same stored vertices land closer together on screen than the tolerance
//! they were simplified at assumed. Re-simplifying in already-projected
//! integer pixels, rather than re-deriving a per-frame tolerance in degree
//! space and threading it through the selection path, needs no frame width
//! at all: a fixed pixel tolerance is automatically the right amount of
//! simplification for any frame, because a wider frame's excess detail
//! shows up as points that are closer together on screen, and this
//! operates on exactly those screen coordinates.
//!
//! This is a real simplification pass, not what `path.rs`'s existing
//! `points.dedup()` already does: `dedup` only drops back-to-back
//! *identical* rounded points, so it does nothing about a zigzag of
//! near-collinear points a native ~0.05-degree contour grid leaves behind
//! once projected into a frame wider than the pack's own calibration.

/// Douglas-Peucker simplification directly on already-snapped integer
/// screen points, `tolerance` in the same 720x480 viewBox pixel units
/// `path.rs::snap` rounds to. Endpoints are always kept; everything else is
/// dropped if its perpendicular distance to the infinite line through the
/// two points bracketing it is within `tolerance` — the generator's own
/// degree-space pass (`scripts/place-map/generate.mjs`'s `rdp`) measures
/// distance to the bracketing *segment* instead, so the two do not always
/// drop the same points. Re-simplifying here, in a different coordinate
/// space, is for a tolerance the pack itself cannot know ahead of a
/// specific frame. Relief and sea-floor bands skip this pass for
/// `simplify_screen_path_by_area`, which keeps their curves.
pub(super) fn simplify_screen_path(points: &[(i32, i32)], tolerance: f64) -> Vec<(i32, i32)> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    mark_kept(points, 0, points.len() - 1, tolerance, &mut keep);
    points
        .iter()
        .zip(keep)
        .filter_map(|(&point, kept)| kept.then_some(point))
        .collect()
}

fn mark_kept(points: &[(i32, i32)], start: usize, end: usize, tolerance: f64, keep: &mut [bool]) {
    if end <= start + 1 {
        return;
    }
    let (start_x, start_y) = (f64::from(points[start].0), f64::from(points[start].1));
    let (end_x, end_y) = (f64::from(points[end].0), f64::from(points[end].1));
    let (dx, dy) = (end_x - start_x, end_y - start_y);
    let length_squared = dx * dx + dy * dy;
    let mut farthest_index = start;
    let mut farthest_distance = 0.0_f64;
    for index in (start + 1)..end {
        let (x, y) = (f64::from(points[index].0), f64::from(points[index].1));
        let distance = if length_squared < f64::EPSILON {
            (x - start_x).hypot(y - start_y)
        } else {
            ((x - start_x) * dy - (y - start_y) * dx).abs() / length_squared.sqrt()
        };
        if distance > farthest_distance {
            farthest_distance = distance;
            farthest_index = index;
        }
    }
    if farthest_distance > tolerance {
        keep[farthest_index] = true;
        mark_kept(points, start, farthest_index, tolerance, keep);
        mark_kept(points, farthest_index, end, tolerance, keep);
    }
}

/// Visvalingam-Whyatt simplification of already-snapped screen points, the
/// band counterpart of `simplify_screen_path`: repeatedly drop the interior
/// point whose triangle with its neighbours has the smallest area, until
/// every point left spans at least `min_area` square pixels. A point that
/// deviates little from its neighbours but over a long span covers enough
/// area to stay, so a gently curving band edge keeps its curve; a
/// chord-distance pass would replace it with one long straight edge, the
/// same collapse the generator avoids for bands (`generate.mjs`'s
/// `visvalingam`). Endpoints are always kept.
pub(super) fn simplify_screen_path_by_area(points: &[(i32, i32)], min_area: f64) -> Vec<(i32, i32)> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let n = points.len();
    if n <= 2 {
        return points.to_vec();
    }
    let area = |a: usize, b: usize, c: usize| {
        let (ax, ay) = (i64::from(points[a].0), i64::from(points[a].1));
        let (bx, by) = (i64::from(points[b].0), i64::from(points[b].1));
        let (cx, cy) = (i64::from(points[c].0), i64::from(points[c].1));
        // Twice the triangle area, exact in integers.
        ((bx - ax) * (cy - ay) - (cx - ax) * (by - ay)).abs()
    };
    let limit = (2.0 * min_area).ceil() as i64;
    let mut previous: Vec<usize> = (0..n).map(|index| index.saturating_sub(1)).collect();
    let mut next: Vec<usize> = (0..n).map(|index| (index + 1).min(n - 1)).collect();
    let mut current = vec![0i64; n];
    let mut alive = vec![true; n];
    let mut heap = BinaryHeap::new();
    for index in 1..n - 1 {
        current[index] = area(index - 1, index, index + 1);
        heap.push(Reverse((current[index], index)));
    }
    let mut floor = 0i64;
    while let Some(Reverse((value, index))) = heap.pop() {
        if !alive[index] || value != current[index] {
            continue;
        }
        if value.max(floor) >= limit {
            break;
        }
        floor = floor.max(value);
        alive[index] = false;
        let (before, after) = (previous[index], next[index]);
        next[before] = after;
        previous[after] = before;
        for neighbour in [before, after] {
            if neighbour != 0 && neighbour != n - 1 {
                current[neighbour] = area(previous[neighbour], neighbour, next[neighbour]).max(floor);
                heap.push(Reverse((current[neighbour], neighbour)));
            }
        }
    }
    points
        .iter()
        .zip(alive)
        .filter_map(|(&point, kept)| kept.then_some(point))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collinear_points_within_tolerance_are_dropped() {
        // A near-straight zigzag (each midpoint deviates well under 1px
        // from the start-end line) is exactly what a native-resolution
        // contour grid produces once projected into a wider-than-designed
        // frame: many points, all visually redundant.
        let points = vec![(0, 0), (10, 0), (20, 1), (30, -1), (40, 0), (50, 0)];
        let simplified = simplify_screen_path(&points, 1.0);
        assert_eq!(simplified, vec![(0, 0), (50, 0)]);
    }

    #[test]
    fn a_real_corner_survives_simplification() {
        // A genuine right angle, well past the 1px tolerance, must not be
        // flattened into a straight line: this proves the pass is a real
        // simplification, not a no-op that happens to pass the first test.
        let points = vec![(0, 0), (10, 0), (10, 10), (20, 10)];
        let simplified = simplify_screen_path(&points, 1.0);
        assert_eq!(simplified, points);
    }

    #[test]
    fn area_simplification_keeps_a_gentle_curve_and_drops_pixel_noise() {
        // A quarter of a 360 px circle, a point every 3.6 px: every point
        // is within 2 px of a 76 px chord, so a 2 px distance pass draws
        // chords that long, while an area pass keeps the curve.
        let arc: Vec<(i32, i32)> = (0..=157)
            .map(|step| {
                let angle = f64::from(step) / 100.0;
                ((360.0 * angle.cos()).round() as i32, (360.0 * angle.sin()).round() as i32)
            })
            .collect();
        let longest = |kept: Vec<(i32, i32)>| {
            kept.windows(2)
                .map(|pair| f64::from(pair[1].0 - pair[0].0).hypot(f64::from(pair[1].1 - pair[0].1)))
                .fold(0.0, f64::max)
        };
        let by_area = longest(simplify_screen_path_by_area(&arc, 4.0));
        let by_distance = longest(simplify_screen_path(&arc, 2.0));
        assert!(by_area < 50.0 && by_distance > 60.0, "by area {by_area} px, by distance {by_distance} px");
        // One-pixel zigzag along a straight line is noise under 4 px^2.
        let zigzag: Vec<(i32, i32)> = (0..40).map(|x| (x, x % 2)).collect();
        assert_eq!(simplify_screen_path_by_area(&zigzag, 4.0).len(), 2);
    }

    #[test]
    fn endpoints_are_always_kept_even_for_two_points() {
        let points = vec![(0, 0), (5, 5)];
        assert_eq!(simplify_screen_path(&points, 1.0), points);
    }
}
