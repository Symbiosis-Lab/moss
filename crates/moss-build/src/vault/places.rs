//! `.moss/places.toml`: the hand-edited gazetteer a `location:` frontmatter
//! value looks a place name up in. Read-only this slice — the write half
//! (`infra::toml_rewrite::apply_changes` / `vault::config::write_managed_toml`,
//! reused the way `config.toml` already is) has no consumer until the places
//! editor (slice 5) and is not built here, per moss's own "every module must
//! have a consumer" rule.

use std::path::Path;

/// How precisely a gazetteer entry may be shown — the privacy control, not a
/// display preference. A missing or unrecognized value coarsens to
/// `Country`, the widest ring, never defaults to `Exact`: the fail-safe
/// direction for a malformed `.moss/places.toml` has to be the coarse one, or
/// a typo becomes a privacy leak instead of a build warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Precision {
    Exact,
    City,
    Region,
    Country,
}

/// One quoted-key table in `.moss/places.toml`. Coordinates live only here —
/// a claimed place page (`place_page: true`) supplies prose and a cover,
/// never a coordinate of its own.
#[derive(Debug, Clone)]
pub struct PlaceRecord {
    /// `None` when `lat`/`lng` were missing or non-numeric — the entry is
    /// still kept, with a diagnostic, so its `parent` and `precision` still
    /// drive the breadcrumb and roll-up. A map generator is the only reader
    /// that would need to skip a `None` here, and none exists this slice.
    pub coords: Option<(f64, f64)>,
    pub precision: Precision,
    /// Another place's display name, exactly as written in
    /// `.moss/places.toml` — resolved into a pseudo-folder key by
    /// `build::terms::places::attach_parents`, never here; this struct is a
    /// plain parse of one table.
    pub parent: Option<String>,
    /// An explicit URL segment for this place's page, replacing the one its
    /// name would give (`slug = "shanghai"` for `上海`). Already validated by
    /// `parse_gazetteer`: never a slug that is malformed or shared with
    /// another place.
    pub slug: Option<String>,
}

/// The parsed, validated `.moss/places.toml`. Missing the whole file is not
/// an error (a site with no gazetteer yet is normal); a malformed whole-file
/// parse is a diagnostic plus an empty gazetteer, never a build failure; an
/// individual bad entry is dropped or coarsened with a diagnostic, never a
/// crash.
#[derive(Debug, Clone, Default)]
pub struct Gazetteer {
    places: std::collections::BTreeMap<String, PlaceRecord>,
    slugs: std::collections::BTreeMap<String, String>,
}

/// The key of the place `name` in `ns`, honouring an explicit slug from
/// `slugs` (keyed by the slug the name derives, so case variants of one name
/// share it).
pub fn slugged_key(slugs: &std::collections::BTreeMap<String, String>, ns: &str, name: &str) -> String {
    let derived = moss_core::terms::term_slug(name);
    format!("{ns}/{}", slugs.get(&derived).unwrap_or(&derived))
}

impl Gazetteer {
    /// The record for a place's exact display name, if the gazetteer has one.
    pub fn get(&self, name: &str) -> Option<&PlaceRecord> {
        self.places.get(name)
    }

    /// The entry for `name` as a page may spell it: compared by
    /// `moss_core::terms::term_fold`, so case and spacing do not matter.
    /// Returns the entry's display name beside its record.
    pub fn find(&self, name: &str) -> Option<(&String, &PlaceRecord)> {
        let folded = moss_core::terms::term_fold(name);
        self.places.iter().find(|(display, _)| moss_core::terms::term_fold(display) == folded)
    }

    /// The pseudo-folder key of the place `name` in `ns`: `places/shanghai`
    /// for a row with `slug = "shanghai"`, else the key its name gives. The
    /// one rule every link, roll-up and map id goes through, so they agree.
    pub fn term_key(&self, ns: &str, name: &str) -> String {
        slugged_key(&self.slugs, ns, name)
    }

    /// Explicit slugs by the slug their place's name derives, the form
    /// `slugged_key` looks them up in.
    pub fn slug_overrides(&self) -> &std::collections::BTreeMap<String, String> {
        &self.slugs
    }

    /// Every entry, in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &PlaceRecord)> {
        self.places.iter()
    }
}

/// Every precision value `.moss/places.toml` accepts, in ladder order
/// (narrowest to widest) rather than `Precision`'s declaration order — a
/// diagnostic listing them should read as the ladder an author can pick a
/// rung from, not as an arbitrary enum dump.
const ALLOWED_PRECISIONS: &str = "exact, city, region, country";

/// The diagnostic text for an unrecognized `precision` value. Previously this
/// named only the place and the bad value, leaving an author who mistyped
/// (`"town"`) with no indication of WHERE to fix it or WHAT to fix it to —
/// the downgrade to `country` (the privacy floor) still happened, just
/// without enough in the message to tell a typo from an intentional choice.
/// Pure and separately testable so the message's content — not just that
/// some diagnostic fired — is what a test pins.
fn precision_diagnostic(place: &str, bad: &str) -> String {
    format!(
        "'.moss/places.toml': place '{place}' has an invalid precision '{bad}' (allowed: {ALLOWED_PRECISIONS}); treating it as 'country'"
    )
}

/// Turn an already-loaded `.moss/places.toml` table into a [`Gazetteer`].
/// Pure — no I/O of its own — so it is covered by unit tests with no temp
/// files; [`load_gazetteer`] is the thin I/O wrapper around it.
pub fn parse_gazetteer(table: &toml::value::Table) -> Gazetteer {
    let mut out = std::collections::BTreeMap::new();
    for (name, value) in table {
        let Some(entry) = value.as_table() else { continue };
        let lat = entry.get("lat").and_then(toml_as_f64);
        let lng = entry.get("lng").and_then(toml_as_f64);
        let coords = match (lat, lng) {
            (Some(lat), Some(lng)) => Some((lat, lng)),
            _ => {
                crate::build::cli_output::log_note!(
                    "place '{name}' in .moss/places.toml has no coordinates (lat/lng); its parent and precision still apply, but it will not appear on a map"
                );
                None
            }
        };
        let precision = match entry.get("precision").and_then(|v| v.as_str()) {
            Some("exact") => Precision::Exact,
            Some("city") => Precision::City,
            Some("region") => Precision::Region,
            Some("country") => Precision::Country,
            Some(bad) => {
                crate::build::cli_output::log_warn_problem!("{}", precision_diagnostic(name, bad));
                Precision::Country
            }
            // No `precision` key at all is not itself a mistake worth a
            // diagnostic — it still coarsens to the safe default.
            None => Precision::Country,
        };
        let parent = entry.get("parent").and_then(|v| v.as_str()).map(str::to_string);
        let slug = entry.get("slug").and_then(|v| v.as_str()).map(str::to_string);
        out.insert(name.clone(), PlaceRecord { coords, precision, parent, slug });
    }
    let slugs = check_slugs(&mut out);
    Gazetteer { places: out, slugs }
}

/// Whether `slug` is one URL segment of lowercase ASCII letters, digits and
/// single hyphens, not starting or ending with one.
fn is_valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
        && slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Drop every explicit slug that cannot be served, each with a build problem
/// that names the row, and return the survivors keyed by the slug their place's
/// name derives. A malformed slug, and a slug that is another place's address
/// (an explicit slug, or the one its name derives), would otherwise send two
/// places to one page. Both rows of a clash lose their slug, so the outcome
/// does not depend on row order.
fn check_slugs(rows: &mut std::collections::BTreeMap<String, PlaceRecord>) -> std::collections::BTreeMap<String, String> {
    for (name, record) in rows.iter_mut() {
        if let Some(bad) = record.slug.as_deref().filter(|s| !is_valid_slug(s)) {
            crate::build::cli_output::log_warn_problem!(
                "'.moss/places.toml': place '{name}' has an invalid slug '{bad}' (use lowercase letters, digits and hyphens, like 'new-york'); using the address its name gives"
            );
            record.slug = None;
        }
    }
    // Who answers to each address: every row (by its explicit slug, else its
    // derived one) and every parent named only by a `parent` link.
    let mut holders: std::collections::BTreeMap<String, Vec<(String, bool)>> = Default::default();
    for (name, record) in rows.iter() {
        let derived = moss_core::terms::term_slug(name);
        holders.entry(record.slug.clone().unwrap_or(derived)).or_default().push((name.clone(), record.slug.is_some()));
    }
    for parent in rows.values().filter_map(|r| r.parent.as_deref()) {
        if !rows.contains_key(parent) {
            holders.entry(moss_core::terms::term_slug(parent)).or_default().push((parent.to_string(), false));
        }
    }
    for (address, who) in &holders {
        // Case variants of one name derive one address on purpose; only a
        // clash that involves an explicit slug is a mistake.
        let distinct: std::collections::BTreeSet<_> = who.iter().map(|(n, _)| moss_core::terms::term_slug(n)).collect();
        if !who.iter().any(|(_, explicit)| *explicit) || distinct.len() < 2 {
            continue;
        }
        let names: Vec<String> = who.iter().map(|(n, _)| format!("'{n}'")).collect();
        crate::build::cli_output::log_warn_problem!(
            "'.moss/places.toml': places {} would all be served at the address '{address}'; their slugs are ignored, so each keeps the address its name gives",
            names.join(" and ")
        );
        for (n, explicit) in who {
            if *explicit {
                if let Some(r) = rows.get_mut(n) {
                    r.slug = None;
                }
            }
        }
    }
    rows.iter()
        .filter_map(|(name, r)| Some((moss_core::terms::term_slug(name), r.slug.clone()?)))
        .collect()
}

/// TOML stores a whole number written without a decimal point (`lat = 35`) as
/// an integer, not a float — `Value::as_float` alone would miss it.
fn toml_as_f64(v: &toml::Value) -> Option<f64> {
    v.as_float().or_else(|| v.as_integer().map(|i| i as f64))
}

/// Load and parse `.moss/places.toml`, or an empty gazetteer when the file is
/// genuinely absent. A malformed or unreadable file is one diagnostic plus an
/// empty gazetteer, never a build failure — reusing the exact absent/
/// unreadable split [`crate::vault::config::load_managed_toml`] already makes
/// for `config.toml`, rather than leaving the whole-file error case
/// unspecified.
pub fn load_gazetteer(path: &Path) -> Gazetteer {
    match crate::vault::config::load_managed_toml(path) {
        Ok(managed) => parse_gazetteer(&managed.root),
        Err(e) => {
            crate::build::cli_output::log_warn_problem!("{e}");
            Gazetteer::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(toml_str: &str) -> toml::value::Table {
        toml::from_str(toml_str).expect("valid toml in test fixture")
    }

    #[test]
    fn a_well_formed_entry_with_a_parent_parses_cleanly() {
        let gaz = parse_gazetteer(&table(
            "[\"Kyoto\"]\nlat = 35.0116\nlng = 135.7681\nprecision = \"city\"\nparent = \"Japan\"\n",
        ));
        let kyoto = gaz.get("Kyoto").expect("Kyoto entry present");
        assert_eq!(kyoto.coords, Some((35.0116, 135.7681)));
        assert_eq!(kyoto.precision, Precision::City);
        assert_eq!(kyoto.parent.as_deref(), Some("Japan"));
    }

    #[test]
    fn an_entry_missing_lng_has_no_coords_but_keeps_its_parent_with_a_diagnostic() {
        let _ = crate::build::cli_output::take_cli_problems();
        let gaz = parse_gazetteer(&table(
            "[\"Kyoto\"]\nlat = 35.0116\nprecision = \"city\"\nparent = \"Japan\"\n",
        ));
        let kyoto = gaz.get("Kyoto").expect("Kyoto entry present");
        assert_eq!(kyoto.coords, None);
        assert_eq!(kyoto.precision, Precision::City);
        assert_eq!(kyoto.parent.as_deref(), Some("Japan"));
        assert_eq!(crate::build::cli_output::take_cli_problems(), 0, "a place without coordinates is a note, not a --strict problem");
    }

    #[test]
    fn an_entry_with_an_invalid_precision_is_coarsened_to_country_with_a_diagnostic() {
        let gaz = parse_gazetteer(&table(
            "[\"Kyoto\"]\nlat = 35.0116\nlng = 135.7681\nprecision = \"neighborhood\"\n",
        ));
        let kyoto = gaz.get("Kyoto").expect("Kyoto entry present");
        assert_eq!(kyoto.precision, Precision::Country);
    }

    /// The downgrade alone (pinned above) told nobody WHERE a bad value came
    /// from or WHAT would have been accepted — a site that wrote `"town"`
    /// just silently got the privacy floor. The diagnostic text has to carry
    /// all four facts, or the gap reopens the moment a line gets trimmed.
    #[test]
    fn unknown_precision_diagnostic_names_the_file_place_value_and_allowed_set() {
        let msg = precision_diagnostic("Kyoto", "town");
        assert!(msg.contains(".moss/places.toml"), "names the file: {msg}");
        assert!(msg.contains("Kyoto"), "names the place: {msg}");
        assert!(msg.contains("'town'"), "names the value given: {msg}");
        for allowed in ["exact", "city", "region", "country"] {
            assert!(msg.contains(allowed), "names allowed value '{allowed}': {msg}");
        }
    }

    fn slugged(toml_str: &str) -> Gazetteer {
        let _ = crate::build::cli_output::take_cli_problems();
        parse_gazetteer(&table(toml_str))
    }

    #[test]
    fn a_slug_replaces_the_address_the_name_gives_for_that_place_and_its_parent_links() {
        let gaz = slugged("[\"上海\"]\nslug = \"shanghai\"\n[\"徐汇\"]\nparent = \"上海\"\n");
        assert_eq!(gaz.get("上海").unwrap().slug.as_deref(), Some("shanghai"));
        assert_eq!(gaz.term_key("places", "上海"), "places/shanghai");
        assert_eq!(gaz.term_key("places", "徐汇"), "places/徐汇");
        assert_eq!(crate::build::cli_output::take_cli_problems(), 0);
    }

    #[test]
    fn a_malformed_slug_is_a_problem_and_the_place_keeps_its_own_address() {
        for bad in ["Shanghai", "上海", "a/b", "a b", "-a", "a--b", ""] {
            let gaz = slugged(&format!("[\"上海\"]\nslug = \"{bad}\"\n"));
            assert_eq!(crate::build::cli_output::take_cli_problems(), 1, "{bad:?}");
            assert_eq!(gaz.term_key("places", "上海"), "places/上海", "{bad:?}");
        }
    }

    #[test]
    fn two_places_with_one_slug_are_a_problem_naming_both_and_neither_gets_it() {
        let gaz = slugged("[\"上海\"]\nslug = \"sh\"\n[\"Shenhu\"]\nslug = \"sh\"\n");
        assert_eq!(crate::build::cli_output::take_cli_problems(), 1);
        assert_eq!(gaz.term_key("places", "上海"), "places/上海");
        assert_eq!(gaz.term_key("places", "Shenhu"), "places/shenhu");
    }

    #[test]
    fn a_slug_equal_to_another_places_derived_address_is_a_problem() {
        let gaz = slugged("[\"上海\"]\nslug = \"kyoto\"\n[\"Kyoto\"]\nlat = 1\nlng = 2\n");
        assert_eq!(crate::build::cli_output::take_cli_problems(), 1);
        assert_eq!(gaz.term_key("places", "上海"), "places/上海");
        assert_eq!(gaz.term_key("places", "Kyoto"), "places/kyoto");
    }

    #[test]
    fn a_slug_equal_to_a_parent_named_only_by_a_link_is_a_problem() {
        let gaz = slugged("[\"上海\"]\nslug = \"china\"\n[\"徐汇\"]\nparent = \"China\"\n");
        assert_eq!(crate::build::cli_output::take_cli_problems(), 1);
        assert_eq!(gaz.term_key("places", "上海"), "places/上海");
    }

    #[test]
    fn a_missing_file_is_an_empty_gazetteer_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let gaz = load_gazetteer(&tmp.path().join(".moss").join("places.toml"));
        assert!(gaz.iter().next().is_none());
    }

    #[test]
    fn a_malformed_file_is_a_diagnostic_and_an_empty_gazetteer_not_a_build_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let moss = tmp.path().join(".moss");
        std::fs::create_dir_all(&moss).unwrap();
        std::fs::write(moss.join("places.toml"), "this is not [ valid toml").unwrap();
        let gaz = load_gazetteer(&moss.join("places.toml"));
        assert!(gaz.iter().next().is_none());
    }
}
