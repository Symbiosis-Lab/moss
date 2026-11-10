//! What a direct publish will put and delete, and the record that says which
//! files on the destination are moss's. Pure: no I/O, so every rule about
//! order and ownership is tested without a destination.

use std::collections::{BTreeMap, HashMap};

/// The file on the destination naming the site that published there and
/// every path it put, with the manifest entry (mode and hash) it put. `.moss-`
/// keeps it out of the addresses a visitor can have stored
/// (`served_path::is_public_address`).
pub const RECORD_NAME: &str = ".moss-published.json";

const RECORD_VERSION: u32 = 1;

/// The published-files record. An entry of `None` is a path whose bytes on
/// the destination are not known: its put started and may not have finished.
/// The path is still the site's to delete, and the next publish puts it
/// again whatever its hash.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Record {
    version: u32,
    /// Which site published here. Another site's record owns nothing for
    /// this one, so its paths are never this site's to delete.
    owner: String,
    files: BTreeMap<String, Option<String>>,
}

impl Record {
    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn files(&self) -> &BTreeMap<String, Option<String>> {
        &self.files
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).expect("a record serializes")
    }

    /// Read a record from the destination. Anything moss would not have
    /// written is refused rather than ignored: its paths are what moss
    /// deletes, and someone other than moss can write the destination.
    pub fn parse(bytes: &[u8]) -> Result<Record, String> {
        let record: Record = serde_json::from_slice(bytes)
            .map_err(|e| format!("{RECORD_NAME} on the destination is not readable ({e})"))?;
        if record.version > RECORD_VERSION {
            return Err(format!(
                "{RECORD_NAME} on the destination was written by a newer version of moss; update moss to publish here"
            ));
        }
        if let Some(bad) = record.files.keys().find(|p| !is_safe_path(p)) {
            return Err(format!(
                "{RECORD_NAME} on the destination names {bad:?}, which is not a path inside the destination"
            ));
        }
        Ok(record)
    }

    fn new(owner: &str, files: BTreeMap<String, Option<String>>) -> Record {
        Record { version: RECORD_VERSION, owner: owner.to_string(), files }
    }
}

/// A relative path that stays inside the folder it is joined to: no root, no
/// `..`, no empty or `.` segment, no backslash or `:` (a separator and a
/// drive or stream marker on Windows), and never the record itself.
pub fn is_safe_path(path: &str) -> bool {
    !path.is_empty()
        && path != RECORD_NAME
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

/// Whether a link at `path` pointing at `target` resolves inside the site: a
/// relative target that never climbs above the root. A link out of the site
/// would publish a pointer into the server's own files.
pub fn link_stays_inside(path: &str, target: &str) -> bool {
    if target.starts_with('/') || target.contains('\\') || target.contains(':') {
        return false;
    }
    let mut depth = path.split('/').count() - 1;
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." if depth == 0 => return false,
            ".." => depth -= 1,
            _ => depth += 1,
        }
    }
    true
}

/// Which pass a put belongs to. Content-addressed assets first, since their
/// names are new and no page links them yet; HTML last, so a page never
/// arrives before what it links.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Pass {
    Asset,
    Other,
    Html,
}

fn pass(path: &str) -> Pass {
    if !crate::build::served_path::is_public_address(path) {
        Pass::Asset
    } else if path.ends_with(".html") {
        Pass::Html
    } else {
        Pass::Other
    }
}

/// One publish's work, in the order it runs.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// Paths to put: assets, then other files, then HTML.
    pub puts: Vec<String>,
    /// Paths to delete, after every put: only paths the previous record
    /// names, and none at all without one. HTML first, assets last, the
    /// reverse of the puts, so a page still being served never loses what
    /// it links before it goes.
    pub deletes: Vec<String>,
    /// Written before the first put: the old record, with every path about
    /// to be put marked unknown. A publish that stops mid-put leaves a
    /// record that still owns those paths and puts them again next time.
    pub before_puts: Record,
    /// Written after the puts: old and new together, with the new hashes, so
    /// a publish that stops before its deletions does not put its files
    /// again next time.
    pub after_puts: Record,
    /// Written after the deletions: exactly what was published.
    pub after_deletes: Record,
}

impl Plan {
    /// `published` must be `owner`'s own record: another site's owns nothing
    /// here.
    pub fn new(owner: &str, files: &HashMap<String, String>, published: Option<&Record>) -> Plan {
        let old = published.map(Record::files);
        let mut puts: Vec<String> = files
            .iter()
            .filter(|(path, entry)| old.and_then(|o| o.get(*path)).and_then(Option::as_ref) != Some(*entry))
            .map(|(path, _)| path.clone())
            .collect();
        puts.sort_by(|a, b| (pass(a), a).cmp(&(pass(b), b)));

        let mut deletes: Vec<String> = old
            .into_iter()
            .flat_map(|o| o.keys())
            .filter(|path| !files.contains_key(*path))
            .cloned()
            .collect();
        deletes.sort_by(|a, b| (std::cmp::Reverse(pass(a)), a).cmp(&(std::cmp::Reverse(pass(b)), b)));

        let new = || files.iter().map(|(p, e)| (p.clone(), Some(e.clone())));
        let old = old.cloned().unwrap_or_default();
        let mut before_puts = old.clone();
        before_puts.extend(puts.iter().map(|p| (p.clone(), None)));
        let mut after_puts = old;
        after_puts.extend(new());

        Plan {
            puts,
            deletes,
            before_puts: Record::new(owner, before_puts),
            after_puts: Record::new(owner, after_puts),
            after_deletes: Record::new(owner, new().collect()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(p, e)| (p.to_string(), e.to_string())).collect()
    }

    const OWNER: &str = "site-a";

    /// An entry of `""` stands for an unknown one.
    fn record(pairs: &[(&str, &str)]) -> Record {
        Record::new(OWNER, pairs.iter().map(|(p, e)| (p.to_string(), Some(e.to_string()).filter(|e| !e.is_empty()))).collect())
    }

    #[test]
    fn assets_go_first_and_html_last() {
        let plan = Plan::new(
            OWNER,
            &files(&[("index.html", "a"), ("_moss/s.1.css", "b"), ("photo.jpg", "c"), ("a/index.html", "d")]),
            None,
        );
        assert_eq!(plan.puts, ["_moss/s.1.css", "photo.jpg", "a/index.html", "index.html"]);
    }

    #[test]
    fn deletions_run_html_first_and_assets_last() {
        let plan = Plan::new(
            OWNER,
            &files(&[]),
            Some(&record(&[("_moss/s.1.css", "b"), ("photo.jpg", "c"), ("old.html", "d")])),
        );
        assert_eq!(plan.deletes, ["old.html", "photo.jpg", "_moss/s.1.css"]);
    }

    #[test]
    fn only_changed_and_new_files_are_put() {
        let plan = Plan::new(
            OWNER,
            &files(&[("same.html", "1"), ("changed.html", "2"), ("new.html", "3")]),
            Some(&record(&[("same.html", "1"), ("changed.html", "old")])),
        );
        assert_eq!(plan.puts, ["changed.html", "new.html"]);
        assert!(plan.deletes.is_empty());
    }

    #[test]
    fn with_no_record_nothing_is_deleted() {
        let plan = Plan::new(OWNER, &files(&[("index.html", "1")]), None);
        assert!(plan.deletes.is_empty());
        assert_eq!(plan.after_deletes, record(&[("index.html", "1")]));
    }

    #[test]
    fn the_record_after_the_puts_owns_old_and_new_and_the_last_owns_only_new() {
        let plan = Plan::new(
            OWNER,
            &files(&[("keep.html", "2"), ("new.html", "3")]),
            Some(&record(&[("keep.html", "1"), ("gone.html", "9")])),
        );
        assert_eq!(plan.deletes, ["gone.html"]);
        assert_eq!(plan.after_puts, record(&[("gone.html", "9"), ("keep.html", "2"), ("new.html", "3")]));
        assert_eq!(plan.after_deletes, record(&[("keep.html", "2"), ("new.html", "3")]));
    }

    /// A put that stops midway leaves bytes the record cannot vouch for. If
    /// the record kept the old hash, a later build that returned to the old
    /// bytes would skip the put and serve the half-published ones.
    #[test]
    fn the_record_before_the_puts_marks_every_path_being_put_unknown() {
        let plan = Plan::new(
            OWNER,
            &files(&[("keep.html", "2"), ("new.html", "3")]),
            Some(&record(&[("keep.html", "1"), ("gone.html", "9")])),
        );
        assert_eq!(plan.before_puts, record(&[("gone.html", "9"), ("keep.html", ""), ("new.html", "")]));
        let after_a_stop = Plan::new(OWNER, &files(&[("keep.html", "1")]), Some(&plan.before_puts));
        assert_eq!(after_a_stop.puts, ["keep.html"]);
        assert_eq!(after_a_stop.deletes, ["gone.html", "new.html"]);
    }

    #[test]
    fn a_record_naming_a_path_outside_the_destination_is_refused() {
        for bad in ["../etc/passwd", "/etc/passwd", "a/../../b", "a//b", "./a", "c:\\x", "a\\..\\b", RECORD_NAME, ""] {
            let bytes = record(&[(bad, "1")]).to_bytes();
            assert!(Record::parse(&bytes).is_err(), "{bad:?} must be refused");
        }
        assert!(Record::parse(&record(&[("a/b.html", "1"), (".well-known/x", "2")]).to_bytes()).is_ok());
    }

    #[test]
    fn a_link_that_climbs_out_of_the_site_is_caught() {
        assert!(link_stays_inside("latest", "app"));
        assert!(link_stays_inside("a/b/link", "../c/d"));
        assert!(link_stays_inside("a/link", "../x"));
        assert!(!link_stays_inside("link", "../x"));
        assert!(!link_stays_inside("a/link", "../../x"));
        assert!(!link_stays_inside("a/link", "b/../../../x"));
        assert!(!link_stays_inside("link", "/etc/passwd"));
        assert!(!link_stays_inside("link", "c:\\x"));
    }

    #[test]
    fn a_record_from_a_newer_moss_is_refused() {
        let bytes = br#"{"version": 2, "owner": "site-a", "files": {}}"#;
        assert!(Record::parse(bytes).unwrap_err().contains("newer version"));
    }
}
