//! Which folders the user trusts, and where moss keeps that answer.
//!
//! A plugin directory is part of the project, so anything written there
//! travels with a shared folder. The answer therefore lives in app data, never
//! in the folder: a folder passed on carries no trust with it.
//!
//! Pure over a root. The reader and the writer take the app-data directory as
//! a parameter, and inside a test binary they refuse any root outside the
//! system temporary directory, so no test can read or change a person's
//! answers.
//!
//! The legacy per-plugin approvals file is read once, to migrate it, and the
//! reader goes in the release after folder trust ships.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE: &str = "trusted-folders.json";

/// The per-plugin approvals an earlier moss kept. Read only to settle a
/// folder that has no answer yet.
pub const LEGACY_FILE: &str = "plugin-approvals.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FolderAnswer {
    /// The folder, canonical.
    pub dir: PathBuf,
    pub trusted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrustRecord {
    #[serde(default)]
    pub folders: Vec<FolderAnswer>,
}

impl TrustRecord {
    pub fn answer(&self, folder: &Path) -> Option<bool> {
        self.folders.iter().find(|a| a.dir == folder).map(|a| a.trusted)
    }

    pub fn set(&mut self, folder: PathBuf, trusted: bool) {
        self.folders.retain(|a| a.dir != folder);
        self.folders.push(FolderAnswer { dir: folder, trusted });
    }
}

/// Where `roots`' app keeps its answers: the non-roaming root, since whether
/// this computer runs a folder's code is a fact about this computer. The
/// per-plugin approvals a released moss kept in the roaming root are copied
/// over the first time and left in place for that moss; no release kept a
/// record there, so there is none to copy.
pub fn root(roots: &crate::infra::app_data::AppRoots) -> PathBuf {
    adopt_legacy(&roots.roaming, &roots.local);
    roots.local.clone()
}

fn adopt_legacy(from: &Path, to: &Path) {
    refuse_a_real_root_in_tests(from);
    refuse_a_real_root_in_tests(to);
    let old = from.join(LEGACY_FILE);
    if from == to || to.join(LEGACY_FILE).exists() || to.join(FILE).exists() || !old.is_file() || fs::create_dir_all(to).is_err() {
        return;
    }
    // Whole or not at all: a torn copy would read as no approvals.
    let partial = to.join(format!(".{LEGACY_FILE}.{}.tmp", std::process::id()));
    // allow:raw_write the temp this call just minted, renamed into place whole
    if fs::copy(&old, &partial).is_err() || fs::rename(&partial, to.join(LEGACY_FILE)).is_err() {
        let _ = fs::remove_file(&partial);
    }
}

/// The record under `root`, or empty. An unreadable file is empty too: the
/// cost is a question asked again, not a plugin running without an answer.
pub fn load(root: &Path) -> TrustRecord {
    refuse_a_real_root_in_tests(root);
    fs::read_to_string(root.join(FILE))
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default()
}

pub fn save(root: &Path, record: &TrustRecord) -> Result<(), String> {
    refuse_a_real_root_in_tests(root);
    fs::create_dir_all(root).map_err(|e| format!("create {}: {e}", root.display()))?;
    crate::infra::atomic_write::write_json_atomic(&root.join(FILE), record)
}

/// The approvals an earlier moss recorded, as plugin directory → the code
/// hash allowed there. Rows under `temp` are dropped: test runs once wrote
/// rows for throwaway folders into the real file, and they name no folder a
/// person will open.
pub fn load_legacy(root: &Path, temp: &Path) -> HashMap<PathBuf, String> {
    #[derive(Deserialize)]
    struct Row {
        dir: PathBuf,
        code_sha256: String,
    }
    #[derive(Deserialize)]
    struct Legacy {
        #[serde(default)]
        approvals: Vec<Row>,
    }
    refuse_a_real_root_in_tests(root);
    let temp = temp.canonicalize().unwrap_or_else(|_| temp.to_path_buf());
    fs::read_to_string(root.join(LEGACY_FILE))
        .ok()
        .and_then(|body| serde_json::from_str::<Legacy>(&body).ok())
        .map(|legacy| legacy.approvals)
        .unwrap_or_default()
        .into_iter()
        .filter(|row| !row.dir.starts_with(&temp))
        .map(|row| (row.dir, row.code_sha256))
        .collect()
}

/// Whether the earlier approvals already covered everything in a folder:
/// `carried` is each plugin directory in it that moss did not ship, with its
/// code hash now. A folder carrying nothing to trust is not settled by this.
pub fn legacy_covers(legacy: &HashMap<PathBuf, String>, carried: &[(PathBuf, Option<String>)]) -> bool {
    !carried.is_empty()
        && carried
            .iter()
            .all(|(dir, hash)| hash.is_some() && legacy.get(dir) == hash.as_ref())
}

/// Inside a test binary, the trust record may only live in a temporary
/// directory. A test that reaches a person's own record fails here, instead
/// of leaving rows in it. Checked here rather than left to the callers that
/// pass the root, because a test can pass the real root as easily as a
/// temporary one; a shipped binary never runs from `deps`, so it is never
/// stopped.
fn refuse_a_real_root_in_tests(root: &Path) {
    if !in_test_binary() {
        return;
    }
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let temp = canonical(&std::env::temp_dir());
    let root = root.ancestors().find(|a| a.exists()).map(canonical).unwrap_or_else(|| root.to_path_buf());
    assert!(
        root.starts_with(&temp),
        "a test reached the trust record at {}, outside the temporary directory {}",
        root.display(),
        temp.display()
    );
}

fn in_test_binary() -> bool {
    std::env::current_exe().is_ok_and(|exe| is_test_binary_path(&exe))
}

/// Cargo runs every test binary, unit and integration, from
/// `<profile>/deps/`, or under its newer build-dir layout from
/// `<profile>/build/<package>/<hash>/out/`. A binary a person runs is
/// copied up to `<profile>/` or installed elsewhere, so it is neither.
fn is_test_binary_path(exe: &Path) -> bool {
    let mut dirs = exe.ancestors().skip(1).map(|dir| dir.file_name().unwrap_or_default());
    match dirs.next() {
        Some(parent) if parent == "deps" => true,
        Some(parent) if parent == "out" => dirs.nth(2).is_some_and(|dir| dir == "build"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_approvals_are_copied_to_the_local_root_once_and_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let roots = crate::infra::app_data::AppRoots { roaming: tmp.path().join("roaming"), local: tmp.path().join("local") };
        fs::create_dir_all(&roots.roaming).unwrap();
        fs::write(roots.roaming.join(LEGACY_FILE), r#"{"approvals":[]}"#).unwrap();

        assert_eq!(root(&roots), roots.local);
        assert_eq!(fs::read_to_string(roots.local.join(LEGACY_FILE)).unwrap(), r#"{"approvals":[]}"#);
        assert!(roots.roaming.join(LEGACY_FILE).exists(), "the old file stays for the moss that wrote it");
        let names: Vec<_> = fs::read_dir(&roots.local).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1, "no partial copy and no record made up: {names:?}");

        fs::write(roots.local.join(LEGACY_FILE), "{}").unwrap();
        root(&roots);
        assert_eq!(fs::read_to_string(roots.local.join(LEGACY_FILE)).unwrap(), "{}", "a copy already there is never overwritten");

        fs::remove_file(roots.local.join(LEGACY_FILE)).unwrap();
        save(&roots.local, &TrustRecord::default()).unwrap();
        root(&roots);
        assert!(!roots.local.join(LEGACY_FILE).exists(), "a root that already holds a record is done");
    }

    #[test]
    fn an_answer_is_per_folder_and_survives_a_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let a = PathBuf::from("/sites/a");

        let mut record = load(tmp.path());
        assert_eq!(record.answer(&a), None, "nothing is answered until asked");

        record.set(a.clone(), true);
        save(tmp.path(), &record).unwrap();
        let mut record = load(tmp.path());
        assert_eq!(record.answer(&a), Some(true));
        assert_eq!(record.answer(Path::new("/sites/b")), None);

        record.set(a.clone(), false);
        assert_eq!(record.answer(&a), Some(false), "a later answer replaces the earlier one");
        assert_eq!(record.folders.len(), 1);

        fs::write(tmp.path().join(FILE), "{not json").unwrap();
        assert_eq!(load(tmp.path()), TrustRecord::default(), "unreadable is empty, not an error");
    }

    #[test]
    fn a_test_binary_is_known_in_both_cargo_layouts_and_a_shipped_one_is_not() {
        assert!(is_test_binary_path(Path::new("/w/target/debug/deps/moss_build-0123abcd")));
        assert!(is_test_binary_path(Path::new("/w/target/debug/build/moss-build/0123abcd/out/moss_build-0123abcd")));
        assert!(!is_test_binary_path(Path::new("/w/target/debug/moss")));
        assert!(!is_test_binary_path(Path::new("/Applications/moss.app/Contents/MacOS/moss")));
        assert!(!is_test_binary_path(Path::new("/w/out/moss")));
    }

    #[test]
    fn a_test_cannot_reach_a_record_outside_the_temporary_directory() {
        let home = crate::infra::app_data::app_data_dir_early().expect("a data directory");
        let reached = std::panic::catch_unwind(|| load(&home));
        assert!(reached.is_err(), "the real app-data record at {} was read by a test", home.display());
        let reached = std::panic::catch_unwind(|| save(&home, &TrustRecord::default()));
        assert!(reached.is_err(), "the real app-data record at {} was written by a test", home.display());
    }

    #[test]
    fn earlier_approvals_settle_a_folder_only_when_they_cover_every_plugin_at_its_code() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join(LEGACY_FILE),
            r#"{"approvals":[
                {"dir":"/sites/a/.moss/plugins/x","code_sha256":"aaa","version":"1.0.0"},
                {"dir":"/sites/a/.moss/plugins/y","code_sha256":"bbb"},
                {"dir":"/scratch/t/.moss/plugins/z","code_sha256":"ccc"}
            ]}"#,
        )
        .unwrap();
        let legacy = load_legacy(tmp.path(), Path::new("/scratch"));
        assert_eq!(legacy.len(), 2, "rows under the temporary directory are dropped");

        let x = PathBuf::from("/sites/a/.moss/plugins/x");
        let y = PathBuf::from("/sites/a/.moss/plugins/y");
        let both = [(x.clone(), Some("aaa".into())), (y.clone(), Some("bbb".into()))];
        assert!(legacy_covers(&legacy, &both));
        assert!(!legacy_covers(&legacy, &[(x.clone(), Some("aaa".into())), (y.clone(), Some("new".into()))]), "changed code");
        assert!(
            !legacy_covers(&legacy, &[(x, Some("aaa".into())), (PathBuf::from("/sites/a/.moss/plugins/w"), Some("w".into()))]),
            "a plugin never approved"
        );
        assert!(!legacy_covers(&legacy, &[(y, None)]), "unreadable code");
        assert!(!legacy_covers(&legacy, &[]), "nothing to trust settles nothing");
    }
}
