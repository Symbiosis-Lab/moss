//! The per-user moss directory: `$MOSS_HOME` if set, else `~/.moss`.

use std::path::PathBuf;

/// Distinct from [`crate::moss_paths::MossPaths`], which is per-PROJECT
/// (`<site>/.moss`). This is the one directory moss's own tooling keeps
/// beside a user's home regardless of which site they have open — `bin/`
/// (downloaded helper binaries), `assets/` (cached build-tool downloads),
/// `stacks/` (installed deploy stacks), and `locks/` (cross-process build
/// coordination). Every caller that used to spell
/// `dirs::home_dir().join(".moss")` by hand routes through here so
/// `$MOSS_HOME` overrides all of them at once, the way `MOSS_STACKS_ROOT`
/// already overrides `stacks/` alone for tests that need a root that is not
/// the developer's own `~/.moss`.
pub fn moss_home() -> Result<PathBuf, String> {
    if let Some(over) = std::env::var_os("MOSS_HOME") {
        return Ok(PathBuf::from(over));
    }
    dirs::home_dir()
        .map(|h| h.join(".moss"))
        .ok_or_else(|| "Cannot determine home directory".to_string())
}

/// Serializes every test in the crate that reads or writes `$MOSS_HOME` —
/// env vars are process-global and cargo runs tests in parallel by default,
/// so two tests each doing their own save/set/restore race each other
/// without this. `pub(crate)` because `infra::folder_lock`'s tests set the
/// same var and must take the same lock, not a lock of their own.
#[cfg(test)]
pub(crate) static MOSS_HOME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Points `MOSS_HOME` at a fresh temp dir for the duration of the closure and
/// restores the prior value, holding [`MOSS_HOME_TEST_LOCK`] throughout. Any
/// test that takes a lock under `~/.moss/locks` goes through this, so it never
/// writes into the developer's real home.
#[cfg(test)]
pub(crate) fn with_moss_home<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
    let _guard = MOSS_HOME_TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let prior = std::env::var_os("MOSS_HOME");
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("MOSS_HOME", tmp.path());
    let result = f(tmp.path());
    match prior {
        Some(v) => std::env::set_var("MOSS_HOME", v),
        None => std::env::remove_var("MOSS_HOME"),
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `$MOSS_HOME` overrides `~/.moss` for every caller that used to spell
    /// `dirs::home_dir().join(".moss")` by hand — `stacks_root`,
    /// `get_moss_assets_dir`, `get_moss_bin_dir`, and the folder build lock.
    /// Takes [`MOSS_HOME_TEST_LOCK`] because `infra::folder_lock`'s tests
    /// mutate the same process-global var.
    #[test]
    fn moss_home_honours_the_env_var_override() {
        let _guard = MOSS_HOME_TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let prior = std::env::var_os("MOSS_HOME");

        std::env::remove_var("MOSS_HOME");
        assert_eq!(
            moss_home().unwrap(),
            dirs::home_dir().unwrap().join(".moss"),
            "unset MOSS_HOME must fall back to ~/.moss"
        );

        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_var("MOSS_HOME", tmp.path());
        assert_eq!(moss_home().unwrap(), tmp.path());

        match prior {
            Some(v) => std::env::set_var("MOSS_HOME", v),
            None => std::env::remove_var("MOSS_HOME"),
        }
    }
}
