//! An import's source address moved out of `syndicated:` into `origin:`.
//!
//! `moss import` once recorded the page it imported from in `syndicated:`,
//! which says the page also lives there, so the page shows that address as
//! a copy ("Also on"). Nothing in a page tells a source from a copy, so the
//! author lists the source addresses and only those move. An address moss
//! itself posted a copy to never moves, and a page already naming a
//! different `origin:` is left as written.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use moss_core::frontmatter::{edit_fields, get_present, parse, FieldEdit};
use serde_yaml::Value;

use super::{FileChange, Skipped};

pub struct Plan {
    pub changes: Vec<FileChange>,
    pub skipped: Vec<Skipped>,
    /// Listed addresses no page's `syndicated:` holds.
    pub unmatched: Vec<String>,
}

/// One address, for matching: parsed, so host case and a default port do
/// not matter, while the path (a trailing slash included) and query do.
fn address(text: &str) -> Option<url::Url> {
    url::Url::parse(text.trim()).ok().filter(|u| matches!(u.scheme(), "http" | "https"))
}

/// The addresses in the list file at `list`, a JSON array.
pub fn load(list: &str) -> Result<Vec<url::Url>, String> {
    let text = std::fs::read_to_string(list).map_err(|e| format!("could not read {list}: {e}"))?;
    let items: Vec<String> = serde_json::from_str(&text).map_err(|e| format!("{list} must be a JSON array of addresses: {e}"))?;
    items.iter().map(|s| address(s).ok_or_else(|| format!("{s:?} in {list} is not an http(s) address"))).collect()
}

/// The addresses of the copies moss has posted for this site.
pub fn copy_addresses(folder: &Path) -> Vec<url::Url> {
    crate::syndication::record::Records::in_vault(&crate::moss_paths::MossPaths::new(folder))
        .all()
        .into_iter()
        .flat_map(|r| r.remote.into_iter().chain(r.parts))
        .filter_map(|r| address(&r.url))
        .collect()
}

pub fn plan(pages: &[(String, String)], sources: &[url::Url], copies: &[url::Url]) -> Plan {
    // Listed address → the pages whose `syndicated:` holds it.
    let mut holders: BTreeMap<&url::Url, Vec<usize>> = sources.iter().map(|u| (u, Vec::new())).collect();
    for (i, (_, text)) in pages.iter().enumerate() {
        let mut held: Vec<url::Url> = entries(&parse(text).frontmatter).iter().filter_map(Value::as_str).filter_map(address).collect();
        held.sort();
        held.dedup();
        for url in held {
            if let Some(pages) = holders.get_mut(&url) {
                pages.push(i);
            }
        }
    }

    let mut plan = Plan {
        changes: Vec::new(),
        skipped: Vec::new(),
        unmatched: holders.iter().filter(|(_, p)| p.is_empty()).map(|(u, _)| u.to_string()).collect(),
    };
    let mut per_page: BTreeMap<usize, Vec<&url::Url>> = BTreeMap::new();
    for (url, held_by) in &holders {
        for &i in held_by {
            per_page.entry(i).or_default().push(url);
        }
    }
    for (i, listed) in per_page {
        let (path, text) = &pages[i];
        match move_to_origin(text, &listed, holders[listed[0]].len(), copies) {
            Ok(after) => plan.changes.push(FileChange { path: path.clone(), before: text.clone(), after }),
            Err(reason) => plan.skipped.push(Skipped { path: path.clone(), line: None, reason }),
        }
    }
    plan
}

/// `text` with its one listed `source` moved from `syndicated:` to
/// `origin:`, or why it stays as written.
fn move_to_origin(text: &str, listed: &[&url::Url], pages_holding: usize, copies: &[url::Url]) -> Result<String, String> {
    let source = listed[0];
    if listed.len() > 1 {
        return Err(format!("{} listed addresses are in its syndicated:, and a page has one origin", listed.len()));
    }
    if pages_holding > 1 {
        return Err(format!("{source} is in {pages_holding} pages' syndicated:, so it is not one page's source"));
    }
    if copies.contains(source) {
        return Err(format!("moss posted a copy at {source}"));
    }
    let fields = parse(text).frontmatter;
    let origin = match get_present(&fields, "origin") {
        None => None,
        Some(Value::String(o)) if address(o).as_ref() == Some(source) => Some(o),
        Some(Value::String(o)) => return Err(format!("its origin: is already {o}")),
        Some(_) => return Err("its origin: is not one address".to_string()),
    };
    let (moved, rest): (Vec<Value>, Vec<Value>) =
        entries(&fields).into_iter().partition(|v| v.as_str().and_then(address).as_ref() == Some(source));
    let mut edits = vec![if rest.is_empty() {
        FieldEdit::Remove { key: "syndicated".to_string() }
    } else {
        FieldEdit::Set { key: "syndicated".to_string(), value: Value::Sequence(rest) }
    }];
    if origin.is_none() {
        let written = moved[0].as_str().unwrap_or_default().trim().to_string();
        edits.push(FieldEdit::Set { key: "origin".to_string(), value: Value::String(written) });
    }
    edit_fields(text, &edits)
}

/// A page's `syndicated:` entries as written, whatever their type: a list,
/// or one address.
fn entries(fields: &HashMap<String, Value>) -> Vec<Value> {
    match fields.get("syndicated") {
        Some(Value::Sequence(items)) => items.clone(),
        Some(one @ Value::String(_)) => vec![one.clone()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(v: &[&str]) -> Vec<url::Url> {
        v.iter().map(|s| address(s).unwrap()).collect()
    }

    fn one(path: &str, text: &str) -> Vec<(String, String)> {
        vec![(path.to_string(), text.to_string())]
    }

    fn syndicated(text: &str) -> Vec<Value> {
        entries(&parse(text).frontmatter)
    }

    const OLD: &str = "https://builder.example/u1/site/music/";
    const BSKY: &str = "https://bsky.app/profile/ana.example/post/3k";

    #[test]
    fn a_listed_source_becomes_origin_and_leaves_syndicated() {
        let page = format!("---\ntitle: Music\nsyndicated:\n- {OLD}\nuid: 34f2f76c\n---\n\nSongs.\n");
        let plan = plan(&one("music.md", &page), &urls(&[OLD]), &[]);
        assert!(plan.skipped.is_empty() && plan.unmatched.is_empty());
        assert_eq!(plan.changes[0].after, format!("---\ntitle: Music\nuid: 34f2f76c\norigin: {OLD}\n---\n\nSongs.\n"));
    }

    #[test]
    fn a_single_address_and_a_crlf_page_are_moved_too() {
        let plan = plan(&one("a.md", &format!("---\r\nsyndicated: {OLD}\r\n---\r\n\r\nSongs.\r\n")), &urls(&[OLD]), &[]);
        assert_eq!(plan.changes[0].after, format!("---\r\norigin: {OLD}\r\n---\r\n\r\nSongs.\r\n"));
    }

    #[test]
    fn the_other_entries_stay_whatever_their_type() {
        let page = format!("---\nsyndicated:\n  - {OLD}\n  - {BSKY}\n  - 2024\n---\n\nSongs.\n");
        let plan = plan(&one("music.md", &page), &urls(&[OLD]), &[]);
        let after = &plan.changes[0].after;
        assert_eq!(syndicated(after), vec![Value::String(BSKY.into()), Value::Number(2024.into())], "{after}");
        assert_eq!(parse(after).frontmatter.get("origin").and_then(Value::as_str), Some(OLD));
    }

    #[test]
    fn an_unlisted_address_and_a_different_path_are_left_alone() {
        let page = format!("---\nsyndicated:\n  - {BSKY}\n  - https://builder.example/u1/site/music\n---\n");
        let plan = plan(&one("music.md", &page), &urls(&[OLD]), &[]);
        assert!(plan.changes.is_empty() && plan.skipped.is_empty());
        assert_eq!(plan.unmatched, vec![OLD.to_string()]);
    }

    #[test]
    fn host_case_does_not_matter_but_the_query_does() {
        let page = "---\nsyndicated:\n  - https://Builder.Example/u1/site/music/\n  - https://builder.example/p?id=2\n---\n";
        let plan = plan(&one("music.md", page), &urls(&[OLD, "https://builder.example/p?id=1"]), &[]);
        let after = &plan.changes[0].after;
        assert_eq!(syndicated(after), vec![Value::String("https://builder.example/p?id=2".into())], "{after}");
    }

    #[test]
    fn an_address_written_twice_is_one_source() {
        let page = format!("---\nsyndicated:\n  - {OLD}\n  - {BSKY}\n  - {OLD}\n---\n");
        let plan = plan(&one("music.md", &page), &urls(&[OLD]), &[]);
        assert_eq!(plan.changes[0].after, format!("---\nsyndicated:\n  - {BSKY}\norigin: {OLD}\n---\n"));
    }

    #[test]
    fn a_page_with_another_origin_is_left_as_written_and_a_blank_one_is_absent() {
        let other = format!("---\norigin: https://other.example/a\nsyndicated:\n  - {OLD}\n---\n");
        let listed = plan(&one("a.md", &other), &urls(&[OLD]), &[]);
        assert!(listed.changes.is_empty() && listed.skipped[0].reason.contains("already https://other.example/a"), "{:?}", listed.skipped);

        let odd = format!("---\norigin:\n  - https://other.example/a\nsyndicated:\n  - {OLD}\n---\n");
        assert!(plan(&one("a.md", &odd), &urls(&[OLD]), &[]).skipped[0].reason.contains("not one address"));

        let blank = format!("---\norigin: \"\"\nsyndicated:\n  - {OLD}\n---\n");
        assert_eq!(plan(&one("a.md", &blank), &urls(&[OLD]), &[]).changes[0].after, format!("---\norigin: {OLD}\n---\n"));
    }

    #[test]
    fn an_origin_already_equal_is_kept_and_only_the_entry_goes() {
        let page = format!("---\norigin: {OLD}\nsyndicated:\n  - {OLD}\n---\n");
        let plan = plan(&one("music.md", &page), &urls(&[OLD]), &[]);
        assert_eq!(plan.changes[0].after, format!("---\norigin: {OLD}\n---\n"));
    }

    #[test]
    fn an_address_moss_posted_a_copy_to_never_moves() {
        let page = format!("---\nsyndicated:\n  - {BSKY}\n---\n");
        let plan = plan(&one("log.md", &page), &urls(&[BSKY]), &urls(&[BSKY]));
        assert!(plan.changes.is_empty());
        assert!(plan.skipped[0].reason.contains("moss posted a copy"), "{:?}", plan.skipped);
    }

    #[test]
    fn ambiguous_sources_are_skipped() {
        let two = format!("---\nsyndicated:\n  - {OLD}\n  - https://builder.example/u1/site/videos/\n---\n");
        let plan_two = plan(&one("a.md", &two), &urls(&[OLD, "https://builder.example/u1/site/videos/"]), &[]);
        assert!(plan_two.changes.is_empty() && plan_two.skipped[0].reason.contains("one origin"), "{:?}", plan_two.skipped);

        let page = format!("---\nsyndicated:\n  - {OLD}\n---\n");
        let pages = vec![("a.md".to_string(), page.clone()), ("b.md".to_string(), page)];
        let shared = plan(&pages, &urls(&[OLD]), &[]);
        assert!(shared.changes.is_empty() && shared.skipped.len() == 2, "{:?}", shared.skipped);
    }

    #[test]
    fn running_the_plan_on_its_own_output_changes_nothing() {
        let page = format!("---\nsyndicated:\n  - {OLD}\n  - {BSKY}\n---\n");
        let first = plan(&one("music.md", &page), &urls(&[OLD]), &[]);
        let again = plan(&one("music.md", &first.changes[0].after), &urls(&[OLD]), &[]);
        assert!(again.changes.is_empty() && again.skipped.is_empty());
    }

    #[test]
    fn the_list_is_a_json_array_of_addresses() {
        let dir = tempfile::tempdir().unwrap();
        let file = |text: &str| {
            let path = dir.path().join("list.json");
            std::fs::write(&path, text).unwrap();
            path.to_string_lossy().into_owned()
        };
        assert_eq!(load(&file(&format!("[\"{OLD}\"]"))).unwrap(), urls(&[OLD]));
        assert!(load(&file("{\"a\": 1}")).is_err());
        assert!(load(&file("[\"not an address\"]")).unwrap_err().contains("not an http(s) address"));
    }
}
