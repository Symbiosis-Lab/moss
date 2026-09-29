//! A file's stat record, and the one rule every stat-keyed fast path uses to
//! decide whether a file still holds the bytes a record was taken of — see
//! [`FileStat::vouches_for`].

use std::fs;
#[cfg(test)]
use std::path::Path;

/// What `stat(2)` reports about a file that can tell one version of its bytes
/// from another — the record the [`HashIndex`](crate::build::cache::HashIndex) keeps beside a content hash, and
/// the record a file must still show for the hash to be trusted.
///
/// The same fields, for the same reasons, as `SourceMetadata` (`build/types.rs`):
/// a whole-second mtime cannot tell the hashed file from a same-size rewrite
/// landing in the same second, ctime cannot be forged from userland where mtime
/// can, and replace-via-rename changes the inode even when size and mtime survive.
///
/// Serialized the way the hash index stores it (flattened into each entry): an
/// absent field is omitted, and one missing on read — an index from before it
/// existed — is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileStat {
    pub size: u64,
    /// Modification time, whole Unix seconds.
    pub mtime: u64,
    /// Sub-second part of the modification time. `None` when the platform reports
    /// no mtime, or for a stat built by [`FileStat::whole_second`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime_nanos: Option<u32>,
    /// Inode change time, Unix seconds, where the platform reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctime: Option<i64>,
    /// Inode number, where the platform reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inode: Option<u64>,
}

impl FileStat {
    /// Everything the platform reports about `md`.
    pub fn of(md: &fs::Metadata) -> Self {
        let mtime = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
        let (ctime, inode) = crate::build::stat::stat_identity(md);
        Self {
            size: md.len(),
            mtime: mtime.map_or(0, |d| d.as_secs()),
            mtime_nanos: mtime.map(|d| d.subsec_nanos()),
            ctime,
            inode,
        }
    }

    /// Size and whole-second mtime only, for a caller that has nothing finer —
    /// see [`HashIndex::lookup_whole_second`].
    pub fn whole_second(size: u64, mtime: u64) -> Self {
        Self { size, mtime, mtime_nanos: None, ctime: None, inode: None }
    }

    /// Whether `current`, the same file stat'd again, proves it still holds the
    /// bytes it held when this record was taken. `recorded_at` is when those bytes
    /// were read (Unix seconds, sampled before the read began), `None` when unknown.
    /// Every stat-keyed fast path — the hash index, the watcher's admission gate,
    /// the frontmatter pre-scan — decides through this one rule, and fails open: a
    /// miss costs a re-read, a false hit serves stale bytes.
    ///
    /// Size, whole-second mtime and the sub-second mtime must all agree, and a
    /// ctime or inode both sides report must too ([`identity_disagrees`]). What
    /// the sub-second agreement proves depends on the reading:
    ///
    /// - Non-zero on both sides: a real sub-second timestamp, so an equal one is
    ///   the same write.
    /// - Exactly zero on both sides: a coarse-timestamp filesystem (FAT at 2s,
    ///   older SMB/NFS, HFS+ at 1s), or a file whose stamp was restored from one — a ZIP
    ///   extraction, `rsync -a`. Every write within one tick reads the same, so
    ///   equality alone proves nothing. It proves the same write only when the
    ///   mtime is more than [`ZERO_NANOS_TRUST_AGE_SECS`] OLDER than `recorded_at`
    ///   (git's racily-clean rule, with a margin for clock skew): any write after
    ///   the read carries an mtime no earlier than one tick before the read, which
    ///   cannot land that far back. A future stamp could be reached by the clock
    ///   and collide, and a record with no clock (`None`, an index written before
    ///   the field existed) cannot show the gap, so both fail open.
    /// - Absent on either side: never proof.
    ///
    /// What remains, for either kind of stamp, is a tool that rewrites a file and
    /// then sets its mtime back to the exact recorded value (`touch -r`, an archive
    /// extractor restoring an old date). The rewrite still moves ctime, and an
    /// unlink-and-create moves the inode, so the identity check catches it on
    /// platforms that report those — short of a ctime landing in the recorded
    /// second, or a platform (Windows) or filesystem that reports neither
    /// faithfully.
    pub(crate) fn vouches_for(&self, current: &FileStat, recorded_at: Option<u64>) -> bool {
        let same_write = match (self.mtime_nanos, current.mtime_nanos) {
            (Some(a), Some(b)) if a != b => false,
            (Some(0), Some(0)) => zero_stamp_settled(self.mtime, recorded_at),
            (Some(_), Some(_)) => true,
            _ => false,
        };
        self.size == current.size
            && self.mtime == current.mtime
            && same_write
            && !identity_disagrees(self.ctime, current.ctime)
            && !identity_disagrees(self.inode, current.inode)
    }
}

/// One timestamp granularity, generously: FAT stores mtimes at 2s resolution —
/// the coarsest a filesystem moss runs on rounds to — and SMB servers round to
/// 1–2s. A write landing inside this window of a record's
/// reading could share the recorded mtime while carrying different bytes.
pub(crate) const RACY_WRITE_EPSILON_SECS: u64 = 2;

/// How much older than its recorded read an exact-zero sub-second mtime must be
/// before [`FileStat::vouches_for`] trusts it. One tick (2s) would do if the
/// mtime came from this machine's clock, but on a network mount the server
/// stamps it: a server clock behind ours, or our wall clock jumping backwards,
/// by more than this margin could still pass a same-tick rewrite. An hour
/// covers any skew a working setup has, and costs nothing where zero stamps
/// come from ZIP extraction or `rsync -a`, which are days old.
pub(crate) const ZERO_NANOS_TRUST_AGE_SECS: u64 = 3600;

/// Whether an exact-zero sub-second `mtime`, read at `recorded_at`, was already
/// older than [`ZERO_NANOS_TRUST_AGE_SECS`] then — the only case in which
/// [`FileStat::vouches_for`] takes a zero reading as proof. No clock, no proof.
pub(crate) fn zero_stamp_settled(mtime: u64, recorded_at: Option<u64>) -> bool {
    recorded_at.is_some_and(|at| mtime.saturating_add(ZERO_NANOS_TRUST_AGE_SECS) < at)
}

/// git's racily-clean window: does `mtime` fall within
/// [`RACY_WRITE_EPSILON_SECS`] of `recorded_at`, on either side? `None` (no
/// clock) marks nothing. The watcher asks it of a real sub-second mtime (see
/// `source_metadata_verdict`).
///
/// TWO-sided, deliberately: the racy case is a write straddling the read, and a
/// real sub-second mtime far in the FUTURE (a fast-clock device's sync) moves on
/// the next write like any other. One-sided marked every such file racy until
/// the clock caught up — re-hashed on every 2s watcher sweep pass.
pub(crate) fn mtime_is_racy(mtime: u64, recorded_at: Option<u64>) -> bool {
    recorded_at.is_some_and(|at| mtime.abs_diff(at) <= RACY_WRITE_EPSILON_SECS)
}

/// The clock [`FileStat::vouches_for`] measures a record's age against, in Unix
/// seconds. Sample it BEFORE reading the bytes the record will vouch for.
pub(crate) fn recording_clock() -> Option<u64> {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
}

#[cfg(test)]
impl FileStat {
    /// Stamp `path` with the wall-clock second `previous` carries, at a different
    /// instant inside it — which is what two writes in one second are on a
    /// filesystem with sub-second timestamps, forced instead of hoped for. Nothing
    /// sleeps. Returns the new mtime.
    pub(crate) fn stamp_in_the_second_of(path: &Path, previous: std::time::SystemTime) -> std::time::SystemTime {
        let d = previous.duration_since(std::time::UNIX_EPOCH).unwrap();
        let nanos = d.subsec_nanos();
        let other = if nanos >= 500_000_000 { nanos - 250_000_000 } else { nanos + 250_000_000 };
        let stamped = std::time::UNIX_EPOCH + std::time::Duration::new(d.as_secs(), other);
        fs::File::options().write(true).open(path).unwrap().set_modified(stamped).unwrap();
        stamped
    }

    /// Replace `path` the way an atomic save does — write the new bytes beside it and
    /// rename over — with the old mtime carried across. Size and mtime are what a
    /// record keyed by them cannot tell apart; the inode (and, a second later, the
    /// ctime) is all that does.
    pub(crate) fn replace_by_rename_keeping_mtime(path: &Path, bytes: &[u8]) {
        let before = fs::metadata(path).unwrap();
        assert_eq!(bytes.len() as u64, before.len(), "precondition: a same-size replacement");
        let beside = path.with_file_name(format!("{}.replacement", path.file_name().unwrap().to_string_lossy()));
        fs::write(&beside, bytes).unwrap();
        fs::File::options().write(true).open(&beside).unwrap().set_modified(before.modified().unwrap()).unwrap();
        fs::rename(&beside, path).unwrap();
        assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), before.modified().unwrap());
    }

    /// This record with each field changed in turn: what a record taken at another
    /// instant of the file can differ in. A consumer that leaves one out of the stat
    /// it looks up (or records) trusts the hash of bytes the file no longer has.
    /// ctime and inode are left out where the platform has none, since an absent
    /// field agrees with anything.
    pub(crate) fn each_field_changed(&self) -> Vec<(&'static str, FileStat)> {
        let mut rows = vec![
            ("size", FileStat { size: self.size + 1, ..*self }),
            ("mtime", FileStat { mtime: self.mtime + 1, ..*self }),
            ("sub-second mtime", FileStat { mtime_nanos: self.mtime_nanos.map(|n| (n + 250_000_000) % 1_000_000_000), ..*self }),
        ];
        if let Some(c) = self.ctime {
            rows.push(("ctime", FileStat { ctime: Some(c - 7), ..*self }));
        }
        if let Some(i) = self.inode {
            rows.push(("inode", FileStat { inode: Some(i + 1), ..*self }));
        }
        rows
    }
}


/// The forgery-resistant half of a stat record: (ctime seconds, inode).
///
/// `(None, None)` on platforms that report neither — every consumer must
/// fail open on `None` (compare only when both sides are `Some`).
pub fn stat_identity(md: &std::fs::Metadata) -> (Option<i64>, Option<u64>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (Some(md.ctime()), Some(md.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = md;
        (None, None)
    }
}

/// Both sides reported a value and they disagree. Absence on either side is
/// agreement (fail open) — old records and platforms without the field must not
/// lose their fast path forever. Every consumer of [`stat_identity`] compares
/// through this one definition.
pub(crate) fn identity_disagrees<T: PartialEq>(recorded: Option<T>, current: Option<T>) -> bool {
    matches!((recorded, current), (Some(a), Some(b)) if a != b)
}
