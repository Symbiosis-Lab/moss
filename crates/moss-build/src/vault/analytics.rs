//! The vault's analytics log: `.moss/data/events.jsonl` and its cursor.
//!
//! A `.moss/` file the machine owns and the author never edits, like
//! [`super::deployment_state`] — which is why it sits here rather than under
//! [`crate::seta`], whose subject is the API client. One function in this
//! module makes network calls, and it does so as the log's only writer:
//! separating the file format from the sole thing that produces it would put
//! the two halves of one invariant in two modules.
//!
//! Crossed out of `src-tauri/src/domain/events_sync.rs` on 2026-09-09 (track
//! C4e) because the publish body calls `sync_events` and had to stop naming an
//! app-crate path to cross itself. The Tauri command that reads this log for
//! the Analytics panel stayed behind — it needs `AppState` to learn which
//! folder is open.
//!
//! # Storage Layout
//!
//! ```text
//! <project_root>/
//!   .moss/data/
//!     events.jsonl          — append-only log, one JSON object per line
//!     events-cursor.json    — {"cursor": <last_id>} for delta sync
//! ```
//!
//! # Design Principles
//!
//! - **Append-only JSONL**: new events are always appended; existing lines are
//!   never modified or removed. A partial write (crash mid-line) leaves a
//!   malformed line at the end, but the cursor file (written atomically) is
//!   the authoritative "how far did we get" marker. On the next sync, events
//!   will be re-fetched from the last committed cursor — the partial line is
//!   benign (readers can skip malformed lines).
//!
//! - **Atomic cursor update**: the cursor file goes through
//!   `infra::atomic_write`, which writes a uniquely named sibling, flushes it
//!   and renames, so a crash between the JSONL flush and the cursor write
//!   never results in a corrupted or zero-length cursor.
//!
//! - **Idempotent sync**: if the server returns events we already have
//!   (cursor rewound, etc.) we will append duplicates. The consumer
//!   (analytics query layer) is expected to deduplicate by `id` if needed.

use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::seta::sites::{EventRow, EventsBatchResponse};
use crate::seta::client::{MossSetaClient, SetaError};

/// Path to the events JSONL file, relative to the project root.
const EVENTS_JSONL: &str = ".moss/data/events.jsonl";

/// Path to the cursor sidecar file, relative to the project root.
const EVENTS_CURSOR: &str = ".moss/data/events-cursor.json";

/// Number of events to request per sync page.
const SYNC_PAGE_LIMIT: u32 = 50_000;

/// Contents of the cursor sidecar file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct CursorFile {
    /// The last event ID that was successfully written to events.jsonl.
    /// The next sync should fetch events with id > cursor.
    pub cursor: u64,
}

/// Absolute path to events.jsonl for a given project root.
pub(crate) fn events_jsonl_path(project_root: &Path) -> PathBuf {
    project_root.join(EVENTS_JSONL)
}

/// Absolute path to events-cursor.json for a given project root.
pub(crate) fn cursor_path(project_root: &Path) -> PathBuf {
    project_root.join(EVENTS_CURSOR)
}

/// Read the raw contents of `events.jsonl`.
///
/// The reader lives beside the writer on purpose. It used to live in the
/// frontend, which reached the file by name through the *plugin* file API — and
/// that API fences off `.moss/` to keep third-party plugins away from the
/// identity secret, so moss's own dashboard was refused on every load for
/// eleven days (moss#997). Naming this file is now this module's privilege and
/// nobody else's.
///
/// Two answers, deliberately distinguishable:
///
/// * `Ok(None)` — the log is genuinely absent. The site has never synced, and
///   an empty dashboard is the truthful thing to draw.
/// * `Err(_)` — the log is there and we could not read it. That is **not** the
///   same thing, and a caller that renders it as "no data yet" is lying about
///   the state of the world.
///
/// `.moss/data/` syncs, so a third state exists on iCloud: present but
/// evicted. `read_input_if_present` waits for materialization and reports a
/// still-unreadable file as an error rather than as absence, which is what
/// keeps an evicted log from silently reading as a site with no traffic.
pub fn read_local_events(project_root: &Path) -> std::io::Result<Option<String>> {
    crate::build::cloud_readiness::read_input_if_present(&events_jsonl_path(project_root))
}

/// Read the local cursor value.
///
/// Returns `0` if the cursor file is missing or malformed — meaning
/// the next sync will request all events from the beginning.
pub(crate) fn read_local_cursor(project_root: &Path) -> u64 {
    let path = cursor_path(project_root);
    // `.moss/data/` syncs, so the cursor can be evicted rather than missing.
    // Both answers land on 0 here (the caller then rebuilds from events.jsonl,
    // which is what keeps a re-fetch from duplicating rows) — the materialize
    // wait is here so the ordinary evicted case gets the real cursor back
    // instead of paying for a full rebuild scan (moss#986).
    let contents = match crate::build::cloud_readiness::read_input_if_present(&path) {
        Ok(Some(s)) => s,
        Ok(None) => return 0,
        Err(_) => return 0,
    };
    match serde_json::from_str::<CursorFile>(&contents) {
        Ok(cf) => cf.cursor,
        Err(_) => 0,
    }
}

/// Read the cursor, rebuilding it from `events.jsonl` if the sidecar is missing
/// or reports cursor=0 while `events.jsonl` already exists.
///
/// Without this repair, deleting or corrupting `events-cursor.json` while
/// `events.jsonl` still exists causes the next sync to re-fetch all events and
/// append them again — producing duplicate rows.
///
/// # Behaviour
///
/// - If `events.jsonl` does **not** exist → cursor=0 is correct (fresh bootstrap).
/// - If `events.jsonl` exists but cursor sidecar is missing/zero → scan the file
///   for the maximum valid `id`, persist that as the new cursor, and return it.
/// - If the sidecar reports a non-zero cursor → return it unchanged (fast path).
///
/// # Errors
///
/// I/O errors reading `events.jsonl` or writing the rebuilt cursor file are
/// returned to the caller. In practice `sync_events` maps these to `SetaError::Io`.
pub(crate) fn read_or_rebuild_cursor(project_root: &Path) -> std::io::Result<u64> {
    let cursor = read_local_cursor(project_root);
    if cursor > 0 {
        return Ok(cursor);
    }

    let jsonl = events_jsonl_path(project_root);
    // events.jsonl exists but cursor sidecar says 0 (missing or corrupted).
    // Scan the file to find the maximum valid event id. An *evicted* log must
    // not be mistaken for a fresh bootstrap: that returns cursor=0, the sync
    // re-fetches everything and appends it, and the user gets every event
    // twice. `read_input_if_present` separates the two answers, and an
    // unreadable-but-present log becomes an error the caller surfaces.
    let contents = match crate::build::cloud_readiness::read_input_if_present(&jsonl)? {
        Some(contents) => contents,
        // Fresh bootstrap — cursor=0 is correct.
        None => return Ok(0),
    };
    let mut max_id: u64 = 0;
    for line in contents.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(id) = event.get("id").and_then(|v| v.as_u64()) {
                if id > max_id {
                    max_id = id;
                }
            }
        }
        // Malformed lines are silently skipped (consistent with reader behaviour).
    }

    if max_id > 0 {
        write_cursor(project_root, max_id)?;
    }

    Ok(max_id)
}

/// Append a batch of events to `events.jsonl` and update the cursor sidecar.
///
/// # Guarantees
///
/// - `events.jsonl` is opened in **append mode** so existing data is preserved.
/// - Each event is serialized as a single JSON line followed by `\n`.
/// - The file is flushed and synced to disk before updating the cursor.
/// - The cursor file is updated **atomically** via temp-file + rename, so a
///   crash between the JSONL flush and the cursor write never corrupts the
///   cursor (worst case: events are re-fetched on the next sync).
/// - The `.moss/data/` directory is created if it does not exist.
///
/// # Arguments
///
/// * `project_root` — Absolute path to the project directory.
/// * `events` — Slice of events to append; may be empty (no-op).
/// * `new_cursor` — Cursor value to store after appending all events.
pub(crate) fn append_events(
    project_root: &Path,
    events: &[EventRow],
    new_cursor: u64,
) -> std::io::Result<()> {
    // 1. Ensure the data directory exists.
    let data_dir = project_root.join(".moss/data");
    fs::create_dir_all(&data_dir)?;

    // 2. Append events to events.jsonl (one JSON object per line).
    if !events.is_empty() {
        let jsonl_path = events_jsonl_path(project_root);
        // User state under `.moss/data`, not regenerable output — and O_APPEND,
        // so nothing here truncates a destination the cloud may have evicted.
        // Flagged only now because crossing into moss-build put it where the
        // ADR-043 scanner can see it; the write itself is unchanged.
        // allow:raw_write appends to the author's own event log
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&jsonl_path)?;
        let mut writer = BufWriter::new(file);
        for event in events {
            let line = serde_json::to_string(event)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            writer.write_all(line.as_bytes())?;
            writer.write_all(b"\n")?;
        }
        writer.flush()?;
        // Sync the underlying file to ensure data is on disk before we update
        // the cursor. This ordering means the cursor is only advanced after the
        // events are durably written.
        writer.into_inner()?.sync_data()?;
    }

    // 3. Commit the cursor. This is the marker that says how far the JSONL is
    //    trusted, so it is written last and atomically.
    write_cursor(project_root, new_cursor)?;

    Ok(())
}

/// Replace the cursor file. The shared atomic writer flushes the bytes before
/// renaming, which is what makes "the cursor never names events that are not
/// on disk" true across a crash.
fn write_cursor(project_root: &Path, cursor: u64) -> std::io::Result<()> {
    crate::infra::atomic_write::write_json_atomic(&cursor_path(project_root), &CursorFile { cursor })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}

/// Sync all new analytics events from the server to local storage.
///
/// Reads the local cursor, then pages through `GET /api/sites/{id}/events`
/// until the server reports no more pages. Each batch is appended to
/// `events.jsonl` and the cursor is advanced atomically.
///
/// # Idempotency
///
/// If nothing has changed on the server since the last sync, the first
/// request returns an empty batch and the function returns `Ok(0)` immediately.
///
/// # Error Handling
///
/// Network and API errors are propagated as `SetaError`. I/O errors writing
/// to disk are wrapped in `SetaError::Io`. A partial sync (error mid-page)
/// leaves the cursor at the last successfully committed batch — the next call
/// resumes from there.
///
/// # Arguments
///
/// * `client` — Authenticated `MossSetaClient`.
/// * `project_root` — Absolute path to the project directory.
/// * `site_id` — moss-hosted site identifier (e.g. `"her-blog"`).
///
/// # Returns
///
/// Total number of new events appended to disk.
pub async fn sync_events(
    client: &MossSetaClient,
    project_root: &Path,
    site_id: &str,
) -> Result<usize, SetaError> {
    // The cursor read and event append touch `.moss/data/` on the (possibly
    // iCloud-synced) site folder. Run them on the blocking pool so a sync
    // fault can't wedge a runtime worker, and so they stay cancellable when
    // the publish's stall watchdog cancels the whole future. `JoinError` (task
    // panic) is surfaced as an I/O error.
    let mut cursor = {
        let pr = project_root.to_path_buf();
        tokio::task::spawn_blocking(move || read_or_rebuild_cursor(&pr))
            .await
            .map_err(|e| SetaError::Io(e.to_string()))?
            .map_err(|e| SetaError::Io(e.to_string()))?
    };
    let mut total_appended = 0usize;

    loop {
        // Each page is one round trip plus one blocking append, and the loop
        // itself is iteration-unbounded. Report liveness so the publish stall
        // watchdog can cover this phase by silence rather than having to be
        // suspended across it — a hang here still leaves the user watching a
        // publish that never returns, even though the site is already live.
        crate::infra::liveness::bump();

        let batch: EventsBatchResponse = client
            .pull_events(site_id, cursor, SYNC_PAGE_LIMIT)
            .await?;

        if batch.events.is_empty() {
            break;
        }

        let new_cursor = batch.cursor;
        let count = batch.events.len();
        let more = batch.more;

        {
            let pr = project_root.to_path_buf();
            let events = batch.events;
            tokio::task::spawn_blocking(move || append_events(&pr, &events, new_cursor))
                .await
                .map_err(|e| SetaError::Io(e.to_string()))?
                .map_err(|e| SetaError::Io(e.to_string()))?;
        }

        crate::infra::liveness::bump();
        total_appended += count;
        cursor = new_cursor;

        if !more {
            break;
        }
    }

    Ok(total_appended)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── The dashboard's read path, end to end on a real filesystem ──────────
    //
    // No mocks here on purpose. The Analytics panel's every existing test stubs
    // out the call that fetches this file, which is exactly why a read that
    // failed 100% of the time shipped green through three releases (moss#997).
    // These write with the real writer and read with the real reader.

    #[test]
    fn reads_back_the_rows_the_writer_appended() {
        let dir = TempDir::new().unwrap();
        let events = vec![make_event(1, "/"), make_event(2, "/about")];
        append_events(dir.path(), &events, 2).unwrap();

        let contents = read_local_events(dir.path())
            .expect("reading a log we just wrote must not fail")
            .expect("a log that exists must not report as absent");

        let lines: Vec<&str> = contents.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 2, "got: {contents}");
        assert!(lines[0].contains("\"path\":\"/\""), "got: {}", lines[0]);
        assert!(lines[1].contains("\"path\":\"/about\""), "got: {}", lines[1]);
        // Every row must be parseable by the frontend that consumes this text.
        for line in lines {
            serde_json::from_str::<serde_json::Value>(line).expect("valid JSON per line");
        }
    }

    #[test]
    fn an_absent_log_is_absent_not_an_error() {
        let dir = TempDir::new().unwrap();
        // Nothing has ever synced. This is the ONE state that may render as an
        // empty dashboard, so it must be distinguishable from a failure.
        assert_eq!(read_local_events(dir.path()).unwrap(), None);
    }

    #[test]
    fn an_unreadable_log_is_an_error_not_an_empty_dashboard() {
        let dir = TempDir::new().unwrap();
        // A directory where the log should be: present, and unreadable as text.
        // Conflating this with "no events" is the bug this whole issue is about.
        fs::create_dir_all(events_jsonl_path(dir.path())).unwrap();
        assert!(
            read_local_events(dir.path()).is_err(),
            "a present-but-unreadable log must not read as absence"
        );
    }

    /// Build a minimal `EventRow` for testing.
    fn make_event(id: u64, path: &str) -> EventRow {
        EventRow {
            id,
            ts: 1_700_000_000 + id,
            path: path.to_string(),
            referrer: None,
            country: Some("US".to_string()),
            browser: Some("Chrome".to_string()),
            os: Some("macOS".to_string()),
            screen: None,
            utm_source: None,
            utm_medium: None,
            event: None,
        }
    }

    // ── read_local_cursor ──────────────────────────────────────────────────

    #[test]
    fn read_local_cursor_returns_zero_when_file_missing() {
        let dir = TempDir::new().unwrap();
        assert_eq!(read_local_cursor(dir.path()), 0);
    }

    #[test]
    fn read_local_cursor_returns_value_from_file() {
        let dir = TempDir::new().unwrap();
        // Manually write a cursor file
        let data_dir = dir.path().join(".moss/data");
        fs::create_dir_all(&data_dir).unwrap();
        fs::write(
            data_dir.join("events-cursor.json"),
            r#"{"cursor":42}"#,
        )
        .unwrap();

        assert_eq!(read_local_cursor(dir.path()), 42);
    }

    #[test]
    fn read_local_cursor_returns_zero_on_malformed_json() {
        let dir = TempDir::new().unwrap();
        let data_dir = dir.path().join(".moss/data");
        fs::create_dir_all(&data_dir).unwrap();
        fs::write(
            data_dir.join("events-cursor.json"),
            b"not-valid-json{{{{",
        )
        .unwrap();

        assert_eq!(read_local_cursor(dir.path()), 0);
    }

    // ── append_events ─────────────────────────────────────────────────────

    #[test]
    fn append_events_creates_data_dir_if_missing() {
        let dir = TempDir::new().unwrap();
        // .moss/data/ does NOT exist yet
        let events = vec![make_event(1, "/home")];
        append_events(dir.path(), &events, 1).unwrap();

        assert!(dir.path().join(".moss/data").is_dir());
        assert!(events_jsonl_path(dir.path()).exists());
        assert!(cursor_path(dir.path()).exists());
    }

    #[test]
    fn append_events_writes_one_line_per_event() {
        let dir = TempDir::new().unwrap();
        let events = vec![
            make_event(1, "/"),
            make_event(2, "/about"),
            make_event(3, "/contact"),
        ];
        append_events(dir.path(), &events, 3).unwrap();

        let content = fs::read_to_string(events_jsonl_path(dir.path())).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3, "expected 3 lines, got: {:?}", lines);

        // Each line is valid JSON with expected id
        for (i, line) in lines.iter().enumerate() {
            let val: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(val["id"], (i + 1) as u64);
        }
    }

    #[test]
    fn append_events_updates_cursor_sidecar() {
        let dir = TempDir::new().unwrap();
        let events = vec![make_event(10, "/page")];
        append_events(dir.path(), &events, 10).unwrap();

        assert_eq!(read_local_cursor(dir.path()), 10);
    }

    #[test]
    fn append_events_is_additive_on_second_call() {
        let dir = TempDir::new().unwrap();

        // First batch
        let batch1 = vec![make_event(1, "/a"), make_event(2, "/b")];
        append_events(dir.path(), &batch1, 2).unwrap();

        // Second batch
        let batch2 = vec![make_event(3, "/c"), make_event(4, "/d")];
        append_events(dir.path(), &batch2, 4).unwrap();

        let content = fs::read_to_string(events_jsonl_path(dir.path())).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 4, "both batches should accumulate to 4 lines");

        // Cursor reflects second batch
        assert_eq!(read_local_cursor(dir.path()), 4);
    }

    #[test]
    fn append_events_cursor_is_authoritative_despite_partial_prior_write() {
        // Simulate a crash mid-write: events.jsonl has a partial/malformed line
        // at the end (no trailing newline). Calling append_events again should
        // succeed; the cursor sidecar is the authoritative position marker.
        //
        // JSONL append-mode behaviour on partial last line (no trailing '\n'):
        //   The new data is written directly after the partial bytes, producing a
        //   "merged" line that contains: partial_bytes + valid_json + '\n'.
        //   That merged line is not valid JSON, but subsequent valid lines are.
        //   Readers that skip malformed lines will recover cleanly.
        let dir = TempDir::new().unwrap();
        let data_dir = dir.path().join(".moss/data");
        fs::create_dir_all(&data_dir).unwrap();

        // Write 2 valid lines + 1 partial line simulating a crash mid-write.
        // No trailing newline after the partial line — this is the crash case.
        let jsonl_path = events_jsonl_path(dir.path());
        let valid_line1 = serde_json::to_string(&make_event(1, "/a")).unwrap();
        let valid_line2 = serde_json::to_string(&make_event(2, "/b")).unwrap();
        let partial_line = r#"{"id":3,"ts":1700000003,"path":"/c","referrer":null"#; // truncated

        let initial_content = format!("{}\n{}\n{}", valid_line1, valid_line2, partial_line);
        fs::write(&jsonl_path, &initial_content).unwrap();

        // Cursor sidecar reflects only events 1 and 2 (event 3 was never committed).
        let cursor_file = cursor_path(dir.path());
        fs::write(&cursor_file, r#"{"cursor":2}"#).unwrap();

        // Append a new batch (as if crash recovery resumed from cursor=2).
        let new_events = vec![make_event(3, "/c"), make_event(4, "/d")];
        append_events(dir.path(), &new_events, 4).unwrap();

        let content = fs::read_to_string(&jsonl_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();

        // Because there was no trailing '\n' after the partial line, the first
        // new event is appended directly onto it, producing 4 "lines" total:
        //   [0] valid event 1
        //   [1] valid event 2
        //   [2] partial_line + new_event_3_json  ← one garbled line
        //   [3] valid event 4
        assert_eq!(
            lines.len(),
            4,
            "expected 4 lines (2 clean + 1 merged partial + 1 new); got: {:?}",
            lines
        );

        // The last line must be valid JSON for event 4.
        let last: serde_json::Value =
            serde_json::from_str(lines[3]).expect("last line must be valid JSON for event 4");
        assert_eq!(last["id"], 4u64);

        // The cursor advances to 4 — the partial/merged line does NOT affect it.
        assert_eq!(read_local_cursor(dir.path()), 4);
    }

    #[test]
    fn append_events_with_empty_slice_only_updates_cursor() {
        let dir = TempDir::new().unwrap();
        // Call with empty events — should not create events.jsonl but should
        // still update the cursor (or create it if missing).
        append_events(dir.path(), &[], 99).unwrap();

        // No JSONL file created for empty batch
        assert!(!events_jsonl_path(dir.path()).exists());

        // Cursor should be written
        assert_eq!(read_local_cursor(dir.path()), 99);
    }

    // ── read_or_rebuild_cursor ─────────────────────────────────────────────

    #[test]
    fn rebuild_cursor_returns_zero_when_neither_file_exists() {
        let dir = TempDir::new().unwrap();
        // No events.jsonl, no cursor sidecar — fresh bootstrap.
        let cursor = read_or_rebuild_cursor(dir.path()).unwrap();
        assert_eq!(cursor, 0);
        // Cursor file should NOT be created (nothing to rebuild from).
        assert!(!cursor_path(dir.path()).exists());
    }

    #[test]
    fn rebuild_cursor_uses_sidecar_when_nonzero() {
        let dir = TempDir::new().unwrap();
        let data_dir = dir.path().join(".moss/data");
        fs::create_dir_all(&data_dir).unwrap();
        // Write a valid cursor sidecar.
        fs::write(data_dir.join("events-cursor.json"), r#"{"cursor":77}"#).unwrap();

        let cursor = read_or_rebuild_cursor(dir.path()).unwrap();
        assert_eq!(cursor, 77);
        // The sidecar must remain unchanged (no unnecessary writes).
        assert_eq!(read_local_cursor(dir.path()), 77);
    }

    #[test]
    fn rebuild_cursor_from_jsonl_when_sidecar_missing() {
        // events.jsonl with ids 1, 2, 3 — no cursor sidecar.
        let dir = TempDir::new().unwrap();
        let data_dir = dir.path().join(".moss/data");
        fs::create_dir_all(&data_dir).unwrap();

        let events = vec![make_event(1, "/a"), make_event(2, "/b"), make_event(3, "/c")];
        let jsonl: String = events
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        fs::write(events_jsonl_path(dir.path()), &jsonl).unwrap();

        // No cursor sidecar exists.
        assert!(!cursor_path(dir.path()).exists());

        let cursor = read_or_rebuild_cursor(dir.path()).unwrap();
        assert_eq!(cursor, 3, "should rebuild to max id in events.jsonl");

        // Cursor file should have been recreated atomically.
        assert_eq!(
            read_local_cursor(dir.path()),
            3,
            "rebuilt cursor must be persisted"
        );
    }

    #[test]
    fn rebuild_cursor_from_jsonl_when_sidecar_zero() {
        // events.jsonl with ids 10, 20, 30 — cursor sidecar says 0.
        let dir = TempDir::new().unwrap();
        let data_dir = dir.path().join(".moss/data");
        fs::create_dir_all(&data_dir).unwrap();

        let events = vec![
            make_event(10, "/x"),
            make_event(20, "/y"),
            make_event(30, "/z"),
        ];
        let jsonl: String = events
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        fs::write(events_jsonl_path(dir.path()), &jsonl).unwrap();
        fs::write(cursor_path(dir.path()), r#"{"cursor":0}"#).unwrap();

        let cursor = read_or_rebuild_cursor(dir.path()).unwrap();
        assert_eq!(cursor, 30);
        assert_eq!(read_local_cursor(dir.path()), 30);
    }

    #[test]
    fn rebuild_cursor_skips_malformed_lines() {
        // events.jsonl has 2 valid lines (ids 5, 7) and 1 malformed line.
        let dir = TempDir::new().unwrap();
        let data_dir = dir.path().join(".moss/data");
        fs::create_dir_all(&data_dir).unwrap();

        let line1 = serde_json::to_string(&make_event(5, "/a")).unwrap();
        let line2 = serde_json::to_string(&make_event(7, "/b")).unwrap();
        let bad_line = r#"{"id":99,"ts":bad_value"#; // malformed JSON
        let jsonl = format!("{}\n{}\n{}\n", line1, line2, bad_line);
        fs::write(events_jsonl_path(dir.path()), &jsonl).unwrap();

        let cursor = read_or_rebuild_cursor(dir.path()).unwrap();
        // Malformed line is skipped; max valid id is 7.
        assert_eq!(cursor, 7);
    }
}
