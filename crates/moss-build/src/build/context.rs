//! Unified emit handle for the build lifecycle.
//!
//! [`BuildContext`] pairs the write destination with the manifest registration
//! it implies, so an artifact cannot reach disk without reaching the manifest.
//!
//! # Stage and site directories
//!
//! Every context writes to stage and only to stage; `ship_phase` mirrors
//! stage→site once, at the end of the blocking phase. A dual-write constructor
//! (`for_deferred`, per-emit inline `ship_one`) existed alongside these and was
//! never called by anything but its own tests — deleted with `DeferredWork`
//! (moss#618), along with the `site_dir` field that only it ever set.
//!
//! There was a second sink — a `Channel` variant that sent registrations to the
//! coordinator for the background phase. Its only constructor
//! (`for_deferred_stage_only`) never acquired a caller, and `emit_existing` was
//! the last thing that could have reached the arm; both are gone, and with one
//! variant left the enum went with them.

use std::path::Path;

use crate::build::manifest::{HashBucket, PendingManifest};

// ---------------------------------------------------------------------------
// BuildContext
// ---------------------------------------------------------------------------

/// Unified emit handle for the build lifecycle.
///
/// Encapsulates both the write destination and the manifest registration path.
/// All artifact emission should go through this type rather than writing files
/// and calling `PendingManifest::register` by hand.
///
/// See module-level docs for the constructor table.
pub struct BuildContext<'a> {
    output_dir: &'a Path,
    manifest: &'a mut PendingManifest,
}

impl<'a> BuildContext<'a> {
    /// Writes to `output_dir` only (the stage). `materialize_and_promote`
    /// builds the generation directory from the sealed manifest afterwards —
    /// nothing here mirrors per-emit.
    pub fn for_render(output_dir: &'a Path, manifest: &'a mut PendingManifest) -> Self {
        Self { output_dir, manifest }
    }

    /// Write file to stage, then register in the manifest.
    ///
    /// Parent directories are created automatically via `create_dir_all`.
    pub fn emit(
        &mut self,
        rel_path: &crate::build::served_path::ServedPath,
        bytes: &[u8],
        bucket: HashBucket,
    ) -> std::io::Result<()> {
        // 1. Write to stage (output_dir).
        write_at(self.output_dir, rel_path.as_str(), bytes)?;

        // 2. Register in the manifest.
        self.manifest.register(rel_path, bytes, bucket);

        Ok(())
    }

    /// [`emit`][Self::emit] for a derived output whose generation must ship
    /// THESE bytes: the manifest keeps them until the seal tail has shipped
    /// (`PendingManifest::register_held`), instead of leaving the generation to
    /// copy whatever a later build has since written to the same stage path.
    ///
    /// Takes the buffer by value so the manifest can keep it without a copy.
    /// Errors, without registering, for a path `ship_phase` transforms (HTML) —
    /// see `register_held`. Use it for a file that every build rewrites under
    /// one name and no CAS object backs; a content-named or config-only output
    /// cannot diverge and gains nothing from being held.
    pub fn emit_held(
        &mut self,
        rel_path: &crate::build::served_path::ServedPath,
        bytes: Vec<u8>,
        bucket: HashBucket,
    ) -> std::io::Result<()> {
        write_at(self.output_dir, rel_path.as_str(), &bytes)?;
        self.manifest.register_held(rel_path, bytes, bucket)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Write `bytes` to `dir.join(rel_path)`, creating parent directories as
/// needed. Routed through `io_utils::write_output` — this is the busiest write
/// into the output tree, so it is the one that most needs ADR-043's rule that a
/// dataless destination is discarded rather than materialized.
fn write_at(dir: &Path, rel_path: &str, bytes: &[u8]) -> std::io::Result<()> {
    crate::build::io_utils::write_output(&dir.join(rel_path), bytes)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::content::SiteHashes;
    use tempfile::tempdir;

    #[test]
    fn for_render_emits_to_single_dir() {
        let dir = tempdir().unwrap();
        let mut manifest = PendingManifest::new(SiteHashes::default());
        let mut ctx = BuildContext::for_render(dir.path(), &mut manifest);

        ctx.emit(&crate::build::served_path::ServedPath::from_source("style.css").unwrap(), b"body{}", HashBucket::Files)
            .unwrap();
        ctx.emit(
            &crate::build::served_path::ServedPath::from_source("og/home.png").unwrap(),
            b"\x89PNG",
            HashBucket::ImageOutputs,
        )
        .unwrap();

        // Files written
        assert!(dir.path().join("style.css").exists());
        assert!(dir.path().join("og/home.png").exists());

        // Manifest registered (drop ctx to release the &mut borrow)
        drop(ctx);
        let sealed = manifest.seal();
        assert!(sealed.files().contains_key("style.css"));
        assert!(sealed.image_outputs().contains("og/home.png"));
        assert!(sealed.blocking_keys().contains("style.css"));
        assert!(sealed.blocking_keys().contains("og/home.png"));
    }

    #[test]
    fn emit_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let mut manifest = PendingManifest::new(SiteHashes::default());
        let mut ctx = BuildContext::for_render(dir.path(), &mut manifest);

        // Deeply nested path — parent dirs don't exist yet
        ctx.emit(
            &crate::build::served_path::ServedPath::from_source("articles/foo/bar/baz/index.html").unwrap(),
            b"<html/>",
            HashBucket::Files,
        )
        .unwrap();

        assert!(dir
            .path()
            .join("articles/foo/bar/baz/index.html")
            .exists());
    }

}
