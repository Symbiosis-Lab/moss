//! iCloud Drive resolves a conflict on a file two machines wrote by keeping
//! both: the name it had, and `name 2`, `name 3`, … beside it. moss reads
//! exactly one name, so the other machine's state was silently ignored — a
//! publish record holding the only memory of a renamed page's old URL, a sync
//! cursor, a subscriber list. This folds each sibling into the file moss reads
//! and removes it: a TOML or JSON object takes the keys the canonical copy
//! lacks (canonical wins where both have one), a line-oriented log takes the
//! lines it lacks, and a file with no mergeable shape keeps the canonical copy
//! and says so in the log. A sibling with no canonical beside it becomes the
//! canonical. Only the synced state moss itself writes is reconciled:
//! `state.toml`, and everything under `.moss/deploy/` and `.moss/data/`.

use std::path::{Path, PathBuf};

use crate::moss_paths::MossPaths;

/// Fold every `name N` sibling under the synced state paths into its
/// canonical file. Idempotent; a sibling that cannot be read is left in place
/// and reported, never dropped on a guess.
pub fn reconcile_synced_state(mp: &MossPaths) {
    let mut files = Vec::new();
    if let Some(parent) = mp.state().parent() {
        files.extend(std::fs::read_dir(parent).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file()));
    }
    for dir in [mp.deploy_dir(), mp.data_dir()] {
        collect_files(&dir, &mut files);
    }
    for path in files {
        let Some(canonical) = canonical_of(&path) else { continue };
        if canonical == mp.state() || canonical.starts_with(mp.deploy_dir()) || canonical.starts_with(mp.data_dir()) {
            reconcile(&canonical, &path);
        }
    }
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// `state 2.toml` → `state.toml`; `deployed-article-map 3.json` → the map;
/// `cursor 2` → `cursor`. `None` for a name that is not a conflict sibling.
fn canonical_of(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let (base, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 && !name[dot + 1..].contains(' ') => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    let (stem, digits) = base.rsplit_once(' ')?;
    if stem.is_empty() || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(path.with_file_name(format!("{stem}{ext}")))
}

fn reconcile(canonical: &Path, sibling: &Path) {
    if !canonical.exists() {
        match std::fs::rename(sibling, canonical) {
            Ok(()) => log::info!("[synced-state] {} had no canonical copy — took the sibling", canonical.display()),
            Err(e) => log::warn!("[synced-state] could not adopt {}: {e}", sibling.display()),
        }
        return;
    }
    // Both paths sit under `state.toml`, `.moss/deploy/`, or `.moss/data/` —
    // synced vault state (CloudPolicy::Synced, moss_paths.rs), so either can
    // be an iCloud placeholder here. Route through the materialize-wait
    // reader rather than a raw read: a transient eviction must get a chance
    // to download before this falls back to "unreadable, left in place",
    // same as `read_managed_toml` does for `.moss/config.toml`/`state.toml`.
    let ours = crate::build::cloud_readiness::read_to_string_with_materialize_wait(
        canonical,
        crate::build::cloud_readiness::INTERACTIVE_DEADLINE,
    );
    let theirs = crate::build::cloud_readiness::read_to_string_with_materialize_wait(
        sibling,
        crate::build::cloud_readiness::INTERACTIVE_DEADLINE,
    );
    let (Ok(ours), Ok(theirs)) = (ours, theirs) else {
        log::warn!("[synced-state] {} or its sibling is unreadable — left both in place", canonical.display());
        return;
    };
    let ext = canonical.extension().and_then(|e| e.to_str()).unwrap_or("");
    let merged = match ext {
        "toml" => merge_toml(canonical, &ours, &theirs),
        "json" => merge_json(canonical, &ours, &theirs),
        "jsonl" | "csv" | "txt" => Ok(Some(union_lines(&ours, &theirs))),
        _ => Ok(None),
    };
    let taken = match merged {
        Ok(Some(text)) if text != ours => crate::infra::atomic_write::write_atomic(canonical, &text).map(|()| true),
        Ok(_) => Ok(false),
        Err(e) => Err(e),
    };
    match taken {
        Ok(true) => log::info!("[synced-state] merged {} into {}", sibling.display(), canonical.display()),
        Ok(false) => log::info!("[synced-state] {} adds nothing to {} — removed", sibling.display(), canonical.display()),
        Err(e) => {
            log::warn!("[synced-state] could not merge {} into {}: {e} — left in place", sibling.display(), canonical.display());
            return;
        }
    }
    // allow:unlink a conflict sibling of a synced state file, after its contents were folded in
    let _ = std::fs::remove_file(sibling);
}

/// `None` when the sibling adds nothing; `Some(text)` is the canonical file
/// with the sibling's missing keys filled in, written through the same
/// diff-applying rewrite every managed TOML writer uses.
fn merge_toml(canonical: &Path, ours: &str, theirs: &str) -> Result<Option<String>, String> {
    let mut root: toml::Table = ours.parse().map_err(|e| format!("canonical does not parse: {e}"))?;
    let other: toml::Table = theirs.parse().map_err(|e| format!("sibling does not parse: {e}"))?;
    if !fill_in_toml(&mut root, &other) {
        return Ok(None);
    }
    crate::infra::toml_rewrite::apply_changes(ours, &root).map(Some).map_err(|e| {
        format!("{}: {e}", canonical.display())
    })
}

fn fill_in_toml(into: &mut toml::Table, from: &toml::Table) -> bool {
    let mut changed = false;
    for (key, value) in from {
        match into.get_mut(key) {
            None => {
                into.insert(key.clone(), value.clone());
                changed = true;
            }
            Some(toml::Value::Table(mine)) => {
                if let toml::Value::Table(theirs) = value {
                    changed |= fill_in_toml(mine, theirs);
                }
            }
            Some(_) => {}
        }
    }
    changed
}

fn merge_json(canonical: &Path, ours: &str, theirs: &str) -> Result<Option<String>, String> {
    let mut mine: serde_json::Value = serde_json::from_str(ours).map_err(|e| format!("canonical does not parse: {e}"))?;
    let other: serde_json::Value = serde_json::from_str(theirs).map_err(|e| format!("sibling does not parse: {e}"))?;
    let (Some(into), Some(from)) = (mine.as_object_mut(), other.as_object()) else {
        log::info!("[synced-state] {} is not an object — keeping the canonical copy", canonical.display());
        return Ok(None);
    };
    if !fill_in_json(into, from) {
        return Ok(None);
    }
    serde_json::to_string_pretty(&mine).map(Some).map_err(|e| e.to_string())
}

fn fill_in_json(into: &mut serde_json::Map<String, serde_json::Value>, from: &serde_json::Map<String, serde_json::Value>) -> bool {
    let mut changed = false;
    for (key, value) in from {
        match into.get_mut(key) {
            None => {
                into.insert(key.clone(), value.clone());
                changed = true;
            }
            Some(serde_json::Value::Object(mine)) => {
                if let serde_json::Value::Object(theirs) = value {
                    changed |= fill_in_json(mine, theirs);
                }
            }
            Some(_) => {}
        }
    }
    changed
}

/// The canonical lines, then every sibling line not already among them.
fn union_lines(ours: &str, theirs: &str) -> String {
    let mut out = ours.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    for line in theirs.lines() {
        if !line.is_empty() && !ours.lines().any(|l| l == line) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn conflict_siblings_are_named_after_their_canonical_file() {
        for (name, canonical) in [
            ("state 2.toml", Some("state.toml")),
            ("deployed-article-map 31.json", Some("deployed-article-map.json")),
            ("cursor 2", Some("cursor")),
            ("state.toml", None),
            ("notes 2024.md", Some("notes.md")),
            (" 2.json", None),
        ] {
            assert_eq!(canonical_of(Path::new(name)).as_deref(), canonical.map(Path::new), "{name}");
        }
    }

    #[test]
    fn each_synced_state_file_takes_what_its_sibling_knew_and_the_sibling_goes() {
        let tmp = tempfile::tempdir().unwrap();
        let mp = MossPaths::new(tmp.path());
        let moss = mp.root();
        write(&mp.state(), "[deployment]\nmethod = \"moss\"\n");
        write(&moss.join("state 2.toml"), "[deployment]\nmethod = \"onion\"\ndomain = \"x.onion\"\n\n[history]\nlast = 3\n");
        write(&mp.deployed_article_map(), r#"{"a": {"url": "/a/"}}"#);
        write(&mp.deploy_dir().join("deployed-article-map 2.json"), r#"{"a": {"url": "/old-a/"}, "b": {"url": "/b/"}}"#);
        write(&mp.data_dir().join("events.jsonl"), "{\"n\":1}\n{\"n\":2}\n");
        write(&mp.data_dir().join("events 2.jsonl"), "{\"n\":2}\n{\"n\":3}\n");
        write(&mp.social_dir().join("feed.json"), "[1]");
        write(&mp.social_dir().join("feed 2.json"), "[2]");
        write(&mp.data_dir().join("redirects 2.json"), r#"{"/old/": "/new/"}"#);

        reconcile_synced_state(&mp);

        let state = std::fs::read_to_string(mp.state()).unwrap();
        assert!(state.contains("method = \"moss\""), "the canonical value wins:\n{state}");
        assert!(state.contains("domain = \"x.onion\"") && state.contains("last = 3"), "missing keys are taken:\n{state}");
        let map: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(mp.deployed_article_map()).unwrap()).unwrap();
        assert_eq!(map["a"]["url"], "/a/");
        assert_eq!(map["b"]["url"], "/b/");
        assert_eq!(std::fs::read_to_string(mp.data_dir().join("events.jsonl")).unwrap(), "{\"n\":1}\n{\"n\":2}\n{\"n\":3}\n");
        assert_eq!(std::fs::read_to_string(mp.social_dir().join("feed.json")).unwrap(), "[1]", "no mergeable shape: canonical kept");
        assert_eq!(std::fs::read_to_string(mp.data_dir().join("redirects.json")).unwrap(), r#"{"/old/": "/new/"}"#, "a lone sibling becomes canonical");
        for gone in ["state 2.toml", "deploy/deployed-article-map 2.json", "data/events 2.jsonl", "data/social/feed 2.json", "data/redirects 2.json"] {
            assert!(!moss.join(gone).exists(), "{gone} should be gone");
        }
        reconcile_synced_state(&mp);
        assert_eq!(std::fs::read_to_string(mp.state()).unwrap(), state, "a second pass changes nothing");
    }

    #[test]
    fn a_sibling_that_does_not_parse_is_left_where_it_is() {
        let tmp = tempfile::tempdir().unwrap();
        let mp = MossPaths::new(tmp.path());
        write(&mp.state(), "[deployment]\nmethod = \"moss\"\n");
        write(&mp.root().join("state 2.toml"), "not = [toml");
        reconcile_synced_state(&mp);
        assert!(mp.root().join("state 2.toml").exists());
        assert_eq!(std::fs::read_to_string(mp.state()).unwrap(), "[deployment]\nmethod = \"moss\"\n");
    }
}
