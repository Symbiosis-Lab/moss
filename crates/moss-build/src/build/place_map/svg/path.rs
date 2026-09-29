use std::fmt::Write;

use crate::build::place_map::simplify::{simplify_screen_path, simplify_screen_path_by_area};

/// How a path is simplified in screen space, in device px of the 720x480
/// viewBox every caller renders into — see simplify.rs's doc comment for
/// why a fixed screen-space tolerance is right for every frame width. The
/// same split as the pack: land (and so the coast drawn from it), lines and
/// the globe by distance at 1 px; relief and sea-floor bands, the bulk of a
/// map's bytes, by area at 2 px^2, the approved design's (2 px)^2 / 2. The
/// pack's locator bands are already coarser than that, so on a locator
/// this pass barely moves them; on the world map, whose bands the pack
/// keeps at about 2 px^2, a coarser pass here would put back the long
/// edges the pack avoids.
#[derive(Debug, Clone, Copy)]
pub(super) enum Simplify {
    Distance(f64),
    Area(f64),
}
pub(super) const FINE: Simplify = Simplify::Distance(1.0);
pub(super) const BANDS: Simplify = Simplify::Area(2.0);

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

pub(super) fn serialize_path(
    paths: &[Vec<(f64, f64)>],
    closed: bool,
    simplify: Simplify,
) -> Option<String> {
    let mut output = String::new();
    let mut cursor = (0i32, 0i32);
    let mut wrote = false;
    for path in paths {
        if path.len() < if closed { 3 } else { 2 } {
            continue;
        }
        let mut points: Vec<(i32, i32)> = path.iter().map(|&(x, y)| (snap(x), snap(y))).collect();
        points.dedup();
        if closed && points.first() == points.last() {
            points.pop();
        }
        // A second simplification pass, in screen space this time: the
        // pack's own degree-space tolerance assumes a ~10deg/720px frame,
        // but a locator's actual frame can be wider (see simplify.rs), so
        // the same stored vertices can still be over-detailed once
        // projected here. `dedup` above only removes back-to-back
        // *identical* rounded points; this drops near-collinear ones too.
        points = match simplify {
            Simplify::Distance(tolerance) => simplify_screen_path(&points, tolerance),
            Simplify::Area(min_area) => simplify_screen_path_by_area(&points, min_area),
        };
        if points.len() < if closed { 3 } else { 2 } {
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
    /// of the subpath it closes. Returns each subpath's absolute start.
    fn svg_subpath_starts(path: &str) -> Vec<(i32, i32)> {
        let mut starts = Vec::new();
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
                starts.push(start);
            }
        }
        starts
    }

    /// The world map's bands arrive from the pack at about 2 px^2 of area
    /// detail; the render-time band pass must not be coarser than that.
    #[test]
    fn the_band_pass_keeps_detail_the_world_tier_keeps() {
        // A 3 px^2 bump on a long straight edge: kept at 2 px^2, dropped
        // at the 4 px^2 the pack uses for locator bands.
        let ring = vec![vec![(0.0, 0.0), (100.0, 0.0), (103.0, 1.0), (106.0, 0.0), (200.0, 0.0), (200.0, 100.0), (0.0, 100.0)]];
        let path = serialize_path(&ring, true, BANDS).unwrap();
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
        assert_eq!(svg_subpath_starts(&path), vec![(10, 10), (50, 60), (400, 20)]);
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
