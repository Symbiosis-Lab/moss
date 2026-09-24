//! Shared assertion for the `SOURCE_ROOTS` scanner tests (raw writes, raw
//! reads, hardlinks, vendor UI launches): fail loudly if a root resolved to
//! zero `.rs` files. A root that silently stopped resolving — renamed,
//! moved, typoed — would otherwise look identical to a passing run: no
//! violations found because nothing was scanned.
//!
//! Twin of the desktop app's `support/scanned_roots_nonempty.rs` — moved
//! here as part of the open-half split of the scanner tests.

/// `scanned` is the count the caller already produced by walking `roots`
/// (each scanner test walks its own way — `walk_rust_files` differs slightly
/// file to file, and some also `assert!(root.exists())` per root while
/// others `continue` — so only the assertion itself, which was pasted
/// identically into all four files, lives here).
pub fn scanned_roots_nonempty(crate_root: &std::path::Path, roots: &[&str], scanned: usize) {
    assert!(
        scanned > 0,
        "SOURCE_ROOTS {:?} resolved to zero .rs files under {} — a root silently \
         stopped resolving would look identical to a passing run",
        roots,
        crate_root.display()
    );
}
