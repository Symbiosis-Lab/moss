//! One recursive copy for every test that stages a fixture into a temp dir.
//! Four private twins of this function lived in the test tree before it
//! (2026-09-05, B1 review); the normalizing copies in `snapshot_tests.rs` are
//! different functions and stay there. One twin, `pipeline_correctness.rs`'s,
//! skipped `.moss` and `.gitkeep`; that skip is gone, so that suite now builds
//! every fixture it stages from its checked-in `.moss/` like the other two
//! suites always did — the config is what a fresh build writes anyway, and the
//! one `state.toml` (bilingual-lang-tree-site) feeds nothing its tests read.

use std::path::Path;

pub fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &dst_path)?;
        } else {
            std::fs::copy(&path, &dst_path)?;
            // `fs::copy` stamps the destination with the current time, not the
            // source's mtime. Two calls a moment apart (e.g. parity tests that
            // stage the same fixture into two temp dirs) then see different
            // mtimes for byte-identical files — and any build code that folds
            // a source mtime into a content hash (og_card.rs's cover-image
            // hash, at least) produces two different hashes for the same
            // input. Preserve the source's mtime so both copies agree.
            let src_mtime = std::fs::metadata(&path)?.modified()?;
            // Read-only open: `fs::copy` carries the source's permission bits
            // onto the destination, so a 0o444 source yields a 0o444 copy.
            // `set_modified` (futimens under the hood) only needs an open fd,
            // not a writable one — opening `.write(true)` here fails
            // PermissionDenied on exactly that file and aborts the whole
            // recursive copy.
            std::fs::File::open(&dst_path)?.set_modified(src_mtime)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::copy_dir_recursive;
    use std::time::{Duration, SystemTime};

    #[test]
    fn copy_preserves_source_mtime() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src_dir = tmp.path().join("src");
        let dst_dir = tmp.path().join("dst");
        std::fs::create_dir_all(&src_dir).expect("create src");
        let src_file = src_dir.join("file.txt");
        std::fs::write(&src_file, b"content").expect("write src file");

        // A fixed past instant, far enough back that "the copy just happened
        // to run at the same wall-clock second" can't produce a false pass.
        let fixed = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
        std::fs::File::open(&src_file)
            .expect("open src file")
            .set_modified(fixed)
            .expect("set src mtime");

        // Read-only source: `fs::copy` carries permission bits onto the
        // copy, so the destination is 0o444 too. Preserving mtime must not
        // need to WRITE the destination — opening it `.write(true)` before
        // `set_modified` fails PermissionDenied on exactly this file and
        // aborts the whole recursive copy.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&src_file, std::fs::Permissions::from_mode(0o444))
                .expect("chmod src read-only");
        }

        copy_dir_recursive(&src_dir, &dst_dir).expect("copy_dir_recursive");

        let dst_file = dst_dir.join("file.txt");
        let dst_mtime = std::fs::metadata(&dst_file)
            .expect("stat dst file")
            .modified()
            .expect("dst mtime");
        assert_eq!(
            dst_mtime, fixed,
            "copy_dir_recursive must carry the source file's mtime onto the copy"
        );
        assert_eq!(
            std::fs::read(&dst_file).expect("read dst file"),
            b"content",
            "the copy must exist and be readable even when the source was read-only"
        );
    }
}
