//! Whether a plugin may run at all, asked where plugins are loaded.
//!
//! Three things can refuse one: the registry's kill list (`revoked.json` names
//! id+version pairs that must not run), the manifest's `min_moss_version`
//! floor, and the user not yet having allowed a plugin that arrived with the
//! folder rather than being installed here. All three verdicts are the app's
//! to give — the kill list arrives over the network and caches in app data,
//! the running app's version is not this crate's version, and consent is
//! recorded in app data — so the app installs one verdict function here at
//! startup and this module is only the seam.
//!
//! "Must not run" has to be enforced inside [`super::discovery::load_plugin`]
//! rather than at its callers. A filter applied to
//! `discover_installed_plugins`'s result would miss the path that matters
//! most: the QuickJS engine re-resolves a plugin on EVERY hook dispatch,
//! through `manager::bundle_source_for_project` straight to `load_plugin`,
//! never through the discovery list. So a plugin refused mid-session stops at
//! its next *hook dispatch* rather than surviving on a cached handle.
//!
//! Two paths re-enter plugin code without re-resolving it, and a mid-session
//! refusal does not reach them until the engine is restarted: an event
//! subscription (`Job::DeliverEvent` in `crate::engine` calls the
//! handler inside the already-built Context) and a plugin-authored panel
//! served at `moss-plugin://`, which outlives the dispatch that opened it.
//! Closing those means dropping the cached Context when a refresh withdraws
//! something, which is engine lifecycle work rather than a check at this seam.
//!
//! Nothing installed means nothing refused — the honest answer for a process
//! that never fetched the list, which is every test binary. The app installs
//! at setup; a headless build of either binary installs in
//! `run_headless_build` (ADR-077).

use std::path::Path;
use std::sync::OnceLock;

use super::types::PluginManifest;

/// Answers "may this plugin run?", with the sentence to show the user when the
/// answer is no. A bare `fn` pointer rather than a boxed closure: there is one
/// implementation and it owns its own process-wide state.
///
/// It takes the whole manifest and the directory it was read from, because
/// the refusal has more than one ground. Narrowing it to id+version once cost
/// a second seam when the version floor needed enforcing at the same point;
/// the directory is what the consent record is keyed by, and what says
/// whether these are the exact bytes moss shipped.
pub type AdmissionCheck = fn(&PluginManifest, &Path) -> Option<String>;

static CHECK: OnceLock<AdmissionCheck> = OnceLock::new();

/// Install the app's verdict function.
///
/// The first caller wins and later ones are dropped, so a second run-mode
/// wiring itself up cannot replace an installed check with a laxer one.
pub fn install_check(check: AdmissionCheck) {
    let _ = CHECK.set(check);
}

/// Why this plugin must not run, in a sentence fit to show a user, or `None`.
pub fn refusal(manifest: &PluginManifest, plugin_dir: &Path) -> Option<String> {
    CHECK.get().and_then(|check| check(manifest, plugin_dir))
}
