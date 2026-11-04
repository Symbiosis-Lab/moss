//! The country search index: one JSON file a page fetches the first time its
//! search field gets focus. It is the same for every country map on a site, so
//! it holds no level; the page's own paths and legend say how a result is
//! rated.

use super::matching::Atlas;
use serde::Deserialize;
use serde_json::json;
use std::sync::OnceLock;

/// The file's name inside the shapes' asset directory.
pub(crate) const FILE: &str = "country-search.json";

const CITIES: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/place-map/countries-cities-v1.json"));

#[derive(Deserialize)]
struct Cities {
    manifest_sha256: String,
    /// (city, admin code of the country that holds it), largest first.
    cities: Vec<(String, String)>,
}

fn cities() -> &'static Cities {
    static PARSED: OnceLock<Cities> = OnceLock::new();
    PARSED.get_or_init(|| serde_json::from_str(CITIES).expect("embedded city list parses"))
}

/// `{"c": [[slug, name, [other names], [cities]], ...]}`, one entry for each
/// country that has a shape. Disputed land and Antarctica have none, so they
/// cannot be found.
pub(crate) fn index() -> &'static str {
    static JSON: OnceLock<String> = OnceLock::new();
    JSON.get_or_init(|| {
        let atlas = Atlas::get();
        let rows: Vec<_> = atlas
            .shapes
            .countries
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.rings.is_empty())
            .map(|(i, c)| {
                let mut names: Vec<&str> = [c.name.as_str(), c.admin.as_str(), c.formal.as_str(), c.code.as_str()].into_iter().filter(|n| !n.is_empty() && *n != c.name_long).collect();
                names.extend(atlas.aliases_of(i));
                names.dedup();
                let places: Vec<&str> = cities().cities.iter().filter(|(_, code)| *code == c.adm0).map(|(name, _)| name.as_str()).collect();
                json!([atlas.slug_of(i), c.name_long, names, places])
            })
            .collect();
        json!({ "c": rows }).to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn entries() -> Vec<Value> {
        serde_json::from_str::<Value>(index()).unwrap()["c"].as_array().unwrap().clone()
    }
    fn strings(v: &Value) -> Vec<&str> {
        v.as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect()
    }
    fn entry(slug: &str) -> Value {
        entries().into_iter().find(|e| e[0] == slug).unwrap_or_else(|| panic!("{slug} is searchable"))
    }

    #[test]
    fn every_shaped_country_is_listed_with_its_other_names_and_its_cities() {
        assert!(entries().len() > 230);
        let nigeria = entry("nigeria");
        assert_eq!(nigeria[1], "Nigeria");
        assert!(strings(&nigeria[2]).contains(&"Federal Republic of Nigeria"));
        assert!(strings(&nigeria[3]).contains(&"Lagos"));
        let uk = entry("united-kingdom");
        assert!(strings(&uk[2]).contains(&"uk"), "the short names an author writes are searchable too");
        assert!(strings(&uk[3]).contains(&"London"));
    }

    #[test]
    fn disputed_land_and_antarctica_cannot_be_found() {
        let slugs: Vec<String> = entries().iter().map(|e| e[0].as_str().unwrap().to_string()).collect();
        for gone in ["somaliland", "northern-cyprus", "antarctica", "western-sahara"] {
            assert!(!slugs.iter().any(|s| s == gone), "{gone} is in the index");
        }
        assert!(!index().contains("Hargeisa"), "a city on disputed land is dropped");
    }

    #[test]
    fn the_city_list_was_cut_from_the_same_sources_as_the_shapes() {
        assert!(cities().manifest_sha256.starts_with(&Atlas::get().shapes.digest));
    }

    #[test]
    fn the_index_stays_within_the_budget_the_generator_records() {
        let report: Value = serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/place-map/countries-size.json"))).unwrap();
        let budget = report["index_budget_bytes"].as_u64().unwrap() as usize;
        assert!(index().len() <= budget, "{} bytes over a budget of {budget}", index().len());
    }
}
