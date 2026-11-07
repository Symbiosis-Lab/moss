//! Registers the outputs a media worker staged with the manifest coordinator.
//!
//! One pass per batch: probe each staged path, heal an absent or evicted one
//! from the content store, settle the promise of one that cannot be healed, hash
//! what is there and send it. Image and video run it with their own
//! [`OutputKind`].

use std::collections::HashSet;
use std::path::Path;

use tokio::sync::mpsc;

use crate::build::coordinator::EmitMessage;
use crate::build::manifest::HashBucket;
use crate::types::assets::{AssetRegistry, Failure};

/// What differs between the workers: the log prefix, how a staged file is named
/// in a violation line, and the manifest bucket its hash lands in.
pub(super) struct OutputKind {
    pub label: &'static str,
    pub what: &'static str,
    pub bucket: HashBucket,
}

/// Where a batch is staged and what it may consult to heal or settle.
///
/// `suppressed` (the ship-time prune's verdict) excludes absent paths from the
/// violation report and from settling `Failed`: an absent suppressed path is an
/// expected prune. Registration depends only on presence.
pub(super) struct OutputEmit<'a> {
    pub kind: &'static OutputKind,
    pub root: &'a Path,
    pub tx: &'a Option<mpsc::Sender<EmitMessage>>,
    pub staging_dir: &'a Path,
    pub objects: &'a crate::build::cache::ObjectStore,
    pub suppressed: &'a HashSet<String>,
    pub registry: Option<&'a AssetRegistry>,
}

impl OutputEmit<'_> {
    /// Send `paths` to the coordinator, one `File` message each. `blocking_send`,
    /// so call it from `spawn_blocking` or a plain thread. Returns the violation
    /// lines logged, for tests.
    ///
    /// Never registers a path whose bytes are not on disk, nor without a real
    /// content hash: a missing hash fails the integrity check on a first deploy
    /// and hides a re-encode behind an unchanged filename.
    pub(super) fn run(&self, paths: &[(String, Option<String>)]) -> Vec<String> {
        let Some(tx) = self.tx else { return Vec::new() };
        if paths.is_empty() {
            return Vec::new();
        }
        let kind = self.kind;
        let mut missing: Vec<String> = Vec::new();
        let mut suppressed_absent_count: usize = 0;
        let mut read_failures: Vec<(String, String)> = Vec::new();
        // Settling `Failed`, not merely skipping, is what lets the page drop the
        // reference to a file that never ships.
        let settle_failed = |path: &String, why: Failure| {
            if let Some(registry) = self.registry {
                registry.set_failed(path.clone(), why);
            }
        };
        // An I/O error that is not a positive `NotFound` settles nothing: an
        // unreadable output is not a missing one. The coordinator records it,
        // which withholds the generation.
        let unverified = |path: &String, err: &std::io::Error| {
            let _ = tx.blocking_send(EmitMessage::Unverified { rel_path: path.clone(), detail: err.to_string() });
        };
        for (path, oid) in paths {
            let abs = self.staging_dir.join(path);
            match crate::build::io_utils::probe_path(&abs) {
                crate::build::io_utils::Presence::Present => {}
                crate::build::io_utils::Presence::Unverified(e) => {
                    unverified(path, &e);
                    continue;
                }
                crate::build::io_utils::Presence::Absent | crate::build::io_utils::Presence::Evicted => {
                    // `link_to`, not `ensure_staged`: the latter short-circuits on
                    // `exists()`, true for a 0-byte evicted stub.
                    let healed = oid.as_deref().is_some_and(|oid| self.objects.link_to(oid, &abs).is_ok());
                    if !healed {
                        if self.suppressed.contains(path) {
                            suppressed_absent_count += 1;
                        } else {
                            settle_failed(path, Failure::encode("missing at manifest registration"));
                            missing.push(abs.display().to_string());
                        }
                        continue;
                    }
                }
            }
            // Streaming hash: a kept original can be a multi-GB film.
            let hashed = std::fs::File::open(&abs)
                .and_then(|mut file| crate::build::assets::paths::hash_reader(&mut file, &|| {}));
            let hash = match hashed {
                Ok(h) => h,
                Err(e) if !crate::build::icloud::is_definitely_absent(&abs, &e) => {
                    unverified(path, &e);
                    continue;
                }
                Err(e) => {
                    let text = e.to_string();
                    if let Some(failure) = Failure::unreadable(
                        "unreadable at manifest registration",
                        crate::build::outcome::classify_unreadable(self.root, &abs, e),
                    ) {
                        settle_failed(path, failure);
                    }
                    read_failures.push((abs.display().to_string(), text));
                    continue;
                }
            };
            // `oid` is `Some` only when the caller knows the CAS blob behind these
            // exact bytes; carry-forward paths pass `None`.
            let msg = EmitMessage::File { rel_path: path.clone(), hash, bucket: kind.bucket, oid: oid.clone() };
            // A send failure means the coordinator was dropped; the bytes are
            // still on disk and the coordinator's own failure is propagated elsewhere.
            if let Err(e) = tx.blocking_send(msg) {
                log::info!("[{}] Failed to register output in coordinator: {}", kind.label, e);
            }
        }
        if suppressed_absent_count > 0 {
            log::info!(
                "[{}] {} orphan-pruned output(s) absent from staging as expected; \
                 not registered, not reported as a violation",
                kind.label,
                suppressed_absent_count
            );
        }
        let lines = summarize_coherence_violations(kind, &missing, &read_failures);
        for line in &lines {
            log::error!("{}", line);
        }
        lines
    }
}

/// At most one line per class (missing / unreadable), each with the count and a
/// first example: one root cause, such as a sync client evicting every staged
/// file, must not log once per file.
pub(super) fn summarize_coherence_violations(
    kind: &OutputKind,
    missing: &[String],
    read_failures: &[(String, String)],
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(first) = missing.first() {
        lines.push(format!(
            "[{}] coherence violation: {} missing ({}× this build); skipping manifest \
             registration. First: {}. An upstream step reported success but the bytes \
             are not on disk.",
            kind.label,
            kind.what,
            missing.len(),
            first
        ));
    }
    if let Some((path, err)) = read_failures.first() {
        lines.push(format!(
            "[{}] coherence violation: {} unreadable after existence check ({}× this \
             build); skipping manifest registration. First: {} ({})",
            kind.label,
            kind.what,
            read_failures.len(),
            path,
            err
        ));
    }
    lines
}
