//! `.moss/places.toml`, the hand-edited gazetteer a `location:` value looks a
//! place name up in, and the `parent` links it gives place-typed kinds.
use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::kinds::{slugged_key, TermKind};
use super::term_slug;
use crate::page::site_config::ConfigValue;

/// How precisely a place may be shown: a privacy control. A missing or
/// unrecognized value coarsens to `Country`, never `Exact`, so a typo is a
/// warning instead of a leak.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Precision {
    Exact,
    City,
    Region,
    Country,
}

/// One quoted-key table. Coordinates live only here; a claimed place page
/// supplies prose and a cover, never a coordinate.
#[derive(Debug, Clone)]
pub struct PlaceRecord {
    /// `None` when `lat`/`lng` were missing or not numbers; the entry is kept
    /// so its `parent` and `precision` still drive the breadcrumb and roll-up.
    pub coords: Option<(f64, f64)>,
    pub precision: Precision,
    /// Another place's display name, as written.
    pub parent: Option<String>,
    /// An explicit URL segment replacing the one the name gives. Never
    /// malformed or shared with another place: [`parse_gazetteer`] drops those.
    pub slug: Option<String>,
}

/// The parsed, validated gazetteer. A bad entry is dropped or coarsened with
/// a message, never an error.
#[derive(Debug, Clone, Default)]
pub struct Gazetteer {
    places: BTreeMap<String, PlaceRecord>,
    slugs: BTreeMap<String, String>,
}

/// What reading the gazetteer found, in the order it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceMessage {
    Note(String),
    Problem(String),
}

impl Gazetteer {
    /// The record for a place's exact display name.
    pub fn get(&self, name: &str) -> Option<&PlaceRecord> {
        self.places.get(name)
    }

    /// The entry for `name` as a page may spell it, compared by
    /// [`super::term_fold`]; returns the entry's display name beside its record.
    pub fn find(&self, name: &str) -> Option<(&String, &PlaceRecord)> {
        let folded = super::term_fold(name);
        self.places.iter().find(|(display, _)| super::term_fold(display) == folded)
    }

    /// The pseudo-folder key of the place `name` in `ns`, honouring its slug:
    /// the one rule every link, roll-up and map id goes through.
    pub fn term_key(&self, ns: &str, name: &str) -> String {
        slugged_key(&self.slugs, ns, name)
    }

    /// Explicit slugs by the slug their place's name derives.
    pub fn slug_overrides(&self) -> &BTreeMap<String, String> {
        &self.slugs
    }

    /// Every entry, in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &PlaceRecord)> {
        self.places.iter()
    }
}

/// Every precision value accepted, narrowest to widest.
const ALLOWED_PRECISIONS: &str = "exact, city, region, country";

/// The message for an unrecognized `precision`: where to fix it and what to.
pub fn precision_diagnostic(place: &str, bad: &str) -> String {
    format!(
        "'.moss/places.toml': place '{place}' has an invalid precision '{bad}' (allowed: {ALLOWED_PRECISIONS}); treating it as 'country'"
    )
}

/// The gazetteer from the text of `.moss/places.toml`. Text that is not TOML
/// is one problem and an empty gazetteer.
pub fn parse_gazetteer_text(text: &str) -> (Gazetteer, Vec<PlaceMessage>) {
    match basic_toml::from_str::<ConfigValue>(text) {
        Ok(ConfigValue::Table(table)) => parse_gazetteer(&table),
        Ok(_) => (Gazetteer::default(), Vec::new()),
        Err(e) => (Gazetteer::default(), vec![PlaceMessage::Problem(format!("places.toml could not be parsed: {e}"))]),
    }
}

/// The gazetteer from an already-parsed table.
pub fn parse_gazetteer(table: &BTreeMap<String, ConfigValue>) -> (Gazetteer, Vec<PlaceMessage>) {
    let mut said = Vec::new();
    let mut out = BTreeMap::new();
    for (name, value) in table {
        let Some(entry) = value.table() else { continue };
        let number = |key: &str| match entry.get(key) {
            Some(ConfigValue::Float(f)) => Some(*f),
            // `lat = 35` is an integer in TOML.
            Some(ConfigValue::Integer(i)) => Some(*i as f64),
            _ => None,
        };
        let coords = match (number("lat"), number("lng")) {
            (Some(lat), Some(lng)) => Some((lat, lng)),
            _ => None,
        };
        let text = |key: &str| entry.get(key).and_then(ConfigValue::text).cloned();
        let precision = match text("precision").as_deref() {
            Some("exact") => Precision::Exact,
            Some("city") => Precision::City,
            Some("region") => Precision::Region,
            Some("country") | None => Precision::Country,
            Some(bad) => {
                said.push(PlaceMessage::Problem(precision_diagnostic(name, bad)));
                Precision::Country
            }
        };
        out.insert(name.clone(), PlaceRecord { coords, precision, parent: text("parent"), slug: text("slug") });
    }
    said.extend(unmapped(&out).into_iter().map(|name| PlaceMessage::Note(format!(
        "place '{name}' in .moss/places.toml has no coordinates (lat/lng); its parent and precision still apply, but it will not appear on a map"
    ))));
    let slugs = check_slugs(&mut out, &mut said);
    (Gazetteer { places: out, slugs }, said)
}

/// The places without coordinates that no other place names as its parent. A
/// parent without a point is a grouping on purpose (a region its towns roll up
/// to), so only a leaf without one is worth a note.
fn unmapped(rows: &BTreeMap<String, PlaceRecord>) -> Vec<&str> {
    let parents: std::collections::BTreeSet<&str> = rows.values().filter_map(|r| r.parent.as_deref()).collect();
    rows.iter()
        .filter(|(name, record)| record.coords.is_none() && !parents.contains(name.as_str()))
        .map(|(name, _)| name.as_str())
        .collect()
}

/// One URL segment of lowercase ASCII letters, digits and single inner hyphens.
fn is_valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
        && slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Drop every slug that cannot be served, each with a problem naming the row,
/// and return the survivors keyed by the slug their place's name derives. A
/// malformed slug, or one that is another place's address, would send two
/// places to one page. Both rows of a clash lose their slug, so the outcome
/// does not depend on row order.
fn check_slugs(rows: &mut BTreeMap<String, PlaceRecord>, said: &mut Vec<PlaceMessage>) -> BTreeMap<String, String> {
    for (name, record) in rows.iter_mut() {
        if let Some(bad) = record.slug.as_deref().filter(|s| !is_valid_slug(s)) {
            said.push(PlaceMessage::Problem(format!(
                "'.moss/places.toml': place '{name}' has an invalid slug '{bad}' (use lowercase letters, digits and hyphens, like 'new-york'); using the address its name gives"
            )));
            record.slug = None;
        }
    }
    // Who answers to each address: every row (by its explicit slug, else its
    // derived one) and every parent named only by a `parent` link.
    let mut holders: BTreeMap<String, Vec<(String, bool)>> = Default::default();
    for (name, record) in rows.iter() {
        holders.entry(record.slug.clone().unwrap_or_else(|| term_slug(name))).or_default().push((name.clone(), record.slug.is_some()));
    }
    for parent in rows.values().filter_map(|r| r.parent.as_deref()) {
        if !rows.contains_key(parent) {
            holders.entry(term_slug(parent)).or_default().push((parent.to_string(), false));
        }
    }
    for (address, who) in &holders {
        // Case variants of one name derive one address on purpose; only a
        // clash that involves an explicit slug is a mistake.
        let distinct: BTreeSet<_> = who.iter().map(|(n, _)| term_slug(n)).collect();
        if !who.iter().any(|(_, explicit)| *explicit) || distinct.len() < 2 {
            continue;
        }
        let names: Vec<String> = who.iter().map(|(n, _)| format!("'{n}'")).collect();
        said.push(PlaceMessage::Problem(format!(
            "'.moss/places.toml': places {} would all be served at the address '{address}'; their slugs are ignored, so each keeps the address its name gives",
            names.join(" and ")
        )));
        for (n, explicit) in who {
            if *explicit {
                if let Some(r) = rows.get_mut(n) {
                    r.slug = None;
                }
            }
        }
    }
    rows.iter().filter_map(|(name, r)| Some((term_slug(name), r.slug.clone()?))).collect()
}

/// Fill `parents` and `slugs` on every place-typed kind from the gazetteer,
/// keyed by the same pseudo-folder key a page's `location:` value resolves
/// to, then break each parent-chain cycle at the place that closes it.
/// Returns a problem per broken cycle.
pub fn attach_parents(kinds: &mut [TermKind], gaz: &Gazetteer) -> Vec<String> {
    let mut problems = Vec::new();
    for kind in kinds.iter_mut().filter(|k| k.is_place) {
        kind.slugs = gaz.slug_overrides().clone();
        for (name, entry) in gaz.iter() {
            let Some(parent) = &entry.parent else { continue };
            let key = kind.key_in("", name);
            kind.parents.insert(key, parent.clone());
        }
    }
    for kind in kinds.iter_mut().filter(|k| k.is_place) {
        break_cycles(kind, &mut problems);
    }
    problems
}

/// Walk each key's parent chain, at most eight hops (the precision ladder
/// has four rungs); a walk that revisits a key removes the closing link.
fn break_cycles(kind: &mut TermKind, problems: &mut Vec<String>) {
    const MAX_HOPS: usize = 8;
    let keys: Vec<String> = kind.parents.keys().cloned().collect();
    for start in keys {
        let mut seen = HashSet::new();
        seen.insert(start.clone());
        let mut current = start.clone();
        for _ in 0..MAX_HOPS {
            let Some(parent_display) = kind.parents.get(&current) else { break };
            let parent_key = kind.key_in("", parent_display);
            if seen.contains(&parent_key) {
                let name = current.rsplit_once('/').map_or(current.as_str(), |(_, n)| n);
                problems.push(format!("place '{name}''s parent chain cycles back to itself; breaking the chain at '{name}'"));
                kind.parents.remove(&current);
                break;
            }
            seen.insert(parent_key.clone());
            current = parent_key;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_place_without_coordinates_that_is_nobody_s_parent_is_noted() {
        let (_, said) = parse_gazetteer_text(
            "[\"Japan\"]\nprecision = \"country\"\n\n[\"Kansai\"]\nprecision = \"region\"\nparent = \"Japan\"\n\n[\"Kyoto\"]\nlat = 35.0116\nlng = 135.7681\nparent = \"Kansai\"\n\n[\"Nara\"]\nparent = \"Kansai\"\n",
        );
        let notes: Vec<&str> = said.iter().filter_map(|m| match m { PlaceMessage::Note(n) => Some(n.as_str()), PlaceMessage::Problem(_) => None }).collect();
        // Japan and Kansai group other places; Nara is a town missing its point.
        assert_eq!(notes, ["place 'Nara' in .moss/places.toml has no coordinates (lat/lng); its parent and precision still apply, but it will not appear on a map"]);
    }
}
