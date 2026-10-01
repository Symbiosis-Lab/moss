use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use moss_core::terms::{term_fold, term_folder_key};

use super::{precision_rank, Frame, FrameTier, Pack, ProjectedPoint, Projection};
use crate::vault::places::Precision;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocatorPlacement { None, AlignRight }

impl LocatorPlacement {
    pub fn from_config(value: Option<&str>) -> Self {
        match value {
            None | Some("") | Some("none") => Self::None,
            Some("align-right") => Self::AlignRight,
            Some(other) => {
                crate::build::cli_output::log_warn_problem!(
                    "[site].locator must be 'align-right' or 'none', not '{other}'; omitting locator maps"
                );
                Self::None
            }
        }
    }
}

/// Fully-resolved inputs shared by page rendering and map-style embeds.
#[derive(Debug, Clone)]
pub struct PlaceMapRenderContext {
    maps: PlaceMapContext,
    gazetteer: Arc<crate::vault::places::Gazetteer>,
    namespace: String,
    locator: LocatorPlacement,
    /// The place kind's already cycle-repaired `parents` map — the same one
    /// `build::terms::places::attach_parents`/`break_cycles` finishes
    /// building before this context is ever constructed (see the pipeline's
    /// call order), and the one the terms layer's own ancestor walks read.
    /// `lies_under` walks this instead of re-deriving its own chain from
    /// the raw gazetteer, so a parent-chain cycle is fixed in exactly one
    /// place rather than risking a second, differently-capped repair here.
    parents: BTreeMap<String, String>,
    /// `TermKind::explorer_enabled()` for this build's place-typed kind,
    /// captured once at construction (`pipeline.rs`) from the same `kinds`
    /// table `namespace`/`parents` already came from. `render_term_map`
    /// reads this rather than re-reading config, so the injection predicate
    /// (`features::should_inject_places_explorer`) and the markup handshake
    /// below can never resolve the default differently.
    explorer: bool,
    /// The `_moss/map.<hash>/` directory name for this build's world map and
    /// regional tiles (`emit::place_map_assets::assets_hash`) — a pure
    /// function of the pack and the gazetteer, so unlike
    /// `explorer_places_hash` below it is known at construction, before any
    /// document is parsed. Empty only when nothing has set it (no
    /// `with_map_assets_hash` call), which `with_explorer_handshake` reads
    /// as "not ready" the same way it reads a `None` places hash.
    map_assets_hash: String,
    /// `places.<hash>.json`'s content hash, set once `derive_terms` has
    /// produced this build's finished document set
    /// (`render::blocking::generate_blocking_content_for_build`, right after
    /// the pass that fills `ParsedDocument::location`/`byline` for the last
    /// time) — `None` until then. `render_term_map` only emits the
    /// `data-moss-places-explorer` handshake once this is `Some`, so a
    /// namespace root rendered before the hash exists (there is no such
    /// call site today, but nothing stops a future one) degrades to the
    /// plain static figure instead of baking in a hash the build might not
    /// actually write.
    explorer_places_hash: Option<String>,
}

impl PlaceMapRenderContext {
    pub fn new(
        maps: PlaceMapContext,
        gazetteer: crate::vault::places::Gazetteer,
        namespace: String,
        locator: LocatorPlacement,
        parents: BTreeMap<String, String>,
    ) -> Self {
        Self {
            maps,
            gazetteer: Arc::new(gazetteer),
            namespace,
            locator,
            parents,
            explorer: true,
            map_assets_hash: String::new(),
            explorer_places_hash: None,
        }
    }

    /// `TermKind::explorer_enabled()` for the place-typed kind this context
    /// was built from. Chained onto `new()` rather than widened into it so
    /// every existing call site (several in this crate's own tests) keeps
    /// compiling unchanged.
    pub fn with_explorer(mut self, explorer: bool) -> Self {
        self.explorer = explorer;
        self
    }

    /// This build's `_moss/map.<hash>/` directory name
    /// (`emit::place_map_assets::assets_hash`) — set once, at construction,
    /// since unlike the places hash it needs nothing documents haven't
    /// provided yet.
    pub fn with_map_assets_hash(mut self, hash: String) -> Self {
        self.map_assets_hash = hash;
        self
    }

    /// This build's `_moss/map.<hash>/` directory name, read back by
    /// `emit::place_map_labels` so `labels.json` lands beside the SVGs
    /// `emit::place_map_assets` already writes there, under the identical
    /// hash, without a second `assets_hash` computation at that call site.
    /// Empty exactly when [`Self::with_map_assets_hash`] was never called —
    /// the same "not ready" signal `with_explorer_handshake` reads.
    pub(crate) fn map_assets_hash(&self) -> &str {
        &self.map_assets_hash
    }

    /// `places.<hash>.json`'s content hash, set once the caller's own
    /// `place_map::places_data::emit_places_data` call (over the SAME
    /// finished document set `places_data::emit` serializes to disk) has
    /// produced it. See [`Self::explorer_places_hash`]'s own doc for why
    /// this is a late `with_*` rather than a `new()` parameter.
    pub fn with_explorer_places_hash(mut self, hash: String) -> Self {
        self.explorer_places_hash = Some(hash);
        self
    }

    pub fn is_place_key(&self, key: &str) -> bool {
        key == self.namespace || key.starts_with(&format!("{}/", self.namespace))
    }

    /// The bundled map pack, gazetteer, place-typed namespace key, and
    /// cycle-repaired parent map this context already bundles — read by
    /// `place_map::places_data`, the one other caller outside this module
    /// that needs these fields, rather than threading each one through its
    /// own parameter the way the pipeline used to hand-capture them.
    pub(crate) fn maps(&self) -> &PlaceMapContext {
        &self.maps
    }

    pub(crate) fn gazetteer(&self) -> &crate::vault::places::Gazetteer {
        &self.gazetteer
    }

    pub(crate) fn namespace(&self) -> &str {
        &self.namespace
    }

    pub(crate) fn parents(&self) -> &BTreeMap<String, String> {
        &self.parents
    }

    pub fn render_locator(&self, names: &[String], page_path: &str, ordinal: usize) -> Option<String> {
        if self.locator == LocatorPlacement::None { return None; }
        let target = self.maps.resolve_locations(&self.namespace, &self.gazetteer, names);
        if !target.has_coordinates() { return None; }
        let first = target.places.iter().find(|place| place.point().is_some())?;
        // The profile (how much detail the locator carries) has to reflect
        // the WHOLE list's precision, not whichever place happens to be
        // declared first: emit_svg already picks the coarsest precision
        // across every declared place for the full/aggregate map, for the
        // same privacy reason a region-precision place anywhere in the list
        // demands. A locator that instead kept the first place's precision
        // (e.g. a [city, city, region, city] list, first = city) requested
        // full relief/seafloor detail for a frame wide enough to hold a
        // region, which is how a real multi-place article blew past
        // LOCATOR_RAW_SAFETY_LIMIT and lost its locator with no warning.
        let precision = target
            .places
            .iter()
            .map(|place| place.precision)
            .max_by_key(precision_rank)
            .unwrap_or(Precision::Country);
        let options = super::SvgMapOptions::new(page_path, ordinal, &first.display, precision);
        match super::emit_locator(&self.maps, &target, options) {
            Ok(locator) => {
                let svg = locator.svg;
                Some(format!(r#"<div class="moss-place-locator moss-align-right">{svg}</div>"#))
            }
            Err(error) => {
                crate::build::cli_output::log_warn_problem!(
                    "{page_path}: locator dropped — {} bytes exceeds the {} byte safety ceiling",
                    error.raw_bytes,
                    super::LOCATOR_RAW_SAFETY_LIMIT
                );
                None
            }
        }
    }

    pub fn render_term_map<'a>(
        &self,
        key: &str,
        members: impl IntoIterator<Item = &'a crate::build::types::ParsedDocument>,
        page_path: &str,
        ordinal: usize,
    ) -> Option<String> {
        if !self.is_place_key(key) { return None; }
        let members = members.into_iter();
        let own_name = self.gazetteer.iter().find_map(|(display, _)| {
            (term_folder_key(&self.namespace, display) == key).then(|| display.clone())
        });
        let target = if let Some(name) = own_name.as_ref() {
            let direct = self.maps.resolve_locations(&self.namespace, &self.gazetteer, std::slice::from_ref(name));
            if direct.has_coordinates() { direct } else { self.aggregate(key, name, members) }
        } else {
            let label = key.strip_prefix(&format!("{}/", self.namespace)).unwrap_or(&self.namespace);
            self.aggregate(key, label, members)
        };
        let svg = target.has_coordinates().then(|| super::emit_svg(&self.maps, &target, page_path, ordinal))?;
        Some(self.with_explorer_handshake(key, svg))
    }

    /// Splice the places-explorer handshake attributes onto the figure's
    /// opening tag when `key` is the bare namespace root and every piece the
    /// runtime needs is ready: the explorer is on, and both hashes have been
    /// set. `emit_svg` is the sole writer of `<figure class="moss-place-map"
    /// ...>` (its own module doc), and this is the only other place that
    /// touches that opening tag — never a second spot that could disagree
    /// about the URLs. A sub-place's own map (`key` holds a `/`) is left
    /// untouched: the explorer scopes to the root, so a claimed place's
    /// embed stays exactly the no-JavaScript figure it always was.
    fn with_explorer_handshake(&self, key: &str, svg: String) -> String {
        if key != self.namespace || !self.explorer || self.map_assets_hash.is_empty() {
            return svg;
        }
        let Some(places_hash) = self.explorer_places_hash.as_deref() else { return svg };
        let (Ok(world), Ok(tiles)) = (
            crate::build::served_path::ServedPath::for_place_map_asset(&self.map_assets_hash, "world.svg"),
            crate::build::served_path::ServedPath::for_place_map_asset(&self.map_assets_hash, "tiles.json"),
        ) else {
            return svg;
        };
        let places = crate::build::served_path::ServedPath::for_places_data_hashed(places_hash);
        svg.replacen(
            "<figure class=\"moss-place-map\"",
            &format!(
                "<figure class=\"moss-place-map\" data-moss-places-explorer data-world=\"{}\" data-tiles=\"{}\" data-places=\"{}\" data-scope=\"{}\"",
                world.to_relative_url(),
                tiles.to_relative_url(),
                places.to_relative_url(),
                self.namespace,
            ),
            1,
        )
    }

    fn aggregate<'a>(
        &self,
        key: &str,
        label: &str,
        members: impl IntoIterator<Item = &'a crate::build::types::ParsedDocument>,
    ) -> PlaceMapTarget {
        let mut names = Vec::new();
        for doc in members {
            if key == self.namespace || doc.also_in.as_ref().is_some_and(|keys| keys.iter().any(|candidate| candidate == key)) {
                // A page listed under a parent may also name places
                // elsewhere; the parent's map marks only its own.
                names.extend(doc.location.iter().filter(|name| self.lies_under(name, key)).cloned());
            }
        }
        self.maps.resolve_aggregate(&self.namespace, &self.gazetteer, label, &names)
    }

    /// Whether a place is `key`, or under it through `self.parents` — the
    /// terms layer's own already cycle-repaired hierarchy, not a second
    /// walk of the raw gazetteer. A raw walk here used to cap itself at
    /// eight hops with no cycle detection of its own, so a gazetteer
    /// parent-chain cycle could make it give up on a place the terms
    /// layer's ancestor walk (`build/terms.rs` pass 2, over this same
    /// `parents` map) still reaches and counts as a member. Walking the
    /// repaired map instead fixes that by construction: there is only one
    /// cycle repair, and both walks read its result. The namespace root
    /// holds every place.
    fn lies_under(&self, name: &str, key: &str) -> bool {
        if key == self.namespace {
            return true;
        }
        let Some((display, _)) = find_record(&self.gazetteer, name) else {
            return false;
        };
        let mut current = term_folder_key(&self.namespace, display);
        let mut seen = HashSet::new();
        seen.insert(current.clone());
        loop {
            if current == key {
                return true;
            }
            let Some(parent_display) = self.parents.get(&current) else {
                return false;
            };
            current = term_folder_key(&self.namespace, parent_display);
            // `parents` is already a fixed point (break_cycles cut every
            // cycle when it was built); `seen` is a defensive backstop
            // against a hand-built map reaching this some other way, the
            // same posture the terms layer's own walks over this map take.
            if !seen.insert(current.clone()) {
                return false;
            }
        }
    }
}

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

    /// The embedded pack's raw schema-4 label-payload bytes -- a stable
    /// fingerprint of "the label set, or the generator that produced it,
    /// changed" for a caller that needs one (`emit::place_map_assets::
    /// assets_hash` folds this into the `_moss/map.<hash>/` directory name
    /// so `labels.json`, served from that same directory, is named
    /// correctly whenever either changes). Unlike [`Self::pack_fingerprint`],
    /// this is NOT derived from the source manifest's digest: a generator
    /// change that encodes different label bytes from the SAME pinned
    /// sources would leave the manifest digest untouched, so these raw
    /// bytes are hashed directly instead.
    pub fn labels_bytes(&self) -> &[u8] {
        &self.pack.labels_bytes
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

    /// Resolve a listing map (the namespace root, or a parent place with no
    /// coordinates of its own) from the places its member pages name. The
    /// frame holds them all and each is marked by its own precision;
    /// nothing marks the parent, which has no location to mark.
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
        self.places.iter().filter(|place| place.point().is_some())
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

    /// A listing map marks the places its pages name, each by its own
    /// precision, and never the parent it lists them under.
    #[test]
    fn aggregate_target_marks_its_members_not_the_parent() {
        let context = PlaceMapContext::new(super::super::embedded().unwrap());
        let target = context.resolve_aggregate(
            "places",
            &gazetteer(),
            "places",
            &["Harbor".into(), "Harbor East".into()],
        );
        assert!(target.frame.is_some());
        let marked: Vec<(&str, Precision)> = target
            .marker_places()
            .map(|place| (place.display.as_str(), place.precision))
            .collect();
        assert_eq!(marked, [("Harbor", Precision::City), ("Harbor East", Precision::Region)]);
        assert_eq!(target.aggregate_name.as_deref(), Some("places"));
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

    #[test]
    fn locator_is_opt_in_and_uses_the_sparse_profile() {
        let maps = PlaceMapContext::new(super::super::embedded().unwrap());
        let off = PlaceMapRenderContext::new(
            maps.clone(), gazetteer(), "places".into(), LocatorPlacement::None, BTreeMap::new(),
        );
        assert!(off.render_locator(&["Harbor".into()], "story/index.html", 0).is_none());

        let on = PlaceMapRenderContext::new(
            maps, gazetteer(), "places".into(), LocatorPlacement::AlignRight, BTreeMap::new(),
        );
        let html = on.render_locator(&["Harbor".into()], "story/index.html", 0).unwrap();
        assert!(html.contains("moss-place-locator moss-align-right"));
        assert!(html.contains("data-map-locator-profile=\"exact-city\""));
    }

    /// The reproduction from the field: a location list that mixes
    /// precisions (here just [city, region] — the minimal shape that still
    /// triggers it) must size the locator to the COARSEST place in the
    /// list, not whichever one was declared first. `gazetteer()`'s "Harbor"
    /// (city) / "Harbor East" (region) pair already gives us that mix.
    /// Before the fix this kept "exact-city" (Harbor's own precision, since
    /// it happened to resolve first) for a frame wide enough to also hold
    /// Harbor East — full relief/seafloor detail over a region-sized frame,
    /// which is how a real multi-place article's locator blew past
    /// LOCATOR_RAW_SAFETY_LIMIT.
    #[test]
    fn multi_place_locator_uses_the_coarsest_precision_not_declaration_order() {
        let maps = PlaceMapContext::new(super::super::embedded().unwrap());
        let context = PlaceMapRenderContext::new(
            maps, gazetteer(), "places".into(), LocatorPlacement::AlignRight, BTreeMap::new(),
        );
        let html = context
            .render_locator(&["Harbor".into(), "Harbor East".into()], "story/index.html", 0)
            .expect("a mixed-precision list must still produce a locator");
        assert!(
            html.contains("data-map-locator-profile=\"region\""),
            "must use Harbor East's (region) precision, not Harbor's (city, first-declared): {html}"
        );
        assert!(!html.contains("data-map-locator-profile=\"exact-city\""));
    }

    /// The safety ceiling stops an oversized locator from shipping, and
    /// dropping it must not be silent: the page has to name itself in a
    /// warning an agent or a --strict build can see. Real geometry no longer
    /// reliably clears the ceiling, because every path is simplified in
    /// screen space and a map's bytes are bounded by its pixels, so the
    /// fixture overflows it with the place's name, which the figure repeats
    /// in its accessible label.
    #[test]
    fn oversized_locator_is_dropped_with_a_warning_not_silently() {
        const LONG_NAME: usize = super::super::LOCATOR_RAW_SAFETY_LIMIT;
        let table: toml::value::Table = toml::from_str(
            &format!("[\"{}\"]\nlat = 62.0\nlng = 6.0\nprecision = \"city\"\n", "F".repeat(LONG_NAME)),
        )
        .unwrap();
        let maps = PlaceMapContext::new(super::super::embedded().unwrap());
        let context = PlaceMapRenderContext::new(
            maps,
            crate::vault::places::parse_gazetteer(&table),
            "places".into(),
            LocatorPlacement::AlignRight,
            BTreeMap::new(),
        );
        crate::build::cli_output::take_cli_problems(); // count only what the render itself reports
        let result =
            context.render_locator(&["F".repeat(LONG_NAME)], "story/oversized.html", 0);
        assert!(result.is_none(), "an oversized locator must be dropped, not shipped");
        assert!(
            crate::build::cli_output::take_cli_problems() >= 1,
            "dropping an oversized locator must warn (and count as a --strict problem), not fail silently"
        );
    }

    /// A gazetteer parent chain that loops (P1..P8 cycle back to P1) after
    /// a short prefix: `attach_parents`/`break_cycles` cuts exactly one
    /// link of it before this context is ever built, the same repair the
    /// terms layer's own ancestor walk (`build/terms.rs` pass 2) reads —
    /// so a document declaring "Leaf" as its location is, by the terms
    /// layer's own count, a member all the way up to P8. A raw re-walk of
    /// the gazetteer here, still cyclic and capped at eight hops, would
    /// give up before reaching P8 and silently drop it from that listing.
    #[test]
    fn lies_under_reaches_past_a_repaired_cycle_the_terms_layer_also_counts() {
        let table: toml::value::Table = toml::from_str(
            "[\"Leaf\"]\nlat=1.0\nlng=1.0\nprecision=\"city\"\nparent=\"Mid\"\n\n\
             [\"Mid\"]\nlat=2.0\nlng=2.0\nprecision=\"region\"\nparent=\"P1\"\n\n\
             [\"P1\"]\nlat=3.0\nlng=3.0\nprecision=\"region\"\nparent=\"P2\"\n\n\
             [\"P2\"]\nlat=4.0\nlng=4.0\nprecision=\"region\"\nparent=\"P3\"\n\n\
             [\"P3\"]\nlat=5.0\nlng=5.0\nprecision=\"region\"\nparent=\"P4\"\n\n\
             [\"P4\"]\nlat=6.0\nlng=6.0\nprecision=\"region\"\nparent=\"P5\"\n\n\
             [\"P5\"]\nlat=7.0\nlng=7.0\nprecision=\"region\"\nparent=\"P6\"\n\n\
             [\"P6\"]\nlat=8.0\nlng=8.0\nprecision=\"region\"\nparent=\"P7\"\n\n\
             [\"P7\"]\nlat=9.0\nlng=9.0\nprecision=\"region\"\nparent=\"P8\"\n\n\
             [\"P8\"]\nlat=10.0\nlng=10.0\nprecision=\"country\"\nparent=\"P1\"\n",
        )
        .unwrap();
        let gaz = crate::vault::places::parse_gazetteer(&table);
        let mut kinds = vec![crate::build::terms::TermKind {
            key: "places".to_string(),
            fields: vec!["location".to_string()],
            title: "Places".to_string(),
            is_place: true,
            parents: Default::default(), explorer: None,
        }];
        crate::build::terms::places::attach_parents(&mut kinds, &gaz);
        let maps = PlaceMapContext::new(super::super::embedded().unwrap());
        let context = PlaceMapRenderContext::new(
            maps, gaz, "places".into(), LocatorPlacement::None, kinds[0].parents.clone(),
        );
        assert!(
            context.lies_under("Leaf", "places/p8"),
            "the repaired chain still reaches P8 past the cut cycle edge"
        );
    }

    fn located_doc(name: &str) -> crate::build::types::ParsedDocument {
        crate::build::types::ParsedDocument {
            location: vec![name.to_string()],
            ..Default::default()
        }
    }

    fn ready_root_context() -> PlaceMapRenderContext {
        let maps = PlaceMapContext::new(super::super::embedded().unwrap());
        PlaceMapRenderContext::new(maps, gazetteer(), "places".into(), LocatorPlacement::None, BTreeMap::new())
            .with_explorer(true)
            .with_map_assets_hash("abc123".into())
            .with_explorer_places_hash("def456".into())
    }

    #[test]
    fn root_map_carries_the_explorer_handshake_when_everything_is_ready() {
        let context = ready_root_context();
        let docs = [located_doc("Harbor")];
        let html = context.render_term_map("places", docs.iter(), "places/index.html", 0).unwrap();
        assert!(html.contains("data-moss-places-explorer"), "{html:.200}");
        assert!(html.contains("data-world=\"/_moss/map.abc123/world.svg\""), "{html:.200}");
        assert!(html.contains("data-tiles=\"/_moss/map.abc123/tiles.json\""), "{html:.200}");
        assert!(html.contains("data-places=\"/_moss/places.def456.json\""), "{html:.200}");
        assert!(html.contains("data-scope=\"places\""), "{html:.200}");
    }

    #[test]
    fn sub_place_map_never_carries_the_handshake() {
        let context = ready_root_context();
        let docs = [located_doc("Harbor East")];
        let html = context
            .render_term_map("places/harbor", docs.iter(), "places/harbor/index.html", 0)
            .unwrap();
        assert!(!html.contains("data-moss-places-explorer"), "{html:.200}");
    }

    #[test]
    fn explorer_off_leaves_the_root_map_untouched() {
        let context = ready_root_context().with_explorer(false);
        let docs = [located_doc("Harbor")];
        let html = context.render_term_map("places", docs.iter(), "places/index.html", 0).unwrap();
        assert!(!html.contains("data-moss-places-explorer"), "{html:.200}");
    }

    #[test]
    fn missing_places_hash_leaves_the_root_map_untouched() {
        let maps = PlaceMapContext::new(super::super::embedded().unwrap());
        let context = PlaceMapRenderContext::new(maps, gazetteer(), "places".into(), LocatorPlacement::None, BTreeMap::new())
            .with_explorer(true)
            .with_map_assets_hash("abc123".into());
        let docs = [located_doc("Harbor")];
        let html = context.render_term_map("places", docs.iter(), "places/index.html", 0).unwrap();
        assert!(!html.contains("data-moss-places-explorer"), "{html:.200}");
    }
}
