//! The tauri-free members of the app's `system` family that the build tree
//! and the plugin runtime reach: per-folder session state, system-proxy
//! resolution, the resumable verified download, the single-shot `fetch_text`/
//! `fetch_bytes` pair `moss desktop install` uses for the updater manifest and
//! the release tarball, the outbound `User-Agent`, and "Show in Finder"
//! ([`reveal`], which two carriers ask for), and a bounded wait for blocking
//! work ([`deadline`]). The rest of the family (windows,
//! menus, dialogs, commands) stays app-side; the reqwest adapter over
//! [`proxy`] stays there too, so this crate carries no reqwest.
pub mod bounded_command;
pub mod deadline;
pub mod derive;
pub mod build_records;
pub mod folder_session;
pub mod large_download;
pub mod proxy;
pub mod proxy_reqwest;
pub mod redirects;
pub mod reveal;
pub mod stack_exec;
pub mod tar_safe;

use std::sync::OnceLock;

/// The instant, in the exact form JS `new Date().toISOString()` produces
/// (`YYYY-MM-DDTHH:MM:SS.sssZ`): the recents list and the registry cache both
/// stamp with it, and the frontend reads both back with `Date`.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

static APP_VERSION: OnceLock<&'static str> = OnceLock::new();
/// Name the moss that owns this process. The app seeds its `CARGO_PKG_VERSION`
/// at startup — the version users see, and the one a plugin's
/// `min_moss_version` is a floor on — because this crate's own version is a
/// different number. A second seed is ignored; the first binary to speak wins.
pub fn seed_app_version(version: &'static str) {
    let _ = APP_VERSION.set(version);
}

/// `debug` or `release`: a debug moss builds sites many times slower, so a
/// build record names which one timed it.
pub fn build_profile() -> &'static str {
    if cfg!(debug_assertions) { "debug" } else { "release" }
}

/// The running moss's version, as seeded. Debug builds — every test binary
/// in the workspace links this crate without `cfg(test)` — fall back to this
/// crate's own number rather than making each test module seed. A release
/// binary gets no fallback: one that forgot to seed would hold every
/// `min_moss_version` floor to the wrong version and send it in every
/// `User-Agent`, so there the miss is a startup-order bug and panics as one.
pub fn app_version() -> &'static str {
    #[cfg(debug_assertions)]
    return APP_VERSION.get().copied().unwrap_or(env!("CARGO_PKG_VERSION"));
    #[cfg(not(debug_assertions))]
    APP_VERSION
        .get()
        .copied()
        .expect("seed_app_version runs at startup, before the first plugin check or request")
}

static APP_NAME: OnceLock<&'static str> = OnceLock::new();
/// Name the moss binary that owns this process, as a sign-in request shows it
/// to the person who reads the link. The app is plain `moss`; the command line
/// seeds its own name at startup.
pub fn seed_app_name(name: &'static str) {
    let _ = APP_NAME.set(name);
}

/// The running moss's name, as seeded; `moss` when nothing was.
pub fn app_name() -> &'static str {
    APP_NAME.get().copied().unwrap_or("moss")
}

/// Every answer [`device_kind`] can give. A fixed list, so a sign-in request
/// can say what kind of computer asked without ever carrying the name its
/// owner gave it, which often holds the owner's own name.
pub const DEVICE_KINDS: &[&str] = &[
    "MacBook Air", "MacBook Pro", "MacBook", "Mac mini", "Mac Studio", "Mac Pro", "iMac", "a Mac",
    "a Windows PC", "a Linux computer",
];

/// What kind of computer this is, one of [`DEVICE_KINDS`]; `None` on a
/// system with no entry there.
pub fn device_kind() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some(mac_kind(&mac_model().unwrap_or_default()))
    } else if cfg!(windows) {
        Some("a Windows PC")
    } else if cfg!(target_os = "linux") {
        Some("a Linux computer")
    } else {
        None
    }
}

/// The Mac family a model name or identifier names ("Mac Studio (2023)",
/// "MacBookPro16,1"), else "a Mac".
pub(crate) fn mac_kind(model: &str) -> &'static str {
    let compact = |s: &str| s.replace(' ', "").to_ascii_lowercase();
    let model = compact(model);
    DEVICE_KINDS
        .iter()
        .filter(|kind| !kind.starts_with("a "))
        .find(|kind| model.starts_with(&compact(kind)))
        .copied()
        .unwrap_or("a Mac")
}

/// The device tree's product name (Apple silicon: "Mac Studio (2023)" on the
/// `/product` node), else
/// the `hw.model` identifier (Intel: "MacBookPro16,1").
#[cfg(target_os = "macos")]
fn mac_model() -> Option<String> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::data::CFData;
    use core_foundation::string::CFString;
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IORegistryEntryFromPath(main_port: u32, path: *const std::ffi::c_char) -> u32;
        fn IORegistryEntryCreateCFProperty(entry: u32, key: core_foundation_sys::string::CFStringRef, allocator: *const std::ffi::c_void, options: u32) -> core_foundation_sys::base::CFTypeRef;
        fn IOObjectRelease(object: u32) -> i32;
    }
    // SAFETY: a valid C path; port 0 is the default main port; a zero entry is "none".
    let entry = unsafe { IORegistryEntryFromPath(0, c"IODeviceTree:/product".as_ptr()) };
    let from_tree = (entry != 0).then(|| {
        let key = CFString::from_static_string("product-name");
        // SAFETY: a live entry and key; the result is +1 (the Create rule) or null.
        let value = unsafe { IORegistryEntryCreateCFProperty(entry, key.as_concrete_TypeRef(), std::ptr::null(), 0) };
        // SAFETY: the entry came from IORegistryEntryFromPath and is released once.
        unsafe { IOObjectRelease(entry) };
        // SAFETY: non-null and owned by us.
        let value = (!value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })?;
        let bytes = value.downcast::<CFData>()?;
        Some(String::from_utf8_lossy(bytes.bytes()).trim_end_matches('\0').to_string())
    });
    from_tree.flatten().filter(|m| !m.is_empty()).or_else(|| {
        let mut buf = [0u8; 64];
        let mut len = buf.len();
        // SAFETY: the buffer is writable for `len` bytes and `len` is updated in place.
        let ok = unsafe { libc::sysctlbyname(c"hw.model".as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) } == 0;
        ok.then(|| String::from_utf8_lossy(&buf[..len]).trim_end_matches('\0').to_string())
    })
}

#[cfg(not(target_os = "macos"))]
fn mac_model() -> Option<String> {
    None
}

/// Whether the running moss meets a `min_moss_version` floor (a site's or a
/// plugin's). `None` when either side is not a version.
pub fn meets_floor(floor: &str) -> Option<bool> {
    floor_met(floor, app_version())
}

/// Compared on `major.minor.patch` only: develop carries the number of the
/// release it will become with a `-dev` suffix, so a `0.15.15-dev` build has
/// what a `0.15.15` floor asks for, though semver orders it below `0.15.15`.
pub fn floor_met(floor: &str, host: &str) -> Option<bool> {
    Some(version_core(host)? >= version_core(floor)?)
}

/// `major.minor.patch` of a semver string, the part floors compare.
pub fn version_core(v: &str) -> Option<(u64, u64, u64)> {
    semver::Version::parse(v.trim()).ok().map(|v| (v.major, v.minor, v.patch))
}

/// The `User-Agent` on every request moss makes to its own backend and to the
/// plugin registry: `moss/<version> (+https://mosspub.com)`. One string for
/// the seta client, the reachability probe, the registry client and the
/// artifact download, because two literals once let one be hardened while
/// the other kept sending the bot signal. Built per call — requests are rare
/// — so it follows the seed instead of freezing whatever version it first saw.
/// The scrape pipeline's `moss-import/…` agent stays separate on purpose: it
/// identifies the importer to third-party sites, a different actor.
pub fn user_agent() -> String {
    format!("moss/{} (+https://mosspub.com)", app_version())
}

#[cfg(test)]
mod tests {
    #[test]
    fn now_iso_matches_js_to_iso_string_format() {
        let now = super::now_iso();
        let re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$").unwrap();
        assert!(re.is_match(&now), "now_iso() produced {now}");
    }

    #[test]
    fn a_dev_build_meets_the_floor_of_the_release_it_becomes() {
        use super::floor_met;
        assert_eq!(floor_met("0.15.15", "0.15.15-dev"), Some(true));
        assert_eq!(floor_met("0.15.15", "0.15.14"), Some(false));
        assert_eq!(floor_met("0.15.15", "0.16.0"), Some(true));
        assert_eq!(floor_met("0.15.16", "0.15.15-dev"), Some(false));
        assert_eq!(floor_met("soon", "0.15.15"), None);
    }
}
