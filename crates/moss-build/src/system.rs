//! The tauri-free members of the app's `system` family that the build tree
//! and the plugin runtime reach: per-folder session state, system-proxy
//! resolution, the resumable verified download, the single-shot `fetch_text`/
//! `fetch_bytes` pair `moss desktop install` uses for the updater manifest and
//! the release tarball, the outbound `User-Agent`, and "Show in Finder"
//! ([`reveal`], which two carriers ask for). The rest of the family (windows,
//! menus, dialogs, commands) stays app-side; the reqwest adapter over
//! [`proxy`] stays there too, so this crate carries no reqwest.
pub mod bounded_command;
pub mod build_records;
pub mod folder_session;
pub mod large_download;
pub mod proxy;
pub mod proxy_reqwest;
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
}
