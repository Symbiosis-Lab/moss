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
}

/// The parsed, validated `.moss/places.toml`. Missing the whole file is not
/// an error (a site with no gazetteer yet is normal); a malformed whole-file
/// parse is a diagnostic plus an empty gazetteer, never a build failure; an
/// individual bad entry is dropped or coarsened with a diagnostic, never a
/// crash.
#[derive(Debug, Default)]
pub struct Gazetteer(std::collections::BTreeMap<String, PlaceRecord>);

impl Gazetteer {
    /// The record for a place's exact display name, if the gazetteer has one.
    pub fn get(&self, name: &str) -> Option<&PlaceRecord> {
        self.0.get(name)
    }

    /// Every entry, in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &PlaceRecord)> {
        self.0.iter()
    }
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
                crate::build::cli_output::log_warn_problem!(
                    "place '{name}' in .moss/places.toml is missing lat/lng; its parent and precision still apply, but it will not appear on a map"
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
                crate::build::cli_output::log_warn_problem!(
                    "place '{name}' has an invalid precision '{bad}'; treating it as 'country'"
                );
                Precision::Country
            }
            // No `precision` key at all is not itself a mistake worth a
            // diagnostic — it still coarsens to the safe default.
            None => Precision::Country,
        };
        let parent = entry.get("parent").and_then(|v| v.as_str()).map(str::to_string);
        out.insert(name.clone(), PlaceRecord { coords, precision, parent });
    }
    Gazetteer(out)
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
        let gaz = parse_gazetteer(&table(
            "[\"Kyoto\"]\nlat = 35.0116\nprecision = \"city\"\nparent = \"Japan\"\n",
        ));
        let kyoto = gaz.get("Kyoto").expect("Kyoto entry present");
        assert_eq!(kyoto.coords, None);
        assert_eq!(kyoto.precision, Precision::City);
        assert_eq!(kyoto.parent.as_deref(), Some("Japan"));
    }

    #[test]
    fn an_entry_with_an_invalid_precision_is_coarsened_to_country_with_a_diagnostic() {
        let gaz = parse_gazetteer(&table(
            "[\"Kyoto\"]\nlat = 35.0116\nlng = 135.7681\nprecision = \"neighborhood\"\n",
        ));
        let kyoto = gaz.get("Kyoto").expect("Kyoto entry present");
        assert_eq!(kyoto.precision, Precision::Country);
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
