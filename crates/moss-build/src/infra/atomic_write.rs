//! The one way moss replaces a file in app data without ever leaving a
//! half-written one behind.
//!
//! Write to a temp file in the destination's own directory, then rename. The
//! rename is atomic on every filesystem moss runs on, so a reader either sees
//! the whole old file or the whole new one — never a truncated middle. That
//! matters most for files whose partial read is a security answer rather than
//! a nuisance: the registry kill list is the case that motivated pulling this
//! together.
//!
//! **The temp name is unique per write, and that is the part worth stating.**
//! The four hand-rolled copies this replaces all used a fixed `<name>.tmp`, so
//! two concurrent writers renamed each other's half-written bytes into place.
//! For the registry that overlap is routine rather than exotic — refresh fires
//! on app launch and again on catalog open.
//!
//! The temp file is flushed with `sync_all` before the rename, so a crash
//! cannot commit the rename ahead of the bytes it names. The parent directory
//! is not flushed, so a crash can still lose the rename entirely — the
//! previous file survives intact, which is why that is acceptable and a torn
//! one would not be.
//!
//! Temp-then-rename rather than a plain `fs::write` for a second reason: a
//! write opens the destination `O_TRUNC`, which fails `EDEADLK` against a file
//! the sync client has evicted, while a rename **over** an evicted file
//! succeeds (ADR-043). So "simplify this back to `fs::write`" is a change that
//! breaks on a cloud-synced folder and nowhere else.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Replace `path` with `contents`, creating its parent directory if needed.
///
/// The temp file is a sibling of `path`, never in a system temp dir: a rename
/// is only atomic within one filesystem, and `/tmp` is routinely a different
/// one from app data.
pub fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{} has no usable file name", path.display()))?;
    // No `.tmp` suffix: a caller can live inside a synced vault folder now,
    // and iCloud Drive excludes `.tmp`-suffixed files from sync — fileproviderd
    // may remove or interfere with them before the rename lands (see
    // `build::cache::ObjectStore::store_file`, which hit this first).
    let tmp = parent.join(format!(
        ".{name}.pending.{}.{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));

    // The bytes are flushed to the device BEFORE the rename. Without that, a
    // crash between the two can commit the rename ahead of the data and leave
    // a zero-length or torn file where an intact old one used to be — which is
    // the failure the rename was chosen to prevent. `state.toml` is the case
    // that pays for it: it holds deploy state nothing regenerates.
    {
        use std::io::Write;
        // allow:raw_write writes a sibling temp file then renames; callers are app-data and vault state, never .moss/build/
        let mut f = std::fs::File::create(&tmp)
            .map_err(|e| format!("failed to write {}: {e}", tmp.display()))?;
        f.write_all(contents.as_bytes())
            .map_err(|e| format!("failed to write {}: {e}", tmp.display()))?;
        f.sync_all()
            .map_err(|e| format!("failed to flush {}: {e}", tmp.display()))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        // A failed rename leaves the temp file orphaned; nothing else will
        // ever look at it, so remove it rather than accumulating one per
        // failure.
        let _ = std::fs::remove_file(&tmp);
        format!("failed to replace {}: {e}", path.display())
    })
}

/// [`write_atomic`] for a value serialized as pretty JSON — what every current
/// caller actually wants.
pub fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| format!("failed to serialize {}: {e}", path.display()))?;
    write_atomic(path, &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_a_file_and_creates_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("thing.json");

        write_atomic(&path, "first").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");

        write_atomic(&path, "second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
    }

    #[test]
    fn leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic(&dir.path().join("a.json"), "x").unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n != "a.json")
            .collect();
        assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
    }

    #[test]
    fn concurrent_writers_do_not_share_a_temp_name() {
        // The race the four hand-rolled copies had: with a fixed `<name>.tmp`,
        // two writers rename each other's half-written bytes into place. Each
        // write must land whole, whichever wins.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("contended.json");
        let long_a = "a".repeat(200_000);
        let long_b = "b".repeat(200_000);

        let path = &path;
        std::thread::scope(|s| {
            for contents in [&long_a, &long_b] {
                s.spawn(move || {
                    for _ in 0..20 {
                        write_atomic(path, contents).unwrap();
                    }
                });
            }
        });

        let final_contents = std::fs::read_to_string(&path).unwrap();
        assert!(
            final_contents == long_a || final_contents == long_b,
            "a write was torn: {} bytes, starts with {:?}",
            final_contents.len(),
            &final_contents[..1]
        );
    }

    #[test]
    fn writes_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v.json");
        write_json_atomic(&path, &serde_json::json!({"a": 1})).unwrap();
        let back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back["a"], 1);
    }
}
