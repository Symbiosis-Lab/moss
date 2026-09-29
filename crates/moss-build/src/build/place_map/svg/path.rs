use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use crate::build::place_map::simplify::{simplify_screen_path, simplify_screen_path_by_area};

/// How a path is simplified in screen space, in device px of the 720x480
/// viewBox every caller renders into — see simplify.rs's doc comment for
/// why a fixed screen-space tolerance is right for every frame width. The
/// same split as the pack: land (and so the coast drawn from it), reefs and
/// the globe by distance at 1 px; relief and sea-floor bands, the bulk of a
/// map's bytes, and rivers by area at 2 px^2, the approved design's
/// (2 px)^2 / 2. The pack's bands and rivers are already coarser than that
/// at a locator's zoom, so on a locator this pass barely moves them, while
/// a pass by distance would straighten the bends the pack kept; the world
/// map draws the same lines at about a thirtieth of the size, and this pass
/// does most of its simplifying.
#[derive(Debug, Clone, Copy)]
pub(super) enum Simplify {
    Distance(f64),
    Area(f64),
}
pub(super) const FINE: Simplify = Simplify::Distance(1.0);
pub(super) const BY_AREA: Simplify = Simplify::Area(2.0);

impl Simplify {
    /// The same pass for a map shown `scale` times smaller than its viewBox.
    pub(super) fn scaled(self, scale: f64) -> Self {
        match self {
            Self::Distance(tolerance) => Self::Distance(tolerance * scale),
            Self::Area(min_area) => Self::Area(min_area * scale * scale),
        }
    }
}

pub(super) fn snap(value: f64) -> i32 {
    value
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

fn sub_pixel_ring(points: &[(i32, i32)]) -> bool {
    let (mut min_x, mut max_x) = (i32::MAX, i32::MIN);
    let (mut min_y, mut max_y) = (i32::MAX, i32::MIN);
    for &(x, y) in points {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    max_x - min_x <= 2 && max_y - min_y <= 2
}

/// The vertices that two or more of the paths pass through.
fn shared_vertices(paths: &[Vec<(i32, i32)>]) -> HashSet<(i32, i32)> {
    let mut first_path = HashMap::new();
    let mut shared = HashSet::new();
    for (index, path) in paths.iter().enumerate() {
        for &point in path {
            if *first_path.entry(point).or_insert(index) != index {
                shared.insert(point);
            }
        }
    }
    shared
}

/// Simplify one path piecewise, between the vertices it shares with other
/// paths, so two shapes that meet along an edge keep one edge between
/// them. Simplified whole, each would drop different points of their
/// common boundary and open a sliver of background between them.
fn simplify_between_shared(points: &[(i32, i32)], shared: &HashSet<(i32, i32)>, closed: bool, simplify: Simplify) -> Vec<(i32, i32)> {
    let run = |run: &[(i32, i32)]| match simplify {
        Simplify::Distance(tolerance) => simplify_screen_path(run, tolerance),
        Simplify::Area(min_area) => simplify_screen_path_by_area(run, min_area),
    };
    if !points.iter().any(|point| shared.contains(point)) {
        return run(points);
    }
    let mut walk = points.to_vec();
    if closed {
        walk.push(points[0]);
    }
    let mut output = vec![walk[0]];
    let mut start = 0;
    for index in 1..walk.len() {
        if index == walk.len() - 1 || shared.contains(&walk[index]) {
            output.extend_from_slice(&run(&walk[start..=index])[1..]);
            start = index;
        }
    }
    if closed {
        output.pop();
    }
    output
}

pub(super) fn serialize_path(
    paths: &[Vec<(f64, f64)>],
    closed: bool,
    simplify: Simplify,
) -> Option<String> {
    let minimum = if closed { 3 } else { 2 };
    let snapped: Vec<Vec<(i32, i32)>> = paths
        .iter()
        .filter(|path| path.len() >= minimum)
        .map(|path| {
            let mut points: Vec<(i32, i32)> = path.iter().map(|&(x, y)| (snap(x), snap(y))).collect();
            points.dedup();
            if closed && points.first() == points.last() {
                points.pop();
            }
            points
        })
        .collect();
    // Bands are nested contours, so they share no edges; they touch only
    // where two contours snap to one pixel, and pinning those points kept
    // about 10% more of the world map's bytes for no edge at all.
    let shared = match simplify {
        Simplify::Distance(_) => shared_vertices(&snapped),
        Simplify::Area(_) => HashSet::new(),
    };
    let mut output = String::new();
    let mut cursor = (0i32, 0i32);
    let mut wrote = false;
    for points in snapped {
        // A second simplification pass, in screen space this time: the
        // pack's own degree-space tolerance assumes a ~10deg/720px frame,
        // but a locator's actual frame can be wider (see simplify.rs), so
        // the same stored vertices can still be over-detailed once
        // projected here. `dedup` above only removes back-to-back
        // *identical* rounded points; this drops near-collinear ones too.
        let points = simplify_between_shared(&points, &shared, closed, simplify);
        if points.len() < minimum {
            continue;
        }
        // A closed ring whose whole bounding box still fits inside a 2x2
        // device-px cell after simplification survives RDP (every point on
        // it genuinely deviates from its neighbours) but is still, at most,
        // a couple of pixels across — the same footprint the generator
        // itself already drops rings under at its own base frame width
        // (scripts/place-map/generate.mjs's minArea). At a wider-than-base
        // frame the ring can shrink under that bar on screen even though
        // it was worth keeping in the pack.
        if closed && sub_pixel_ring(&points) {
            continue;
        }
        let start = points[0];
        write!(output, "m{} {}", start.0 - cursor.0, start.1 - cursor.1)
            .expect("writing to String cannot fail");
        cursor = start;
        for point in points.iter().skip(1) {
            write!(output, "l{} {}", point.0 - cursor.0, point.1 - cursor.1)
                .expect("writing to String cannot fail");
            cursor = *point;
        }
        if closed {
            // SVG moves the current point back to the subpath's start on
            // `z`, so the next relative `m` must be measured from there, not
            // from the ring's last vertex.
            output.push('z');
            cursor = start;
        }
        wrote = true;
    }
    wrote.then_some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resolve a serialized path the way an SVG renderer does: relative
    /// commands accumulate, and `z` returns the current point to the start
    /// of the subpath it closes. Returns each subpath's absolute vertices.
    fn svg_subpaths(path: &str) -> Vec<Vec<(i32, i32)>> {
        let mut subpaths: Vec<Vec<(i32, i32)>> = Vec::new();
        let (mut current, mut start) = ((0, 0), (0, 0));
        let mut chars = path.chars().peekable();
        while let Some(command) = chars.next() {
            if command == 'z' {
                current = start;
                continue;
            }
            let mut take_number = || {
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                let mut digits = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '-' && digits.is_empty() || c.is_ascii_digit() {
                        digits.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                digits.parse::<i32>().unwrap()
            };
            let (dx, dy) = (take_number(), take_number());
            current = (current.0 + dx, current.1 + dy);
            if command == 'm' {
                start = current;
                subpaths.push(Vec::new());
            }
            subpaths.last_mut().unwrap().push(current);
        }
        subpaths
    }

    /// The corner at (50, 1) is a real corner of the upper shape but lies
    /// within a pixel of the lower shape's chord from (0, 0) to (100, 0).
    /// Simplified apart, the lower shape loses it and the upper keeps it,
    /// leaving a sliver between them; the same shape of defect split the
    /// Greenland ice sheet on the world map.
    #[test]
    fn shapes_that_meet_keep_their_common_edge() {
        let below = vec![(0.0, 0.0), (50.0, 1.0), (100.0, 0.0), (100.0, -50.0), (0.0, -50.0)];
        let above = vec![(50.0, 1.0), (0.0, 0.0), (0.0, 50.0), (100.0, 50.0), (100.0, 0.0)];
        let path = serialize_path(&[below, above], true, FINE).unwrap();
        let subpaths = svg_subpaths(&path);
        assert!(subpaths[0].contains(&(50, 1)), "{path}");
        assert!(subpaths[1].contains(&(50, 1)), "{path}");
    }

    /// The world map's bands arrive from the pack at about 2 px^2 of area
    /// detail; the render-time band pass must not be coarser than that.
    #[test]
    fn the_band_pass_keeps_detail_the_world_tier_keeps() {
        // A 3 px^2 bump on a long straight edge: kept at 2 px^2, dropped
        // at the 4 px^2 the pack uses for locator bands.
        let ring = vec![vec![(0.0, 0.0), (100.0, 0.0), (103.0, 1.0), (106.0, 0.0), (200.0, 0.0), (200.0, 100.0), (0.0, 100.0)]];
        let path = serialize_path(&ring, true, BY_AREA).unwrap();
        assert!(path.contains("l3 1"), "{path}");
    }

    #[test]
    fn every_closed_ring_starts_where_it_was_given() {
        // Each ring ends far from where it began, so a cursor left on the
        // last vertex after `z` would displace every following ring.
        let rings = vec![
            vec![(10.0, 10.0), (200.0, 10.0), (200.0, 300.0), (10.0, 300.0)],
            vec![(50.0, 60.0), (90.0, 60.0), (90.0, 120.0), (50.0, 120.0)],
            vec![(400.0, 20.0), (700.0, 20.0), (700.0, 450.0), (400.0, 450.0)],
        ];
        let path = serialize_path(&rings, true, FINE).unwrap();
        let starts: Vec<(i32, i32)> = svg_subpaths(&path).iter().map(|subpath| subpath[0]).collect();
        assert_eq!(starts, vec![(10, 10), (50, 60), (400, 20)]);
    }

    #[test]
    fn a_closed_ring_under_two_px_across_is_dropped() {
        // Every point here is >1px from its RDP neighbours (so the
        // screen-space simplify pass alone keeps all four), but the whole
        // ring still fits inside a 2x2px cell.
        let triangle = vec![vec![(10.0, 10.0), (11.0, 10.0), (11.0, 11.0), (10.0, 11.0)]];
        assert_eq!(serialize_path(&triangle, true, FINE), None);
    }

    #[test]
    fn a_closed_ring_spanning_more_than_two_px_survives() {
        let square = vec![vec![(10.0, 10.0), (13.0, 10.0), (13.0, 13.0), (10.0, 13.0)]];
        assert!(serialize_path(&square, true, FINE).is_some());
    }

    #[test]
    fn an_open_line_is_never_dropped_as_sub_pixel() {
        // sub_pixel_ring only applies to closed (fill) paths: a short line
        // segment is still real, visible stroke content, not a footprint
        // to area-filter the way a fill ring is.
        let short_line = vec![vec![(10.0, 10.0), (11.0, 10.0)]];
        assert!(serialize_path(&short_line, false, FINE).is_some());
    }
}
