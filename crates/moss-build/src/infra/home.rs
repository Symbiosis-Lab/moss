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
    #[cfg(test)]
    if let Some(home) = THREAD_HOME.with(|home| home.borrow().clone()) {
        return Ok(home);
    }
    resolve(std::env::var_os("MOSS_HOME"), default_home())
}

#[cfg(test)]
thread_local! {
    static THREAD_HOME: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Points this thread's [`moss_home`] at `home` until the guard drops, without
/// touching `$MOSS_HOME`: a parallel test never sees it, and it never sees
/// theirs. Only for a test whose home is read on its own thread (a
/// current-thread runtime); a build's worker threads keep the process's home.
#[cfg(test)]
pub(crate) fn thread_moss_home(home: &std::path::Path) -> impl Drop {
    struct Restore(Option<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            THREAD_HOME.with(|home| *home.borrow_mut() = self.0.take());
        }
    }
    Restore(THREAD_HOME.with(|slot| slot.borrow_mut().replace(home.to_path_buf())))
}

/// The home folder the operating system lists for this user, read from the
/// account database rather than `$HOME`.
#[cfg(unix)]
pub fn account_home() -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut buf = vec![0u8; 4096];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: `buf` outlives every read of `pwd`, and `found` is only read after the call.
    let rc = unsafe { libc::getpwuid_r(libc::geteuid(), &mut pwd, buf.as_mut_ptr().cast(), buf.len(), &mut found) };
    if rc != 0 || found.is_null() || pwd.pw_dir.is_null() { return None; }
    let dir = unsafe { std::ffi::CStr::from_ptr(pwd.pw_dir) };
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes())))
}

#[cfg(not(unix))]
pub fn account_home() -> Option<PathBuf> { dirs::home_dir() }

fn resolve(over: Option<std::ffi::OsString>, default: Option<PathBuf>) -> Result<PathBuf, String> {
    match over {
        Some(over) => Ok(PathBuf::from(over)),
        None => default.ok_or_else(|| "Cannot determine home directory".to_string()),
    }
}

#[cfg(not(test))]
fn default_home() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".moss"))
}

/// A unit test that never set `$MOSS_HOME` still must not write the developer's
/// `~/.moss` (build locks land there on every build, and a sandboxed test run
/// cannot write it at all), so it gets one scratch home per test process. It ends
/// in `.moss` like the real one, so path-shape assertions hold.
#[cfg(test)]
fn default_home() -> Option<PathBuf> {
    static HOME: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    Some(HOME.get_or_init(|| std::env::temp_dir().join(format!("moss-test-home-{}", std::process::id())).join(".moss")).clone())
}

/// Serializes the tests that set or clear `$MOSS_HOME` ([`with_moss_home`],
/// [`without_moss_home`]) with those that need it left alone: the variable is
/// process-global. A test whose home is read only on its own thread uses
/// [`thread_moss_home`] instead and needs no lock.
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
    let home = scratch_moss_home();
    std::env::set_var("MOSS_HOME", &home);
    let result = f(&home);
    match prior {
        Some(v) => std::env::set_var("MOSS_HOME", v),
        None => std::env::remove_var("MOSS_HOME"),
    }
    result
}

/// A fresh directory for a test to point `MOSS_HOME` at, beside this process's
/// scratch home and never deleted while the process runs. The variable is
/// process-wide, so a build in a parallel test that holds no lock can read the
/// override just before it is restored and open its folder lock there
/// afterwards; deleting the directory under that open failed the other test
/// with "Invalid argument".
#[cfg(test)]
pub(crate) fn scratch_moss_home() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = default_home().unwrap().with_file_name(format!("override-{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs the closure with `MOSS_HOME` unset under [`MOSS_HOME_TEST_LOCK`], for
/// tests that assert the shape of the default home and would otherwise see
/// another test's override.
#[cfg(test)]
pub(crate) fn without_moss_home<T>(f: impl FnOnce() -> T) -> T {
    let _guard = MOSS_HOME_TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let prior = std::env::var_os("MOSS_HOME");
    std::env::remove_var("MOSS_HOME");
    let result = f();
    if let Some(v) = prior {
        std::env::set_var("MOSS_HOME", v);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_account_home_comes_from_the_account_database_not_the_environment() {
        let home = account_home().expect("this user has a home folder");
        assert!(home.is_absolute() && home.is_dir(), "{home:?}");
    }

    #[test]
    fn moss_home_is_the_override_else_the_default() {
        let home = PathBuf::from("/home/someone/.moss");
        assert_eq!(resolve(None, Some(home.clone())).unwrap(), home);
        assert_eq!(resolve(Some("/elsewhere".into()), Some(home)).unwrap(), PathBuf::from("/elsewhere"));
        assert!(resolve(None, None).is_err());
    }

    #[test]
    fn moss_home_honours_the_env_var_override() {
        with_moss_home(|tmp| assert_eq!(moss_home().unwrap(), tmp));
    }

    #[test]
    fn a_thread_home_is_this_threads_alone_and_ends_with_its_guard() {
        let home = scratch_moss_home();
        {
            let _home = thread_moss_home(&home);
            assert_eq!(moss_home().unwrap(), home);
            assert_ne!(std::thread::spawn(|| moss_home().unwrap()).join().unwrap(), home, "another thread keeps the process's home");
        }
        assert_ne!(moss_home().unwrap(), home);
    }

    #[test]
    fn an_overridden_home_outlives_the_override() {
        let home = with_moss_home(|home| home.to_path_buf());
        assert!(home.is_dir(), "a parallel build may still be opening its lock under {home:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_test_build_leaves_the_developers_home_alone() {
        use crate::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
        let site = tempfile::tempdir().unwrap();
        std::fs::write(site.path().join("index.md"), "---\ntitle: Orchard\n---\n\nPlum trees.\n").unwrap();
        run_pipeline(PipelineConfig {
            root: crate::vault::paths::VaultRoot::resolve(site.path()),
            progress: crate::build::null_sink(), plugins: PluginMode::Skip,
            watch: false, start_server: false,
            host: crate::build::ports::host::test_host_ports(),
            trigger: BuildTrigger::Full, reason: None, exits_after_build: true,
            site_url_override: None, server_port: None, admission_epoch: None, live_port: None, rename_pairs: Vec::new(),
        }).await.expect("fixture builds");
        let id = crate::infra::folder_store::name_for(site.path());
        let real_locks = dirs::home_dir().unwrap().join(".moss").join("locks");
        let leaked: Vec<_> = std::fs::read_dir(&real_locks).into_iter().flatten().flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(&id)).map(|e| e.path()).collect();
        assert!(leaked.is_empty(), "a test build wrote the developer's home: {leaked:?}");
    }
}
