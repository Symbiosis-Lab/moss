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
    /// A specific content/asset file (carries `item: Some(filename)`).
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
/// item · what · action-as-data) so the frontend can render `item` (a
/// filename) with `what` on hover, branch the build-wide case
/// (`item: None`, e.g. "FFmpeg not available") into a distinct inline row,
/// and offer a recovery affordance driven by `action`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[derive(specta::Type)]
pub struct Advisory {
    /// Which axis of the system this advisory is about.
    pub scope: Scope,
    /// How serious it is — drives the surface tier.
    pub severity: Severity,
    /// The item this advisory is about — usually a filename. `None` for a
    /// build-wide advisory that isn't tied to a single file.
    pub item: Option<String>,
    /// What happened, shown on hover (per-file) or inline (build-wide).
    pub what: String,
    /// The recovery affordance, expressed as data.
    pub action: Action,
}

impl Advisory {
    /// One file did not ship, and there is nothing the author can do from the
    /// UI about it. `item` is a path; only its file name is shown.
    pub(crate) fn blocking_file(item: &str, what: String) -> Self {
        let name = std::path::Path::new(item)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| item.to_string());
        Self {
            scope: Scope::File,
            severity: Severity::Blocking,
            item: Some(name),
            what,
            action: Action::None,
        }
    }
}
