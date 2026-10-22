//! Attaching the gazetteer's `parent` links to place-typed kinds, at the
//! config stage — before [`super::derive_terms`] ever runs, and the only
//! place the gazetteer type crosses into the terms machinery at all.
//! `derive_terms` itself never reads [`crate::vault::places::Gazetteer`]; it
//! only walks the generic `parents: BTreeMap<String, String>` this pass
//! fills on each place-typed [`super::TermKind`] — the "geography is a thin
//! layer on top of the term model" rule holds structurally, not just by
//! convention, because the gazetteer type never crosses into `terms.rs`.

use super::TermKind;
use moss_core::terms::term_folder_key;

/// Fill `kind.parents` for every place-typed kind from the gazetteer's own
/// `parent` links, keyed by the SAME pseudo-folder key `derive_terms` will
/// later compute for a real `location:` value naming the same place — a
/// folded, slugged key, not a display-string lookup.
///
/// Pure: the gazetteer is already loaded by the caller, so this does no I/O
/// of its own. Detects and breaks a parent-chain cycle here, before
/// `derive_terms` ever runs, by walking each key's `parents` chain to a
/// fixed point (capped at 8 hops — deeper than the four-rung precision
/// ladder ever needs); a walk that revisits a key already seen is a cycle,
/// diagnosed and that one link removed rather than looped.
pub fn attach_parents(kinds: &mut [TermKind], gaz: &crate::vault::places::Gazetteer) {
    for kind in kinds.iter_mut() {
        if !kind.is_place {
            continue;
        }
        for (name, entry) in gaz.iter() {
            let Some(parent) = &entry.parent else { continue };
            let key = term_folder_key(&kind.key, name);
            kind.parents.insert(key, parent.clone());
        }
    }
    for kind in kinds.iter_mut() {
        if !kind.is_place {
            continue;
        }
        break_cycles(kind);
    }
}

/// A problem for every place name a page writes in a place-typed kind's field
/// that `.moss/places.toml` does not list: without an entry the page silently
/// drops off every map. A listed place with no coordinates is not this case
/// (it is a note, at load).
pub fn warn_unknown_places(documents: &[crate::build::types::ParsedDocument], kinds: &[TermKind], gaz: &crate::vault::places::Gazetteer) {
    let known: Vec<(String, &String)> = gaz.iter().map(|(name, _)| (moss_core::terms::term_fold(name), name)).collect();
    for kind in kinds.iter().filter(|k| k.is_place) {
        for doc in documents {
            let page = doc.source_path.as_deref().unwrap_or(doc.url_path.as_str());
            for field in &kind.fields {
                for name in super::field_names(doc, field) {
                    let folded = moss_core::terms::term_fold(&name);
                    if known.iter().any(|(k, _)| *k == folded) {
                        continue;
                    }
                    let nearest = known
                        .iter()
                        .map(|(k, display)| (crate::infra::edit_distance::levenshtein(&folded, k), *display))
                        .filter(|(d, _)| *d <= 2.max(folded.len() / 4))
                        .min_by_key(|(d, _)| *d);
                    let hint = nearest.map(|(_, display)| format!(" (did you mean '{display}'?)")).unwrap_or_default();
                    crate::build::cli_output::log_warn_problem!(
                        "[{page}] {field}: '{name}' is not in .moss/places.toml, so this page will not appear on any map{hint}"
                    );
                }
            }
        }
    }
}

/// The gazetteer places that pages name through a place-typed kind's fields,
/// with their coordinates, in name order. A parent such as a borough or the
/// city is in the gazetteer without being named by any page, and is not here.
pub fn used_places(documents: &[crate::build::types::ParsedDocument], kinds: &[TermKind], gaz: &crate::vault::places::Gazetteer) -> Vec<(String, (f64, f64))> {
    let mut used = std::collections::BTreeMap::new();
    for kind in kinds.iter().filter(|k| k.is_place) {
        for doc in documents {
            for field in &kind.fields {
                for name in super::field_names(doc, field) {
                    if let Some((display, record)) = gaz.find(&name) {
                        if let Some(coords) = record.coords {
                            used.insert(display.clone(), coords);
                        }
                    }
                }
            }
        }
    }
    used.into_iter().collect()
}

/// Walk every key's `parents` chain to a fixed point; a key that revisits
/// one already seen on its own walk is a cycle. Breaks it by removing the
/// one link that closed the loop, with a diagnostic naming the place whose
/// chain was cut.
fn break_cycles(kind: &mut TermKind) {
    const MAX_HOPS: usize = 8;
    let keys: Vec<String> = kind.parents.keys().cloned().collect();
    for start in keys {
        let mut seen = std::collections::HashSet::new();
        seen.insert(start.clone());
        let mut current = start.clone();
        for _ in 0..MAX_HOPS {
            let Some(parent_display) = kind.parents.get(&current) else { break };
            let parent_key = term_folder_key(&kind.key, parent_display);
            if seen.contains(&parent_key) {
                let name = current.rsplit_once('/').map_or(current.as_str(), |(_, n)| n);
                crate::build::cli_output::log_warn_problem!(
                    "place '{name}''s parent chain cycles back to itself; breaking the chain at '{name}'"
                );
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
    use crate::vault::places::Gazetteer;

    fn place_kind(key: &str) -> TermKind {
        TermKind { cards: false, key: key.to_string(), fields: vec!["location".to_string()], title: "Places".to_string(), titles: Vec::new(), counts: Vec::new(), is_place: true, parents: Default::default(), explorer: None, line: None }
    }

    fn gaz(toml_str: &str) -> Gazetteer {
        crate::vault::places::parse_gazetteer(&toml::from_str(toml_str).expect("valid toml in test fixture"))
    }

    #[test]
    fn attach_parents_records_the_immediate_parent_keyed_by_term_folder_key() {
        let mut kinds = vec![place_kind("places")];
        let g = gaz("[\"Kyoto\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\nparent = \"Japan\"\n");
        attach_parents(&mut kinds, &g);
        assert_eq!(kinds[0].parents.get("places/kyoto").map(String::as_str), Some("Japan"));
    }

    #[test]
    fn attach_parents_creates_no_term_site_itself_only_records_the_display_name() {
        // `attach_parents` runs at the config stage, before any `TermIndex`
        // exists — its whole output is a plain string map. This test pins
        // that the function reaches for nothing beyond `kinds`/`gaz`: it
        // takes no `TermIndex` parameter at all, so there is nothing here
        // that could construct a `TermSite`.
        let mut kinds = vec![place_kind("places")];
        let g = gaz("[\"Kyoto\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\nparent = \"Japan\"\n");
        attach_parents(&mut kinds, &g);
        // The parent ("Japan") never gets its own key in `parents` — only a
        // real gazetteer row or a document's own membership would give it
        // one, and neither happened here.
        assert!(kinds[0].parents.get("places/japan").is_none());
    }

    #[test]
    fn attach_parents_stops_at_a_cycle_and_diagnoses_it() {
        let mut kinds = vec![place_kind("places")];
        let g = gaz(
            "[\"A\"]\nlat = 1.0\nlng = 1.0\nprecision = \"city\"\nparent = \"B\"\n\n\
             [\"B\"]\nlat = 2.0\nlng = 2.0\nprecision = \"city\"\nparent = \"A\"\n",
        );
        attach_parents(&mut kinds, &g);
        // One of the two links must have been cut — a genuine fixed point
        // cannot exist on a two-cycle, so walking from either surviving key
        // must terminate within MAX_HOPS.
        let mut hops = 0;
        let mut current = "places/a".to_string();
        while let Some(parent_display) = kinds[0].parents.get(&current) {
            current = term_folder_key("places", parent_display);
            hops += 1;
            assert!(hops <= 8, "the cycle must have been broken, not merely capped");
        }
    }

    #[test]
    fn a_non_place_kind_is_never_filled() {
        let mut kinds = vec![TermKind {
            cards: false,
            key: "people".to_string(),
            fields: vec!["author".to_string()],
            title: "People".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false,
            parents: Default::default(), explorer: None, line: None,
        }];
        let g = gaz("[\"Kyoto\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\nparent = \"Japan\"\n");
        attach_parents(&mut kinds, &g);
        assert!(kinds[0].parents.is_empty());
    }
}
