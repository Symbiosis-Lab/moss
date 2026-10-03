//! Location inheritance from a work's self-named home page to its
//! companions, as the first thing [`super::derive_terms`] does.
//!
//! A work is a folder whose home page is its self-named file
//! (`<work>/<work>.md`, the tier [`moss_core::home::detect_home_file_in_folder`]
//! calls the self-named folder note). A chapter or companion page sitting in
//! that same folder with no `location:` of its own takes the home page's
//! place, so an author who tags the work once does not have to repeat the
//! tag on every chapter. Inheritance is one level only — from a page's own
//! folder home, never from a section index further up — so a season or
//! category page can never place every work under it by sitting above them.
//!
//! An inherited location is a real location for every consumer, not a
//! second-class one: a companion page that inherited its work's place still
//! shows that place's static locator (`[site] locator = ...`), exactly as
//! it would if the author had typed the same `location:` by hand. The
//! explorer's later one-dot-per-work grouping is a *display* choice made by
//! that pass, not a property of the location itself.
//!
//! Forward contract for the places explorer's data emitter (a later pass):
//! it groups articles by `(folder, self-named home)` into one entry per
//! work, with companions listed under it. This pass only guarantees every
//! companion carries the work's location; it does not itself group or list
//! anything.

use std::collections::BTreeMap;

use moss_core::content_graph::generate_slug;
use moss_core::home::is_index_stem_any_lang;
use moss_core::PageKind;

use crate::build::types::ParsedDocument;

/// Fill in a blank `location:` on every companion page that shares its
/// folder with a work's self-named home.
///
/// The folder's home is not re-elected here by filename alone: it is
/// whichever document in the folder already carries `PageKind::Folder`,
/// the build's own real election result — `home: true` markers and
/// translation-group promotion (`scan::page_map::compute_home_file_winners`)
/// already resolved into this flag well before `derive_terms` runs, including
/// demoting every non-winning candidate back to `PageKind::Article` on a
/// multi-candidate folder. Re-deriving the winner from filenames alone here
/// would disagree with the real build whenever a marker overrides it.
///
/// Even so, winning the folder's home slot is not enough to make the folder
/// a work: the design's test is specifically that the home page is the
/// folder's *self-named* file. A folder whose real home is `index.md`, or
/// is a `home: true`-marked file that is not itself self-named, is an
/// ordinary section — its children never inherit, exactly as if no home
/// existed at all.
///
/// A document whose own `location:` is already set keeps it — this only
/// fills blanks, never overwrites. The home page's own `location:` is never
/// touched either way.
pub fn inherit_work_locations(documents: &mut [ParsedDocument]) {
    for group in group_by_work(documents).into_values() {
        let home_location = documents[group.home_idx].location.clone();
        for i in group.companions {
            if documents[i].location.is_empty() {
                documents[i].location = home_location.clone();
            }
        }
    }
}

/// One folder's work grouping: the index (into the caller's `documents`
/// slice) of the self-named home page, and every other document sharing its
/// folder — in document order, which is source-path order.
pub(crate) struct WorkGroup {
    pub home_idx: usize,
    pub companions: Vec<usize>,
}

/// Group `documents` by folder and pick out each folder's work home, using
/// the design's test verbatim: the folder's real home (`PageKind::Folder`,
/// the build's own election result) must be self-named to its folder, not
/// an index-stem winner and not a `home: true` override that landed on a
/// differently-named file, and the home must name a place. A folder that
/// fails any of those — including a lone document with no companion, or no
/// home at all — is simply absent
/// from the result; every caller's "is this a work" question is answered by
/// `.contains_key` on the returned map.
///
/// The single grouping implementation [`inherit_work_locations`] and the
/// places-explorer data emitter both build on — see this module's forward
/// contract above for why the emitter needs the exact same folder/home
/// election this pass already performs.
pub(crate) fn group_by_work(documents: &[ParsedDocument]) -> BTreeMap<String, WorkGroup> {
    let mut by_folder: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, doc) in documents.iter().enumerate() {
        let Some(path) = doc.source_path.as_deref() else { continue };
        let folder = path.rsplit_once('/').map(|(dir, _)| dir.to_string()).unwrap_or_default();
        by_folder.entry(folder).or_default().push(i);
    }

    let mut works = BTreeMap::new();
    for (folder, indices) in by_folder {
        // A lone document has no companion to inherit anything.
        if indices.len() < 2 {
            continue;
        }
        // A folder name is required for the self-named identity check
        // below (`is_home_file`'s own invariant); root-level documents
        // (`folder` is empty) can never form a work.
        let folder_name = folder.rsplit('/').next().unwrap_or("");
        if folder_name.is_empty() {
            continue;
        }

        let Some(home_idx) = indices.iter().copied().find(|&i| documents[i].kind == PageKind::Folder)
        else {
            continue;
        };

        // A home that names no place makes the folder a section for every
        // caller: nothing to inherit, nothing to group under.
        if documents[home_idx].location.is_empty() {
            continue;
        }

        let home_stem = documents[home_idx]
            .source_path
            .as_deref()
            .and_then(|p| p.rsplit('/').next())
            .and_then(|f| std::path::Path::new(f).file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("");
        // Index-stem winner (`index.md`, `readme.md`, a language-suffixed
        // form, ...): this folder is a section, not a work.
        if is_index_stem_any_lang(home_stem) {
            continue;
        }
        // Not index-stem and not self-named means a `home: true` marker
        // promoted a file that shares neither name with its folder — also
        // not a work.
        if generate_slug(home_stem) != generate_slug(folder_name) {
            continue;
        }

        let companions = indices.into_iter().filter(|&i| i != home_idx).collect();
        works.insert(folder, WorkGroup { home_idx, companions });
    }
    works
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(source_path: &str, location: &[&str], kind: PageKind) -> ParsedDocument {
        ParsedDocument {
            source_path: Some(source_path.to_string()),
            location: location.iter().map(|s| s.to_string()).collect(),
            kind,
            ..Default::default()
        }
    }

    fn home(source_path: &str, location: &[&str]) -> ParsedDocument {
        doc(source_path, location, PageKind::Folder)
    }

    fn companion(source_path: &str, location: &[&str]) -> ParsedDocument {
        doc(source_path, location, PageKind::Article)
    }

    #[test]
    fn group_by_work_finds_the_self_named_home_and_lists_its_companions_in_source_order() {
        // The home sits in the middle of the folder's three documents, not
        // first — `group_by_work` must elect it by the self-named test, not
        // by document position, and `companions` must list the other two in
        // their original source order with the home excluded.
        let docs = vec![
            companion("works/kyoto-walk/evening.md", &[]),
            home("works/kyoto-walk/kyoto-walk.md", &["Kyoto"]),
            companion("works/kyoto-walk/morning.md", &[]),
        ];
        let works = group_by_work(&docs);
        let group = works.get("works/kyoto-walk").expect("kyoto-walk is a work");
        assert_eq!(group.home_idx, 1, "the self-named home, found by election, not by position");
        assert_eq!(group.companions, vec![0, 2], "companions in source order, home excluded");
    }

    #[test]
    fn group_by_work_leaves_out_a_folder_whose_home_names_no_place() {
        let docs = vec![
            home("works/kyoto-walk/kyoto-walk.md", &[]),
            companion("works/kyoto-walk/morning.md", &["Nara"]),
        ];
        assert!(group_by_work(&docs).is_empty());
    }

    #[test]
    fn companion_with_no_location_inherits_the_self_named_homes() {
        let mut docs = vec![
            home("works/kyoto-walk/kyoto-walk.md", &["Kyoto"]),
            companion("works/kyoto-walk/morning.md", &[]),
        ];
        inherit_work_locations(&mut docs);
        assert_eq!(docs[1].location, vec!["Kyoto".to_string()]);
    }

    #[test]
    fn companion_with_its_own_location_keeps_it() {
        let mut docs = vec![
            home("works/kyoto-walk/kyoto-walk.md", &["Kyoto"]),
            companion("works/kyoto-walk/evening.md", &["Osaka"]),
        ];
        inherit_work_locations(&mut docs);
        assert_eq!(docs[1].location, vec!["Osaka".to_string()]);
    }

    #[test]
    fn a_folder_whose_home_is_index_md_never_propagates() {
        let mut docs = vec![
            home("guides/index.md", &["Kyoto"]),
            companion("guides/morning.md", &[]),
        ];
        inherit_work_locations(&mut docs);
        assert!(docs[1].location.is_empty());
    }

    #[test]
    fn a_folder_with_both_index_md_and_a_self_named_file_is_not_a_work() {
        // In the real build only the election's winner keeps `PageKind::Folder`
        // — `index.md` outranks the self-named file, which gets demoted to
        // `PageKind::Article` before `derive_terms` ever runs. This folder is
        // a section, not a work, even though `kyoto-walk.md` would have
        // matched the self-named test on its own.
        let mut docs = vec![
            home("works/kyoto-walk/index.md", &["Kyoto"]),
            companion("works/kyoto-walk/kyoto-walk.md", &[]),
            companion("works/kyoto-walk/morning.md", &[]),
        ];
        inherit_work_locations(&mut docs);
        assert!(docs[1].location.is_empty());
        assert!(docs[2].location.is_empty());
    }

    #[test]
    fn a_marked_home_that_is_not_self_named_inherits_nothing() {
        // `home: true` on `intro.md` wins the real election
        // (`compute_home_file_winners` layers the marker over filename
        // detection), so `intro.md` — not the self-named `kyoto-walk.md` —
        // carries `PageKind::Folder` here. `intro.md` is not self-named to
        // its folder, so this folder is not a work: nothing inherits,
        // including `kyoto-walk.md` itself, which lost the election and is
        // just an ordinary companion now.
        let mut docs = vec![
            home("works/kyoto-walk/intro.md", &["Kyoto"]),
            companion("works/kyoto-walk/kyoto-walk.md", &[]),
            companion("works/kyoto-walk/evening.md", &[]),
        ];
        inherit_work_locations(&mut docs);
        assert!(docs[1].location.is_empty());
        assert!(docs[2].location.is_empty());
    }

    #[test]
    fn a_marked_self_named_home_still_inherits() {
        // The marker and the self-named test agree here — `home: true`
        // simply confirms the same file the filename tier would have
        // picked anyway — so the folder is still a work.
        let mut docs = vec![
            home("works/kyoto-walk/kyoto-walk.md", &["Kyoto"]),
            companion("works/kyoto-walk/morning.md", &[]),
        ];
        inherit_work_locations(&mut docs);
        assert_eq!(docs[1].location, vec!["Kyoto".to_string()]);
    }

    #[test]
    fn a_blank_home_location_propagates_nothing() {
        let mut docs = vec![
            home("works/kyoto-walk/kyoto-walk.md", &[]),
            companion("works/kyoto-walk/morning.md", &[]),
        ];
        inherit_work_locations(&mut docs);
        assert!(docs[1].location.is_empty());
    }

    #[test]
    fn inheritance_does_not_climb_above_the_immediate_folder() {
        // A section index one level up must never seed every work under it.
        let mut docs = vec![
            home("works/index.md", &["Kyoto"]),
            home("works/kyoto-walk/kyoto-walk.md", &[]),
            companion("works/kyoto-walk/morning.md", &[]),
        ];
        inherit_work_locations(&mut docs);
        assert!(docs[1].location.is_empty());
        assert!(docs[2].location.is_empty());
    }

    #[test]
    fn a_folder_with_no_home_at_all_propagates_nothing() {
        // Flat siblings with no `PageKind::Folder` among them at all (a
        // `posts/`-style folder with no index): nothing to elect, nothing
        // to inherit.
        let mut docs = vec![
            companion("posts/kyoto-report.md", &["Kyoto"]),
            companion("posts/nara-diary.md", &[]),
        ];
        inherit_work_locations(&mut docs);
        assert!(docs[1].location.is_empty());
    }

    #[test]
    fn the_companion_inherits_and_becomes_a_member_of_the_homes_place() {
        // Integration with `derive_terms`: an inherited location must feed
        // the term-membership pass exactly like an authored one. Calls
        // `derive_terms` itself (not `inherit_work_locations` directly) so
        // this also pins that `derive_terms` runs the inheritance pass
        // before its own membership derivation.
        let kinds = vec![super::super::TermKind {
            key: "places".to_string(),
            fields: vec!["location".to_string()],
            title: "Places".to_string(),
            is_place: true,
            parents: Default::default(), explorer: None, line: None,
        }];
        let mut docs = vec![
            home("works/kyoto-walk/kyoto-walk.md", &["Kyoto"]),
            companion("works/kyoto-walk/morning.md", &[]),
        ];
        docs[0].url_path = "works/kyoto-walk/".to_string();
        docs[1].url_path = "works/kyoto-walk/morning/".to_string();
        let index = super::super::derive_terms(&mut docs, kinds);
        assert!(docs[1].also_in.as_ref().unwrap().contains(&"places/kyoto".to_string()));
        // The home page itself also carries `location: Kyoto` and is not
        // the claimant of `places/kyoto`, so it is a member alongside the
        // companion that just inherited the same place.
        assert_eq!(
            index.members_by_field().get(&("places/kyoto".to_string(), "location".to_string())),
            Some(&vec!["works/kyoto-walk/".to_string(), "works/kyoto-walk/morning/".to_string()])
        );
    }
}
