use std::collections::HashSet;
use std::sync::Arc;

use moss_core::terms::{term_fold, term_folder_key};

use super::{Frame, FrameTier, Pack, ProjectedPoint, Projection};
use crate::vault::places::Precision;

/// The immutable inputs shared by every map rendered during one build.
/// Decoding is deliberately outside page rendering: a page can borrow this
/// context without reopening the checked-in pack or the gazetteer.
#[derive(Debug, Clone)]
pub struct PlaceMapContext {
    pack: Arc<Pack>,
}

impl PlaceMapContext {
    pub fn new(pack: Pack) -> Self {
        Self {
            pack: Arc::new(pack),
        }
    }

    pub fn embedded() -> Result<Self, super::DecodeError> {
        super::embedded().map(Self::new)
    }

    pub fn pack(&self) -> &Pack {
        &self.pack
    }

    pub fn pack_fingerprint(&self) -> [u8; 32] {
        self.pack.header.manifest_sha256
    }

    /// Resolve declared `location:` values in declaration order. A missing
    /// coordinate intentionally remains in the result so the caller can keep
    /// the linked place line while omitting only the map target.
    pub fn resolve_locations(
        &self,
        namespace: &str,
        gazetteer: &crate::vault::places::Gazetteer,
        names: &[String],
    ) -> PlaceMapTarget {
        let mut seen = HashSet::new();
        let mut places = Vec::new();
        for name in names {
            let Some((display, record)) = find_record(gazetteer, name) else {
                continue;
            };
            let key = term_folder_key(namespace, display);
            if !seen.insert(key.clone()) {
                continue;
            }
            places.push(ResolvedPlace::from_record(key, display.clone(), record));
        }
        PlaceMapTarget::from_places(places)
    }

    /// Resolve a synthetic parent page from its coordinate-bearing
    /// descendants. It carries no invented point: the frame is an aggregate
    /// halo, and its marker group intentionally remains empty. Descendant
    /// markers would falsely imply that the parent itself has a location.
    pub fn resolve_aggregate(
        &self,
        namespace: &str,
        gazetteer: &crate::vault::places::Gazetteer,
        parent_name: &str,
        descendants: &[String],
    ) -> PlaceMapTarget {
        let mut target = self.resolve_locations(namespace, gazetteer, descendants);
        target.aggregate_name = Some(parent_name.trim().to_string());
        target
            .places
            .iter_mut()
            .for_each(|place| place.aggregate_member = true);
        target
    }

    pub fn projection(&self, target: &PlaceMapTarget) -> Option<Projection> {
        target.frame.as_ref().map(Projection::new)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPlace {
    pub key: String,
    pub display: String,
    pub longitude: Option<f64>,
    pub latitude: Option<f64>,
    pub precision: Precision,
    pub aggregate_member: bool,
}

impl ResolvedPlace {
    fn from_record(
        key: String,
        display: String,
        record: &crate::vault::places::PlaceRecord,
    ) -> Self {
        let (latitude, longitude) = record
            .coords
            .and_then(|(lat, lon)| {
                ProjectedPoint::new(lon, lat)
                    .map(|point| (Some(point.latitude), Some(point.longitude)))
            })
            .unwrap_or((None, None));
        Self {
            key,
            display,
            longitude,
            latitude,
            precision: record.precision,
            aggregate_member: false,
        }
    }

    pub fn point(&self) -> Option<ProjectedPoint> {
        match (self.longitude, self.latitude) {
            (Some(longitude), Some(latitude)) => ProjectedPoint::new(longitude, latitude),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaceMapTarget {
    pub places: Vec<ResolvedPlace>,
    pub frame: Option<Frame>,
    pub aggregate_name: Option<String>,
}

impl PlaceMapTarget {
    fn from_places(places: Vec<ResolvedPlace>) -> Self {
        let valid: Vec<ProjectedPoint> = places.iter().filter_map(ResolvedPlace::point).collect();
        let floors = places
            .iter()
            .filter_map(|place| place.point().map(|_| place.precision));
        let frame = Frame::from_points(&valid, floors);
        Self {
            places,
            frame,
            aggregate_name: None,
        }
    }

    pub fn has_coordinates(&self) -> bool {
        self.places.iter().any(|place| place.point().is_some())
    }

    pub fn tier(&self) -> FrameTier {
        self.frame
            .as_ref()
            .map_or(FrameTier::World, |frame| frame.tier)
    }

    pub fn marker_places(&self) -> impl Iterator<Item = &ResolvedPlace> {
        self.places
            .iter()
            .filter(|place| !place.aggregate_member && place.point().is_some())
    }
}

fn find_record<'a>(
    gazetteer: &'a crate::vault::places::Gazetteer,
    name: &str,
) -> Option<(&'a String, &'a crate::vault::places::PlaceRecord)> {
    let folded = term_fold(name);
    gazetteer
        .iter()
        .find(|(display, _)| term_fold(display) == folded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gazetteer() -> crate::vault::places::Gazetteer {
        let table: toml::value::Table = toml::from_str(
            "[\"Harbor\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\n\n[\"Harbor East\"]\nlat = 35.1\nlng = 135.1\nprecision = \"region\"\nparent = \"Harbor\"\n",
        ).unwrap();
        crate::vault::places::parse_gazetteer(&table)
    }

    #[test]
    fn locations_dedupe_canonical_keys_and_keep_first_spelling() {
        let context = PlaceMapContext::new(super::super::embedded().unwrap());
        let target = context.resolve_locations(
            "places",
            &gazetteer(),
            &[" harbor ".into(), "HARBOR".into(), "Harbor East".into()],
        );
        assert_eq!(target.places.len(), 2);
        assert_eq!(target.places[0].display, "Harbor");
        assert_eq!(target.places[0].key, "places/harbor");
    }

    #[test]
    fn missing_coordinates_stay_in_the_target_but_do_not_make_a_frame() {
        let table: toml::value::Table =
            toml::from_str("[\"Unknown\"]\nprecision = \"exact\"\n").unwrap();
        let context = PlaceMapContext::new(super::super::embedded().unwrap());
        let target = context.resolve_locations(
            "places",
            &crate::vault::places::parse_gazetteer(&table),
            &["Unknown".into()],
        );
        assert_eq!(target.places.len(), 1);
        assert!(!target.has_coordinates());
        assert!(target.frame.is_none());
    }

    #[test]
    fn aggregate_target_has_a_frame_but_never_a_centroid_marker() {
        let context = PlaceMapContext::new(super::super::embedded().unwrap());
        let target =
            context.resolve_aggregate("places", &gazetteer(), "Harbor", &["Harbor East".into()]);
        assert!(target.frame.is_some());
        assert_eq!(target.marker_places().count(), 0);
        assert_eq!(target.aggregate_name.as_deref(), Some("Harbor"));
        assert!(target.places.iter().all(|place| place.aggregate_member));
    }

    #[test]
    fn resolved_keys_use_the_declared_namespace() {
        let context = PlaceMapContext::new(super::super::embedded().unwrap());
        let target = context.resolve_locations("locations", &gazetteer(), &["Harbor".into()]);
        assert_eq!(target.places[0].key, "locations/harbor");
    }

    #[test]
    fn invalid_coordinates_are_not_retained_as_finite_place_values() {
        let table: toml::value::Table =
            toml::from_str("[\"Invalid\"]\nlat = 91.0\nlng = nan\nprecision = \"exact\"\n")
                .unwrap();
        let context = PlaceMapContext::new(super::super::embedded().unwrap());
        let target = context.resolve_locations(
            "places",
            &crate::vault::places::parse_gazetteer(&table),
            &["Invalid".into()],
        );
        assert_eq!(target.places[0].longitude, None);
        assert_eq!(target.places[0].latitude, None);
        assert!(!target.has_coordinates());
    }
}
