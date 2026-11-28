//! `.moss/places.toml`: the hand-edited gazetteer a `location:` frontmatter
//! value looks a place name up in. Read-only this slice — the write half
//! (`infra::toml_rewrite::apply_changes` / `vault::config::write_managed_toml`,
//! reused the way `config.toml` already is) has no consumer until the places
//! editor (slice 5) and is not built here, per moss's own "every module must
//! have a consumer" rule.

use std::path::Path;

pub use moss_core::terms::kinds::slugged_key;
pub use moss_core::terms::places::{Gazetteer, PlaceRecord, Precision};
#[cfg(test)]
use moss_core::terms::places::precision_diagnostic;

/// [`moss_core::terms::places::parse_gazetteer`] over a table the build's
/// TOML parser read, logging what it found.
pub fn parse_gazetteer(table: &toml::value::Table) -> Gazetteer {
    let (gazetteer, said) = moss_core::terms::places::parse_gazetteer(&crate::config::core_entries(table));
    log_place_messages(said);
    gazetteer
}

pub fn log_place_messages(said: Vec<moss_core::terms::places::PlaceMessage>) {
    for message in said {
        match message {
            moss_core::terms::places::PlaceMessage::Note(note) => crate::build::cli_output::log_note!("{note}"),
            moss_core::terms::places::PlaceMessage::Problem(problem) => crate::build::cli_output::log_warn_problem!("{problem}"),
        }
    }
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
