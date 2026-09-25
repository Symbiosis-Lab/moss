use super::{Feature, Pack};
use crate::vault::places::Precision;

pub const VIEWBOX_WIDTH: f64 = 720.0;
pub const VIEWBOX_HEIGHT: f64 = 480.0;
const POLAR_LIMIT: f64 = 85.0;
const MIN_FRAME_DEGREES: f64 = 10.0;
const CLIP_ANGLE_DEGREES: f64 = 90.0;
const CLIP_MARGIN: f64 = 24.0;
const CLIP_MIN_X: f64 = -CLIP_MARGIN;
const CLIP_MAX_X: f64 = VIEWBOX_WIDTH + CLIP_MARGIN;
const CLIP_MIN_Y: f64 = -CLIP_MARGIN;
const CLIP_MAX_Y: f64 = VIEWBOX_HEIGHT + CLIP_MARGIN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameTier {
    Local,
    Wide,
    World,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedPoint {
    pub longitude: f64,
    pub latitude: f64,
}

impl ProjectedPoint {
    pub fn new(longitude: f64, latitude: f64) -> Option<Self> {
        if !longitude.is_finite()
            || !latitude.is_finite()
            || !(-180.0..=180.0).contains(&longitude)
            || !(-90.0..=90.0).contains(&latitude)
        {
            return None;
        }
        Some(Self {
            longitude: normalize_longitude(longitude),
            latitude,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub center_longitude: f64,
    pub center_latitude: f64,
    pub longitude_span: f64,
    pub latitude_span: f64,
    pub tier: FrameTier,
    pub antimeridian: bool,
}

impl Frame {
    pub fn from_points<I>(points: &[ProjectedPoint], precisions: I) -> Option<Self>
    where
        I: Iterator<Item = Precision>,
    {
        if points.is_empty() {
            return None;
        }
        let floor = precisions
            .map(privacy_floor)
            .fold(MIN_FRAME_DEGREES, f64::max);
        let latitude_min = points
            .iter()
            .map(|point| point.latitude)
            .fold(90.0, f64::min);
        let latitude_max = points
            .iter()
            .map(|point| point.latitude)
            .fold(-90.0, f64::max);
        let (center_longitude, longitude_span, antimeridian) = circular_longitude_bounds(points);
        let latitude_span = (latitude_max - latitude_min).max(floor * 0.68).min(170.0);
        let longitude_span = longitude_span.max(floor).min(360.0);
        let center_latitude =
            ((latitude_min + latitude_max) / 2.0).clamp(-POLAR_LIMIT, POLAR_LIMIT);
        let tier = if points
            .iter()
            .any(|point| point.latitude.abs() > POLAR_LIMIT)
            || longitude_span > 180.0
        {
            FrameTier::World
        } else if longitude_span > 60.0 || floor >= privacy_floor(Precision::Country) {
            FrameTier::Wide
        } else {
            FrameTier::Local
        };
        Some(Self {
            center_longitude,
            center_latitude,
            longitude_span,
            latitude_span,
            tier,
            antimeridian,
        })
    }

    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        let half_lon = self.longitude_span / 2.0;
        let half_lat = self.latitude_span / 2.0;
        (
            self.center_longitude - half_lon,
            self.center_longitude + half_lon,
            self.center_latitude - half_lat,
            self.center_latitude + half_lat,
        )
    }

    pub fn contains(&self, point: ProjectedPoint) -> bool {
        let delta = shortest_longitude_delta(point.longitude - self.center_longitude).abs();
        delta <= self.longitude_span / 2.0
            && (point.latitude - self.center_latitude).abs() <= self.latitude_span / 2.0
    }
}

pub fn privacy_floor(precision: Precision) -> f64 {
    match precision {
        Precision::Exact => MIN_FRAME_DEGREES,
        Precision::City => 18.0,
        Precision::Region => 52.0,
        Precision::Country => 180.0,
    }
}

pub fn marker_radius(precision: Precision) -> f64 {
    match precision {
        Precision::Exact => 6.0,
        Precision::City => 9.0,
        Precision::Region => 14.0,
        Precision::Country => 20.0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    frame: Frame,
    sin_latitude: f64,
    cos_latitude: f64,
    scale: f64,
}

impl Projection {
    pub fn new(frame: &Frame) -> Self {
        let latitude = frame.center_latitude.to_radians();
        let width_scale = frame_width_scale(frame.longitude_span);
        let (max_x, max_y) = frame_raw_extent(frame, latitude.sin(), latitude.cos());
        let scale = width_scale.min((VIEWBOX_HEIGHT / 2.0) / max_y.max(f64::EPSILON));
        Self {
            frame: *frame,
            sin_latitude: latitude.sin(),
            cos_latitude: latitude.cos(),
            scale: scale.min((VIEWBOX_WIDTH / 2.0) / max_x.max(f64::EPSILON)),
        }
    }

    /// Spherical Lambert azimuthal equal-area projection. The frame's
    /// longitude unwrap is applied before this formula so a date-line frame
    /// never creates a world-spanning segment.
    pub fn project(&self, point: ProjectedPoint) -> Option<(f64, f64)> {
        if point.latitude.abs() > POLAR_LIMIT {
            return None;
        }
        let latitude = point.latitude.to_radians();
        let longitude = unwrap_longitude(point.longitude, self.frame.center_longitude).to_radians();
        let center_longitude = self.frame.center_longitude.to_radians();
        let delta = longitude - center_longitude;
        let cosine_angle =
            self.sin_latitude * latitude.sin() + self.cos_latitude * latitude.cos() * delta.cos();
        if cosine_angle + f64::EPSILON < CLIP_ANGLE_DEGREES.to_radians().cos() {
            return None;
        }
        let denominator = 1.0 + cosine_angle;
        if denominator <= f64::EPSILON {
            return None;
        }
        let raw_scale = (2.0 / denominator).sqrt();
        let x = raw_scale * latitude.cos() * delta.sin();
        let y = raw_scale
            * (self.cos_latitude * latitude.sin()
                - self.sin_latitude * latitude.cos() * delta.cos());
        Some((
            VIEWBOX_WIDTH / 2.0 + self.scale * x,
            VIEWBOX_HEIGHT / 2.0 - self.scale * y,
        ))
    }

    pub fn project_part(&self, points: &[(i32, i32)], quantisation: u32) -> Vec<Vec<(f64, f64)>> {
        let coordinates: Vec<ProjectedPoint> = points
            .iter()
            .filter_map(|&(longitude, latitude)| {
                let longitude = f64::from(longitude) / f64::from(quantisation);
                let latitude = f64::from(latitude) / f64::from(quantisation);
                ProjectedPoint::new(longitude, latitude)
            })
            .collect();
        split_antimeridian(&coordinates)
            .into_iter()
            .flat_map(|part| self.project_path(&part))
            .collect()
    }

    fn project_path(&self, points: &[ProjectedPoint]) -> Vec<Vec<(f64, f64)>> {
        let mut paths: Vec<Vec<(f64, f64)>> = Vec::new();
        for pair in points.windows(2) {
            for (start, end) in self.projected_segments(pair[0], pair[1]) {
                let Some((start, end)) = clip_segment(start, end) else {
                    continue;
                };
                if let Some(path) = paths.last_mut() {
                    if same_point(path.last().copied(), start) {
                        path.push(end);
                        continue;
                    }
                }
                paths.push(vec![start, end]);
            }
        }
        paths
    }

    fn projected_segments(
        &self,
        start: ProjectedPoint,
        end: ProjectedPoint,
    ) -> Vec<((f64, f64), (f64, f64))> {
        let mut cuts = vec![0.0, 1.0];
        add_latitude_cut(&mut cuts, start, end, POLAR_LIMIT);
        add_latitude_cut(&mut cuts, start, end, -POLAR_LIMIT);
        let sample_count = 32;
        let mut previous_t = 0.0;
        let mut previous_visible = self.project(interpolate(start, end, 0.0)).is_some();
        for index in 1..=sample_count {
            let current_t = f64::from(index) / f64::from(sample_count);
            let current_visible = self.project(interpolate(start, end, current_t)).is_some();
            if current_visible != previous_visible {
                cuts.push(find_visibility_boundary(
                    self, start, end, previous_t, current_t,
                ));
            }
            previous_t = current_t;
            previous_visible = current_visible;
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        cuts.windows(2)
            .filter_map(|window| {
                let middle = (window[0] + window[1]) / 2.0;
                self.project(interpolate(start, end, middle))?;
                let a = self
                    .project(interpolate(start, end, window[0]))
                    .or_else(|| {
                        self.project(interpolate(
                            start,
                            end,
                            window[0] + (middle - window[0]) * 1e-7,
                        ))
                    })?;
                let b = self
                    .project(interpolate(start, end, window[1]))
                    .or_else(|| {
                        self.project(interpolate(
                            start,
                            end,
                            window[1] - (window[1] - middle) * 1e-7,
                        ))
                    })?;
                Some((a, b))
            })
            .collect()
    }
}

fn frame_width_scale(longitude_span: f64) -> f64 {
    let theta = (longitude_span / 2.0).to_radians();
    (VIEWBOX_WIDTH / 2.0) / (2.0 * (theta / 2.0).sin()).max(f64::EPSILON)
}

fn frame_raw_extent(frame: &Frame, sin_center: f64, cos_center: f64) -> (f64, f64) {
    let (west, east, south, north) = frame.bounds();
    let points = [
        (west, south),
        (west, north),
        (east, south),
        (east, north),
        (frame.center_longitude, south),
        (frame.center_longitude, north),
    ];
    let mut max_x: f64 = 0.0;
    let mut max_y: f64 = 0.0;
    for (longitude, latitude) in points {
        let latitude = latitude.to_radians();
        let delta = shortest_longitude_delta(longitude - frame.center_longitude).to_radians();
        let denominator =
            1.0 + sin_center * latitude.sin() + cos_center * latitude.cos() * delta.cos();
        if denominator <= f64::EPSILON {
            continue;
        }
        let raw_scale = (2.0 / denominator).sqrt();
        max_x = max_x.max((raw_scale * latitude.cos() * delta.sin()).abs());
        max_y = max_y.max(
            (raw_scale * (cos_center * latitude.sin() - sin_center * latitude.cos() * delta.cos()))
                .abs(),
        );
    }
    (max_x.max(f64::EPSILON), max_y.max(f64::EPSILON))
}

fn add_latitude_cut(
    cuts: &mut Vec<f64>,
    start: ProjectedPoint,
    end: ProjectedPoint,
    latitude: f64,
) {
    let delta = end.latitude - start.latitude;
    if delta.abs() > f64::EPSILON {
        let t = (latitude - start.latitude) / delta;
        if (0.0..1.0).contains(&t) {
            cuts.push(t);
        }
    }
}

fn interpolate(start: ProjectedPoint, end: ProjectedPoint, t: f64) -> ProjectedPoint {
    ProjectedPoint {
        longitude: start.longitude + (end.longitude - start.longitude) * t,
        latitude: start.latitude + (end.latitude - start.latitude) * t,
    }
}

fn find_visibility_boundary(
    projection: &Projection,
    start: ProjectedPoint,
    end: ProjectedPoint,
    mut low: f64,
    mut high: f64,
) -> f64 {
    let low_visible = projection.project(interpolate(start, end, low)).is_some();
    for _ in 0..40 {
        let middle = (low + high) / 2.0;
        if projection
            .project(interpolate(start, end, middle))
            .is_some()
            == low_visible
        {
            low = middle;
        } else {
            high = middle;
        }
    }
    (low + high) / 2.0
}

fn clip_segment(start: (f64, f64), end: (f64, f64)) -> Option<((f64, f64), (f64, f64))> {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let mut lower: f64 = 0.0;
    let mut upper: f64 = 1.0;
    for (coordinate, delta, minimum, maximum) in [
        (start.0, dx, CLIP_MIN_X, CLIP_MAX_X),
        (start.1, dy, CLIP_MIN_Y, CLIP_MAX_Y),
    ] {
        if delta.abs() < f64::EPSILON {
            if coordinate < minimum || coordinate > maximum {
                return None;
            }
            continue;
        }
        let mut low = (minimum - coordinate) / delta;
        let mut high = (maximum - coordinate) / delta;
        if low > high {
            std::mem::swap(&mut low, &mut high);
        }
        lower = lower.max(low);
        upper = upper.min(high);
        if lower > upper {
            return None;
        }
    }
    Some((
        (start.0 + dx * lower, start.1 + dy * lower),
        (start.0 + dx * upper, start.1 + dy * upper),
    ))
}

fn same_point(first: Option<(f64, f64)>, second: (f64, f64)) -> bool {
    first
        .is_some_and(|point| (point.0 - second.0).abs() < 1e-7 && (point.1 - second.1).abs() < 1e-7)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileSelection {
    pub tier: u8,
    pub tiles: Vec<(i16, i16)>,
}

impl TileSelection {
    pub fn for_frame(pack: &Pack, frame: &Frame) -> Self {
        if frame.tier == FrameTier::World {
            return Self {
                tier: 0,
                tiles: Vec::new(),
            };
        }
        let (west, east, south, north) = frame.bounds();
        let x_start = tile_x(west);
        let x_end = tile_x(east);
        let y_start = tile_y(south);
        let y_end = tile_y(north);
        let mut tiles = Vec::new();
        for tile in &pack.tiles {
            let in_x = if x_start <= x_end {
                tile.x >= x_start && tile.x <= x_end
            } else {
                tile.x >= x_start || tile.x <= x_end
            };
            if in_x && tile.y >= y_start && tile.y <= y_end {
                tiles.push((tile.x, tile.y));
            }
        }
        Self { tier: 1, tiles }
    }

    pub fn features<'a>(&self, pack: &'a Pack) -> Vec<(u8, &'a Feature)> {
        let tier = &pack.tiers[usize::from(self.tier)];
        if self.tier == 0 {
            return tier
                .layers
                .iter()
                .flat_map(|layer| {
                    layer
                        .features
                        .iter()
                        .map(move |feature| (layer.id, feature))
                })
                .collect();
        }
        let tile_ids: std::collections::BTreeSet<u32> = pack
            .tiles
            .iter()
            .filter(|tile| self.tiles.contains(&(tile.x, tile.y)))
            .flat_map(|tile| tile.features.iter().copied())
            .collect();
        let mut global_index = 0u32;
        let mut features = Vec::new();
        for layer in &tier.layers {
            for (index, feature) in layer.features.iter().enumerate() {
                let feature_id = global_index + index as u32;
                if tile_ids.contains(&feature_id) {
                    features.push((layer.id, feature));
                }
            }
            global_index += layer.feature_count;
        }
        features
    }
}

fn tile_x(longitude: f64) -> i16 {
    (((normalize_longitude(longitude) + 180.0) / 10.0).floor() as i16).clamp(0, 35)
}

fn tile_y(latitude: f64) -> i16 {
    (((latitude.clamp(-90.0, 90.0) + 90.0) / 10.0).floor() as i16).clamp(0, 17)
}

pub fn normalize_longitude(longitude: f64) -> f64 {
    let mut normalized = (longitude + 180.0).rem_euclid(360.0) - 180.0;
    if normalized == 180.0 {
        normalized = -180.0;
    }
    normalized
}

fn shortest_longitude_delta(delta: f64) -> f64 {
    (delta + 180.0).rem_euclid(360.0) - 180.0
}

fn unwrap_longitude(longitude: f64, center: f64) -> f64 {
    center + shortest_longitude_delta(longitude - center)
}

fn circular_longitude_bounds(points: &[ProjectedPoint]) -> (f64, f64, bool) {
    let mut longitudes: Vec<f64> = points
        .iter()
        .map(|point| normalize_longitude(point.longitude))
        .collect();
    longitudes.sort_by(f64::total_cmp);
    if longitudes.len() == 1 {
        return (longitudes[0], 0.0, false);
    }
    let mut largest_gap = (0usize, -1.0f64);
    for index in 0..longitudes.len() {
        let next = if index + 1 < longitudes.len() {
            longitudes[index + 1]
        } else {
            longitudes[0] + 360.0
        };
        let gap = next - longitudes[index];
        if gap > largest_gap.1 {
            largest_gap = (index, gap);
        }
    }
    let start_index = (largest_gap.0 + 1) % longitudes.len();
    let start = longitudes[start_index];
    let end = longitudes[largest_gap.0]
        + if largest_gap.0 < start_index {
            360.0
        } else {
            0.0
        };
    let span = end - start;
    let center = normalize_longitude(start + span / 2.0);
    (
        center,
        span,
        start > normalize_longitude(end) || span > 180.0,
    )
}

fn split_antimeridian(points: &[ProjectedPoint]) -> Vec<Vec<ProjectedPoint>> {
    if points.is_empty() {
        return Vec::new();
    }
    let mut parts = vec![vec![points[0]]];
    for &point in &points[1..] {
        let previous = *parts.last().and_then(|part| part.last()).unwrap();
        let delta = point.longitude - previous.longitude;
        if delta.abs() <= 180.0 {
            parts.last_mut().unwrap().push(point);
            continue;
        }
        let adjusted_longitude = point.longitude + if delta > 0.0 { -360.0 } else { 360.0 };
        let boundary = if delta > 0.0 { -180.0 } else { 180.0 };
        let fraction = (boundary - previous.longitude) / (adjusted_longitude - previous.longitude);
        let crossing = ProjectedPoint {
            longitude: boundary,
            latitude: previous.latitude + (point.latitude - previous.latitude) * fraction,
        };
        let counterpart = ProjectedPoint {
            longitude: -boundary,
            latitude: crossing.latitude,
        };
        parts.last_mut().unwrap().push(crossing);
        parts.push(vec![counterpart, point]);
    }
    parts.into_iter().filter(|part| part.len() >= 2).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longitude_normalisation_is_half_open() {
        assert_eq!(normalize_longitude(180.0), -180.0);
        assert_eq!(normalize_longitude(-540.0), -180.0);
        assert_eq!(normalize_longitude(540.0), -180.0);
    }

    #[test]
    fn date_line_points_use_the_short_arc() {
        let points = [
            ProjectedPoint::new(179.0, 10.0).unwrap(),
            ProjectedPoint::new(-179.0, 11.0).unwrap(),
        ];
        let frame =
            Frame::from_points(&points, [Precision::Exact, Precision::Exact].into_iter()).unwrap();
        assert!(frame.longitude_span < 20.0);
        assert!(frame.antimeridian);
    }

    #[test]
    fn privacy_floor_wins_over_a_tiny_coordinate_span() {
        let points = [
            ProjectedPoint::new(10.0, 10.0).unwrap(),
            ProjectedPoint::new(10.1, 10.1).unwrap(),
        ];
        let frame = Frame::from_points(&points, [Precision::Country].into_iter()).unwrap();
        assert!(frame.longitude_span >= privacy_floor(Precision::Country));
        assert_eq!(frame.tier, FrameTier::Wide);
    }

    #[test]
    fn laea_center_is_the_viewbox_center() {
        let points = [ProjectedPoint::new(12.0, 41.0).unwrap()];
        let frame = Frame::from_points(&points, [Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        let projected = projection.project(points[0]).unwrap();
        assert!((projected.0 - VIEWBOX_WIDTH / 2.0).abs() < 0.001);
        assert!((projected.1 - VIEWBOX_HEIGHT / 2.0).abs() < 0.001);
    }

    #[test]
    fn laea_scale_uses_the_frame_width_without_axis_distortion() {
        let points = [
            ProjectedPoint::new(-5.0, 0.0).unwrap(),
            ProjectedPoint::new(5.0, 0.0).unwrap(),
        ];
        let frame =
            Frame::from_points(&points, [Precision::Exact, Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        let west = projection.project(points[0]).unwrap();
        let east = projection.project(points[1]).unwrap();
        assert!((west.0 - 7.454599).abs() < 0.001);
        assert!((east.0 - 712.545401).abs() < 0.001);
        assert!((west.1 - 240.0).abs() < 0.001);
        assert!((east.1 - 240.0).abs() < 0.001);
    }

    #[test]
    fn laea_scale_fits_a_tall_frame_without_changing_aspect_ratio() {
        let points = [
            ProjectedPoint::new(0.0, -20.0).unwrap(),
            ProjectedPoint::new(0.0, 20.0).unwrap(),
        ];
        let frame =
            Frame::from_points(&points, [Precision::Exact, Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        let south = projection.project(points[0]).unwrap();
        let north = projection.project(points[1]).unwrap();
        assert!((south.0 - VIEWBOX_WIDTH / 2.0).abs() < 0.001);
        assert!((north.0 - VIEWBOX_WIDTH / 2.0).abs() < 0.001);
        assert!((0.0..=VIEWBOX_HEIGHT).contains(&south.1));
        assert!((0.0..=VIEWBOX_HEIGHT).contains(&north.1));
        assert!(south.1 > north.1);
    }

    #[test]
    fn laea_uses_the_short_arc_across_the_antimeridian() {
        let points = [
            ProjectedPoint::new(179.0, 0.0).unwrap(),
            ProjectedPoint::new(-179.0, 0.0).unwrap(),
        ];
        let frame =
            Frame::from_points(&points, [Precision::Exact, Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        let first = projection.project(points[0]).unwrap();
        let second = projection.project(points[1]).unwrap();
        assert!(first.0 < VIEWBOX_WIDTH / 2.0);
        assert!(second.0 > VIEWBOX_WIDTH / 2.0);
        assert!((first.0 + second.0 - VIEWBOX_WIDTH).abs() < 0.001);
    }

    #[test]
    fn local_projection_rejects_points_beyond_the_polar_cap_and_clip_hemisphere() {
        let center = ProjectedPoint::new(0.0, 0.0).unwrap();
        let frame = Frame::from_points(&[center], [Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        assert!(projection
            .project(ProjectedPoint::new(0.0, 85.0).unwrap())
            .is_some());
        assert!(projection
            .project(ProjectedPoint::new(0.0, 85.1).unwrap())
            .is_none());
        assert!(projection
            .project(ProjectedPoint::new(90.0, 0.0).unwrap())
            .is_some());
        assert!(projection
            .project(ProjectedPoint::new(90.001, 0.0).unwrap())
            .is_none());
    }

    #[test]
    fn project_returns_unclamped_points_and_project_part_clips_segments() {
        let center = ProjectedPoint::new(0.0, 0.0).unwrap();
        let frame = Frame::from_points(&[center], [Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        let far = projection
            .project(ProjectedPoint::new(80.0, 0.0).unwrap())
            .unwrap();
        assert!(far.0 > CLIP_MAX_X);
        let paths = projection.project_part(&[(-80, 0), (80, 0)], 1);
        assert_eq!(paths.len(), 1);
        assert!(paths[0].iter().all(|point| {
            (CLIP_MIN_X..=CLIP_MAX_X).contains(&point.0)
                && (CLIP_MIN_Y..=CLIP_MAX_Y).contains(&point.1)
        }));
        assert!(paths[0][0].0 <= CLIP_MIN_X + 0.001);
        assert!(paths[0][1].0 >= CLIP_MAX_X - 0.001);
    }

    #[test]
    fn project_part_clips_both_hemisphere_and_polar_crossings() {
        let center = ProjectedPoint::new(0.0, 0.0).unwrap();
        let frame = Frame::from_points(&[center], [Precision::Exact].into_iter()).unwrap();
        let projection = Projection::new(&frame);
        let hemisphere = projection.project_part(&[(-100, 0), (-1, 0)], 1);
        assert_eq!(hemisphere.len(), 1);
        assert!(hemisphere[0]
            .iter()
            .all(|point| point.0.is_finite() && point.1.is_finite()));
        let polar = projection.project_part(&[(0, -90), (0, 90)], 1);
        assert_eq!(polar.len(), 1);
        assert!(polar[0]
            .iter()
            .all(|point| point.0.is_finite() && point.1.is_finite()));
    }

    #[test]
    fn antimeridian_split_handles_both_travel_directions() {
        let eastward = split_antimeridian(&[
            ProjectedPoint::new(179.0, 0.0).unwrap(),
            ProjectedPoint::new(-179.0, 1.0).unwrap(),
        ]);
        assert_eq!(eastward.len(), 2);
        assert_eq!(eastward[0].last().unwrap().longitude, 180.0);
        assert_eq!(eastward[1].first().unwrap().longitude, -180.0);
        let westward = split_antimeridian(&[
            ProjectedPoint::new(-179.0, 0.0).unwrap(),
            ProjectedPoint::new(179.0, 1.0).unwrap(),
        ]);
        assert_eq!(westward.len(), 2);
        assert_eq!(westward[0].last().unwrap().longitude, -180.0);
        assert_eq!(westward[1].first().unwrap().longitude, 180.0);
    }

    #[test]
    fn tile_rows_follow_the_generator_south_to_north_index() {
        assert_eq!(tile_y(-90.0), 0);
        assert_eq!(tile_y(-80.0), 1);
        assert_eq!(tile_y(0.0), 9);
        assert_eq!(tile_y(80.0), 17);
        assert_eq!(tile_y(90.0), 17);
    }

    #[test]
    fn dateline_frame_selects_tiles_on_both_longitude_edges() {
        let pack = super::super::embedded().unwrap();
        let points = [
            ProjectedPoint::new(179.0, 0.0).unwrap(),
            ProjectedPoint::new(-179.0, 0.0).unwrap(),
        ];
        let frame =
            Frame::from_points(&points, [Precision::Exact, Precision::Exact].into_iter()).unwrap();
        let selection = TileSelection::for_frame(&pack, &frame);
        assert_eq!(selection.tier, 1);
        assert!(selection.tiles.iter().any(|(x, _)| *x == 0));
        assert!(selection.tiles.iter().any(|(x, _)| *x == 35));
        assert!(selection.tiles.iter().all(|(x, _)| *x == 0 || *x == 35));
    }

    #[test]
    fn world_frames_select_the_world_tier_without_fine_tiles() {
        let pack = super::super::embedded().unwrap();
        let points = [ProjectedPoint::new(0.0, 85.1).unwrap()];
        let frame = Frame::from_points(&points, [Precision::Exact].into_iter()).unwrap();
        let selection = TileSelection::for_frame(&pack, &frame);
        assert_eq!(selection.tier, 0);
        assert!(selection.tiles.is_empty());
        assert_eq!(
            selection.features(&pack).len(),
            pack.tiers[0].feature_count as usize
        );
    }

    #[test]
    fn tile_references_are_global_across_layer_boundaries() {
        let pack = super::super::embedded().unwrap();
        let first_layer_count = pack.tiers[1].layers[0].feature_count;
        let tile = pack
            .tiles
            .iter()
            .find(|tile| tile.features.iter().any(|id| *id >= first_layer_count))
            .unwrap();
        let selection = TileSelection {
            tier: 1,
            tiles: vec![(tile.x, tile.y)],
        };
        let actual = selection.features(&pack);
        let mut offset = 0u32;
        for layer in &pack.tiers[1].layers {
            for (index, _) in layer.features.iter().enumerate() {
                if tile.features.contains(&(offset + index as u32)) {
                    assert!(actual.iter().any(|(id, feature)| *id == layer.id
                        && std::ptr::eq(*feature, &layer.features[index])));
                }
            }
            offset += layer.feature_count;
        }
    }

    #[test]
    fn polar_points_select_world_tier() {
        let points = [ProjectedPoint::new(0.0, 85.1).unwrap()];
        let frame = Frame::from_points(&points, [Precision::Exact].into_iter()).unwrap();
        assert_eq!(frame.tier, FrameTier::World);
    }
}
