//! Generic append-only JSONL event log — the ADR-025 local-first substrate.
//!
//! One JSON object per line. Appending is O(1) and crash-safe: a process that
//! dies mid-write can only corrupt the final line, and [`read_jsonl`] skips any
//! unparseable line rather than failing the whole read. Events are immutable and
//! superseded by newer events (never edited in place).
//!
//! Reused by the signed moderation log (`moderation.jsonl`) and intended to back
//! the future subscriber/analytics event logs (ADR-025) — one helper, not three.

use std::io::Write;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Append one event as a single JSON line. Creates parent dirs and the file if
/// absent. The newline terminator makes a torn final line detectable.
pub fn append_jsonl<T: Serialize>(path: &Path, event: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(event).map_err(std::io::Error::other)?;
    let mut f = std::fs::OpenOptions::new() // allow:raw_write append, no O_TRUNC — and .moss/data is user state, not regenerable output
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{line}")
}

/// Read every event, skipping blank lines and any line that fails to parse as
/// `T`. A torn final line (crash mid-append) OR a corrupt middle line is
/// skipped-and-continued — never fatal. Missing file → empty vec.
pub fn read_jsonl<T: DeserializeOwned>(path: &Path) -> Vec<T> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<T>(l).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Ev {
        n: u64,
        s: String,
    }

    fn tmp() -> tempfile::TempDir {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .parent()
            .unwrap()
            .join("target")
            .join("test-tmp");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::TempDir::new_in(&base).unwrap()
    }

    #[test]
    fn append_then_read_roundtrips_in_order() {
        let d = tmp();
        let p = d.path().join("sub").join("log.jsonl");
        append_jsonl(&p, &Ev { n: 1, s: "a".into() }).unwrap();
        append_jsonl(&p, &Ev { n: 2, s: "b".into() }).unwrap();
        let got: Vec<Ev> = read_jsonl(&p);
        assert_eq!(got, vec![Ev { n: 1, s: "a".into() }, Ev { n: 2, s: "b".into() }]);
    }

    #[test]
    fn skips_torn_and_corrupt_lines() {
        let d = tmp();
        let p = d.path().join("log.jsonl");
        // valid, corrupt-middle, valid, torn-final (no newline, truncated json)
        std::fs::write(&p, "{\"n\":1,\"s\":\"a\"}\nNOT JSON\n{\"n\":2,\"s\":\"b\"}\n{\"n\":3,\"s\":").unwrap();
        let got: Vec<Ev> = read_jsonl(&p);
        assert_eq!(got, vec![Ev { n: 1, s: "a".into() }, Ev { n: 2, s: "b".into() }]);
    }

    #[test]
    fn missing_file_is_empty() {
        let got: Vec<Ev> = read_jsonl(std::path::Path::new("/nonexistent/x.jsonl"));
        assert!(got.is_empty());
    }
}
