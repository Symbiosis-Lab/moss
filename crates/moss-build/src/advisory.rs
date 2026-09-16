//! The unified advisory value objects — the recovery vocabulary shared by
//! every Job producer (Build, Domain, Deploy, plugins). Pure values, zero I/O
//! (Hickey: model values, decomplect producer/state/rendering/recovery).
//!
//! Moved here from `build::progress` in Step 3 so producers outside `build`
//! (Domain, Deploy, plugins) can speak the model without depending on the
//! build pipeline, then into `moss-build` (2026-08-27, M6a B3) because the
//! pipeline is the chief producer and must reach the vocabulary after the
//! crate move. Pure values with no `crate::` reference of their own, so the
//! move is a relocation and nothing more.
//!
//! `PartialEq` is derived on every type here (it was absent on the originals)
//! so adapters and smart-constructor tests can compare advisories by value.

use serde::{Deserialize, Serialize};

/// Where an advisory lives — which axis of the system it's about. Lets the
/// frontend group/branch advisories by their domain (a missing file vs. a
/// missing tool vs. an account problem) without parsing free text.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[derive(specta::Type)]
pub enum Scope {
    /// A specific content/asset file (carries `item: Some(site_relative_path)`).
    File,
    /// The project/site configuration.
    Config,
    /// The local machine/environment (e.g. a missing tool like FFmpeg).
    Environment,
    /// A remote target (deploy host, DNS, etc.).
    Remote,
    /// The user's account/subscription.
    Account,
}

/// How serious an advisory is — drives the surface's tier (a bare dot vs. an
/// actionable banner vs. a blocking stop).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[derive(specta::Type)]
pub enum Severity {
    /// The build shipped, but this thing was not done optimally (e.g. a video
    /// published full-size). Non-blocking, informational.
    ShippedDegraded,
    /// The user should do something to fully resolve this (e.g. install a
    /// tool, re-check DNS), but the build still produced output.
    NeedsAction,
    /// The operation could not proceed — nothing shipped for this item.
    Blocking,
}

/// A closed set of in-app operations an `Action::InApp` can request. Build
/// only ever emits `Action::None` / `Action::Command`, but the full set is
/// defined here so other producers (deploy, account) share one vocabulary.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[derive(specta::Type)]
pub enum AppOp {
    /// Move/relocate a file to resolve the advisory.
    MoveFile,
    /// Open the billing/subscription surface.
    OpenBilling,
    /// Prompt the user to sign in.
    SignIn,
    /// Re-run a DNS check for a custom domain.
    RecheckDns,
}

/// The recovery affordance for an advisory, expressed as data (no rendering
/// logic on the model — the frontend maps each variant to a control). Per
/// §12b R1 there is intentionally **no** `display` field.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[derive(specta::Type)]
pub enum Action {
    /// Nothing to do — informational advisory.
    None,
    /// A shell command the user can copy/run (e.g. `brew install ffmpeg`).
    Command {
        /// The command text.
        run: String,
        /// Label for the affordance (e.g. "Copy").
        label: String,
    },
    /// An in-app operation the frontend can dispatch.
    InApp {
        /// Which operation to run.
        op: AppOp,
        /// Operation arguments as free-form JSON (shape is op-specific).
        args: serde_json::Value,
        /// Label for the affordance.
        label: String,
    },
    /// Nothing to repair — moss already acted, irreversibly, and the author's
    /// part is to look at the result and then say she has. The affordance
    /// dismisses the notice.
    ///
    /// It is also what keeps the notice on screen. Every other advisory
    /// describes a *condition of this build*, so a rebuild re-derives it and
    /// the panel clears the lot at build-start; this one describes an *act*,
    /// which a rebuild does not answer and cannot repeat. So an `Acknowledge`
    /// advisory is sticky: it survives the build-start clear and leaves only
    /// by being acknowledged. Without that, a notice asking her to check
    /// something disappears the moment she saves a file — possibly before she
    /// has read it, and with no way to bring it back (2026-08-30).
    Acknowledge {
        /// Label for the affordance.
        label: String,
    },
    /// An external link to open. `href` is a plain `String` (not `url::Url`)
    /// — specta + `Url` is unused elsewhere in the contract.
    Link {
        /// The URL to open.
        href: String,
        /// Label for the affordance.
        label: String,
    },
}

/// A soft, non-blocking advisory from a background task — something the
/// pipeline encountered but did not fully process this build (e.g. a video
/// that failed to transcode and was published unoptimized). Replaces the
/// old free-form `warnings: Vec<String>`: structured (scope · severity ·
/// item · what · action-as-data) so the frontend can render `item` with
/// `what` on hover, branch the build-wide case (`item: None`, e.g. "FFmpeg
/// not available") into a distinct inline row, and offer a recovery
/// affordance driven by `action`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[derive(specta::Type)]
pub struct Advisory {
    /// Which axis of the system this advisory is about.
    pub scope: Scope,
    /// How serious it is — drives the surface tier.
    pub severity: Severity,
    /// The item this advisory is about: the SITE-ROOT-RELATIVE path of the
    /// source file, exactly as it sits under the open folder on disk — never
    /// a bare filename, and never a page-tree/URL path reshaped by a `url:`
    /// override. `None` for a build-wide advisory that isn't tied to a single
    /// file. The frontend's click-to-open resolves `item` by joining it onto
    /// the open folder (`resolveAgainstFolder`), so a value that isn't the
    /// real on-disk path opens nothing. Every moss-internal producer sets it
    /// via [`Advisory::for_source`] (or [`Advisory::blocking_file`], which
    /// delegates to it) — a producer that hand-writes `item: Some(...)`
    /// without checking [`is_site_relative`] first has almost certainly
    /// narrowed a site-relative path down to something that no longer
    /// resolves. `clamp_plugin_advisory` is the one legitimate exception: it
    /// starts from a plugin-supplied `Option<String>` rather than a bare
    /// path, so it applies `is_site_relative` itself instead of going
    /// through `for_source`.
    pub item: Option<String>,
    /// What happened, shown on hover (per-file) or inline (build-wide).
    pub what: String,
    /// The recovery affordance, expressed as data.
    pub action: Action,
}

/// Could `path` resolve under the site root when a caller joins it onto the
/// open folder — a relative path with no `..` component? This is the whole
/// contract [`Advisory::item`] must satisfy. Shared by `for_source` (a moss
/// producer handing over the wrong shape is a bug, but these advisories are
/// built in release too, so the wrong shape is dropped rather than trusted)
/// and `clamp_plugin_advisory` (a plugin handing over the wrong shape is
/// untrusted input to drop, in every build) — one predicate, not two.
pub(crate) fn is_site_relative(path: &str) -> bool {
    let p = std::path::Path::new(path);
    !p.is_absolute() && !p.components().any(|c| matches!(c, std::path::Component::ParentDir))
}

impl Advisory {
    /// Build an advisory about one source file. `source_path` becomes `item`
    /// when it is site-relative — see the field doc on [`Advisory::item`]. A
    /// path that fails [`is_site_relative`] (stripped to a bare filename,
    /// handed over absolute, or escaping the site root via `..`) drops to
    /// `item: None` instead of being stored. That rejection is
    /// unconditional, not a `debug_assert!`: these advisories are produced
    /// in release builds on users' machines, so a check that only ran in
    /// dev would be compiled out exactly where a malformed path does
    /// damage — a build-wide advisory with no item is always a safe
    /// fallback.
    pub(crate) fn for_source(
        scope: Scope,
        severity: Severity,
        source_path: &str,
        what: String,
        action: Action,
    ) -> Self {
        let item = is_site_relative(source_path).then(|| source_path.to_string());
        Self { scope, severity, item, what, action }
    }

    /// One file did not ship, and there is nothing the author can do from the
    /// UI about it. `item` is the file's own site-relative path.
    pub(crate) fn blocking_file(item: &str, what: String) -> Self {
        Self::for_source(Scope::File, Severity::Blocking, item, what, Action::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_source_keeps_a_site_relative_path() {
        let advisory = Advisory::for_source(
            Scope::File,
            Severity::Blocking,
            "posts/2026/photo.jpg",
            "decode failed".into(),
            Action::None,
        );
        assert_eq!(advisory.item.as_deref(), Some("posts/2026/photo.jpg"));
    }

    // The rejection must hold with no `debug_assert!` to strip: these
    // advisories are built in release binaries on users' machines, which is
    // exactly where the bug this guards against reached the frontend.
    #[test]
    fn for_source_drops_a_path_escaping_the_site_root() {
        let advisory = Advisory::for_source(
            Scope::File,
            Severity::Blocking,
            "../outside/photo.jpg",
            "decode failed".into(),
            Action::None,
        );
        assert_eq!(advisory.item, None);
    }

    #[test]
    fn for_source_drops_an_absolute_path() {
        let advisory = Advisory::for_source(
            Scope::File,
            Severity::Blocking,
            "/etc/passwd",
            "decode failed".into(),
            Action::None,
        );
        assert_eq!(advisory.item, None);
    }
}
