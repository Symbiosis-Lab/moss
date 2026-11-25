//! The vault's analytics log, `.moss/data/events.jsonl`: one event per line,
//! appended by [`super::read_back`] from the site's events route.
//!
//! The UI command that reads this log for the Analytics panel lives outside
//! this crate: it needs app state to learn which folder is open.

use std::path::{Path, PathBuf};

use crate::seta::client::{MossSetaClient, SetaError};
use super::read_back::{self, Part, Sources};

const EVENTS_JSONL: &str = ".moss/data/events.jsonl";

pub(crate) fn events_jsonl_path(project_root: &Path) -> PathBuf {
    project_root.join(EVENTS_JSONL)
}

/// Read the raw contents of `events.jsonl`.
///
/// Naming this file is this module's privilege: the plugin file API fences
/// off `.moss/` to keep third-party plugins away from the identity secret,
/// so moss's own dashboard reads it here.
///
/// * `Ok(None)`: the log is absent. The site has never synced, and an empty
///   dashboard is the truthful thing to draw.
/// * `Err(_)`: the log is there and could not be read, which a caller must
///   never render as "no data yet".
///
/// `.moss/data/` syncs, so on iCloud the log can be present but evicted;
/// `read_input_if_present` waits for it and reports a still-unreadable file
/// as an error rather than as absence.
pub fn read_local_events(project_root: &Path) -> std::io::Result<Option<String>> {
    crate::build::cloud_readiness::read_input_if_present(&events_jsonl_path(project_root))
}

/// Pull the site's new analytics events into the log; the number appended.
pub async fn sync_events(client: &MossSetaClient, project_root: &Path, site_id: &str) -> Result<usize, SetaError> {
    let sources = Sources { seta: client, commerce: None, site_id };
    let mut outcomes = read_back::pull(&sources, project_root, &[Part::Analytics]).await;
    outcomes.pop().map_or(Ok(0), |(_, pulled)| pulled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // No mocks: the panel's own tests stub the call that fetches this file,
    // which is how a read that always failed once shipped green.

    #[test]
    fn reads_back_the_rows_the_puller_appended() {
        let dir = TempDir::new().unwrap();
        let rows = [serde_json::json!({"id": 1, "path": "/"}), serde_json::json!({"id": 2, "path": "/about", "kind": "engage"})];
        read_back::append_lines(&events_jsonl_path(dir.path()), &rows).unwrap();

        let contents = read_local_events(dir.path()).unwrap().expect("a log that exists is not absent");
        let lines: Vec<serde_json::Value> = contents.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines, rows);
    }

    #[test]
    fn an_absent_log_is_absent_not_an_error() {
        let dir = TempDir::new().unwrap();
        assert_eq!(read_local_events(dir.path()).unwrap(), None);
    }

    #[test]
    fn an_unreadable_log_is_an_error_not_an_empty_dashboard() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(events_jsonl_path(dir.path())).unwrap();
        assert!(read_local_events(dir.path()).is_err());
    }
}
