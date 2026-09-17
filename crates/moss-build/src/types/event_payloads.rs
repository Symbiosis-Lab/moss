//! The desktop-shaped `MossEvent` payload vocabulary that crossed from the app
//! crate at S1 of the ADR-067 relocation (2026-08-28, plan decision 4): these
//! types ride `MossEvent` variants, and the event contract lives crate-side so
//! the SSE carrier can publish it. The machinery that PRODUCES each payload —
//! the login probe, the updater, the publish-verification supervisor, the
//! nested-site guard's rendezvous — stays app-side; each old home re-exports
//! its type from here (MIGRATION-STATE shim rows).

use serde::{Deserialize, Serialize};
use specta::Type;

/// The reason the login browser-panel triggered the failure view.
/// Serialized to the frontend via `MossEvent::LoginBrowserFailed`. The probe
/// that produces it stays app-side (`plugins/login_probe.rs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LoginFailureReason {
    /// Both control hosts (1.1.1.1:443, 8.8.8.8:443) unreachable: no internet.
    Offline,
    /// Controls reachable; target TCP RST arrived in < 1 s (likely blocked).
    LikelyRestricted,
    /// Controls reachable; target TCP timed out (≥ 5 s).
    Unreachable,
    /// Controls reachable; TCP connected but TLS handshake failed.
    TlsError,
    /// Network appears fine, but the webview page loaded with an empty body.
    LoadedBlank,
}

/// Payload for `MossEvent::ActionPanelOpened`. The constructors that derive
/// the chrome fields from `PanelOccupant` stay app-side
/// (`plugins/runtime/window.rs`) — the occupant vocabulary is shell chrome.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct ActionPanelOpened {
    /// Logical width of the action-panel webview, in CSS pixels.
    pub width: f64,
    /// Initial title text for the action-panel titlebar (e.g., plugin name).
    pub title: String,
    /// Whether the shell shows the plugin titlebar strip + loading spinner.
    /// From PanelOccupant::shows_plugin_titlebar() (NOT is_below_titlebar —
    /// they diverge for the composer).
    pub shows_plugin_titlebar: bool,
    /// Whether the corner Edit (pencil) affordance hides while this panel is open. From PanelOccupant::hides_edit_affordance().
    pub hides_edit: bool,
    /// Geometry model: the panel sits below the titlebar, so the preview stays
    /// full-window and the content insets from the left (--action-panel-width)
    /// — distinct from shows_plugin_titlebar, which is chrome. browser=true,
    /// editor/composer=false. Drives the InsetLeft vs Tiled preview model in
    /// the frontend's ActionPanelOpened handler.
    pub is_below_titlebar: bool,
    /// Whether the browser-panel is showing a login URL (matters.town/matters.icu
    /// login path). When true, the shell renders the login chrome (identity +
    /// back + close) instead of the generic plugin-title strip. Always false for
    /// non-browser occupants (editor, composer). Design: C1–C4 in
    /// docs/archive/2026-06-23-matters-login-lifecycle-and-minimal-browser-design.md
    pub is_login_browser: bool,
}

/// Classification of a transport-level network failure, for class-specific
/// localized error toasts. The classifier that walks an error's source chain
/// stays app-side (`updater.rs::classify_network_failure`); the enum crosses
/// because `UpdateCheckResult` carries it.
///
/// Serializes snake_case to match the hand-written `NetworkFailureClass` in
/// `frontend/app/bindings.ts` (central `pnpm run bindings` regen is the source
/// of truth and must reproduce this).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum NetworkFailureClass {
    /// No internet at all — DNS lookup failed AND we cannot attribute it to a
    /// specific blocked host. In practice DNS failure is the offline signal.
    Offline,
    /// Name resolution failed (getaddrinfo / public resolver marker matched).
    Dns,
    /// The request timed out (`reqwest::Error::is_timeout`).
    Timeout,
    /// The TCP connection was actively refused / unreachable
    /// (`reqwest::Error::is_connect` with no DNS marker).
    ConnectionRefused,
    /// A TLS/secure-connection failure (handshake / cert).
    Tls,
    /// The host was reachable but the HTTP exchange was blocked/rejected.
    HttpBlocked,
    /// A captive portal is intercepting traffic (login/redirect page). Only the
    /// consolidated diagnosis can detect this; the fast classifier never returns it.
    CaptivePortal,
    /// Could not be classified from the available signals.
    Unknown,
}

/// Payload for `MossEvent::UpdateAvailable`.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct UpdateAvailable {
    pub version: String,
    pub notes: Option<String>,
    /// True if triggered by the user via "Check for Updates..." menu item.
    /// False if discovered by the background check at startup.
    pub manual: bool,
    /// True when the verified installer is already parked on disk, so the
    /// button the user is about to see only has to restart. The background
    /// path announces nothing until this is true (or the pre-download has
    /// failed); a manual check announces immediately and reports whatever the
    /// spool happens to hold.
    pub ready: bool,
}

/// Payload for `MossEvent::UpdateCheckResult`. Discriminated on `kind`.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum UpdateCheckResult {
    /// Manual check completed; no update available.
    UpToDate { version: String },
    /// Manual check failed (network / signature / etc.). The user sees a toast.
    /// `class` carries the transport classification so the frontend can show a
    /// class-specific localized message instead of one generic string.
    Error {
        message: String,
        class: NetworkFailureClass,
    },
}

/// Progress of the installer download, for the frontend's update toast.
///
/// Emitted as `MossEvent::UpdateDownloadProgress`. `content_length` is
/// `Option` because a server may answer without `Content-Length`; the toast
/// then stays indeterminate rather than inventing a percentage.
///
/// Byte counts are `f64`, not `u64`: specta renders `u64` as a JS `string` to
/// stay bigint-safe, and the frontend does arithmetic on these. An `f64` is
/// exact to 2^53 bytes — nine petabytes past any installer.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct UpdateDownloadProgress {
    /// Bytes received so far.
    pub downloaded: f64,
    /// Total bytes, when the server declared one.
    pub content_length: Option<f64>,
    /// True on the final event: the bytes are downloaded AND their signature
    /// verified. What remains is the install, which is not a transfer and has
    /// no percentage.
    ///
    /// Emitted by `ensure_downloaded` after `download()` resolves `Ok`, NOT
    /// from the plugin's `on_download_finish` callback — that callback runs
    /// before `verify_signature` (tauri-plugin-updater 2.10.1
    /// `src/updater.rs:710` vs `:712`), so wiring `done` to it would tell the
    /// frontend "ready to install" about bytes that then fail verification.
    pub done: bool,
    /// The release these bytes install, on the `done` event; `None` on progress
    /// ticks, which carry no release identity.
    ///
    /// Present because "the download finished" is not by itself enough for the
    /// button to promise a restart. A pre-download of 0.9.2 can still be in
    /// flight when a manual check announces 0.9.3; without a version here, that
    /// `done` is indistinguishable from 0.9.3's own, and the button would offer
    /// an instant restart backed by bytes for a different release — which Rust
    /// then correctly treats as a cache miss and re-downloads. The frontend
    /// compares this against the version it is offering.
    pub version: Option<String>,
}

/// One `publish-verdict` wire payload. `state` transitions are the ONLY thing
/// emitted — elapsed is a frontend-local timer, and the mount-time drain pulls
/// the same shape through `get_publish_verdict`. The verification supervisor
/// that produces it stays app-side (`system/stack_serving/verify.rs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PublishVerdict {
    /// Which publish this verdict is about — `handlePublishVerdict`
    /// (`deploy-handler.ts`) dispatches on this instead of reading `state`
    /// values as an implicit target signal (task 3e; `PublishVerdict` had no
    /// target at all until this field, so a moss-hosted `live`/`checking`
    /// fell into OnionPress's branches by default).
    pub target: PublishTarget,
    /// The folder (site root) this verdict is about. A verification burst is
    /// fire-and-forget and can land after the author has switched to a
    /// different folder, so every listener that applies a verdict to visible
    /// state must first check this against whatever folder it is currently
    /// showing and ignore a mismatch — otherwise a stray verdict for the
    /// folder just left behind is indistinguishable from one about the
    /// folder now open.
    pub folder: String,
    pub state: PublishVerdictState,
    /// What produced this verdict: a publish that just landed, or moss
    /// re-checking an already-published folder on open. A resume is not news
    /// — the folder was already published, and this only reconfirms it — so
    /// a listener may update state from it silently but must never toast or
    /// otherwise interrupt on it; only a `Publish`-origin verdict may.
    pub origin: PublishVerdictOrigin,
    pub generation: String,
    pub url: String,
    /// The receiver's last effective reachability code (`"200"`, `"takeover"`,
    /// `"000:rc=28"`, …), or moss's own `"down"` when the receiver itself is
    /// not answering. `None` when there is no fresh verdict at all.
    pub http_code: Option<String>,
    /// Tor's bootstrap percentage, when the receiver reported one — feeds the
    /// "Tor is still connecting ({pct}%)" escalation line.
    pub bootstrap_pct: Option<u8>,
    /// Per-URL verdicts for the page rows the receipt shows (task 4-6,
    /// design §4a extended to page rows), verified in the same burst once
    /// the control probe confirms this generation. Always empty for
    /// OnionPress — the per-URL burst is moss-hosting only — and for a
    /// moss-hosted verdict whose control probe never confirmed (`Offline`,
    /// `Incomplete`): with no confirmed generation there is nothing
    /// trustworthy to say about any one page either.
    pub pages: Vec<PageVerdict>,
}

/// One page row's own post-landing reachability verdict — task 4-6's
/// per-URL burst judges each row by what its own kind promises (design
/// §4a): Added wants the new URL live with matching bytes, Moved wants the
/// old URL still serving the redirect stub, Removed wants the old URL gone.
/// `path` always echoes the row's own `PublishReceiptPage.path` (the URL
/// the receipt already shows for it), never the URL that was actually
/// fetched, so the frontend can match a verdict back to its row by
/// `(kind, path)` alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PageVerdict {
    pub kind: PublishReceiptPageKind,
    pub path: String,
    pub ok: bool,
}

/// The verification state machine's announcable states. `Checking`/`Live`/
/// `Unreachable` are OnionPress's; the other three are moss-hosting's,
/// produced by `classify_moss_verification` (design
/// `docs/archive/2026-09-10-publish-receipt-design.md` §4a's table, as
/// amended by ADR-084 — `Live` is that table's first row, shared rather than
/// duplicated).
/// `rename_all` is `snake_case`, not the old `lowercase`, so `UnreachableHere`
/// serializes as `"unreachable_here"`; every pre-existing variant is a single
/// word, so its wire value is unchanged by the switch.
///
/// There is deliberately no "the public address is serving an older version"
/// state. A CDN that rewrites HTML on the way out makes the served bytes
/// differ from the published bytes on every request, so that state was
/// unfalsifiable from the client and fired on healthy sites (ADR-084).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PublishVerdictState {
    Checking,
    Live,
    Unreachable,
    /// Control probe confirms the new generation; the public fetch itself
    /// failed (timeout, DNS, TLS, a non-2xx answer) — a connection problem on
    /// this computer, not the site.
    UnreachableHere,
    /// The control probe itself is unreachable: this computer looks offline,
    /// and nothing about the site can be concluded.
    Offline,
    /// The control probe reports moss's backend is still serving an older
    /// generation — the publish did not complete.
    Incomplete,
}

/// What armed the verification session a [`PublishVerdict`] reports on: a
/// publish that just landed, or moss re-checking a folder that was already
/// published, done on open so a hosted site's status survives a relaunch or
/// a folder switch. Every transition announced while one arming stands
/// carries that same origin — a `Live`/`Unreachable` that follows a `Resume`
/// arm is itself a `Resume`, because it is answering the same "is this still
/// true" question the resume asked, not a fresh publish event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PublishVerdictOrigin {
    Publish,
    Resume,
}

// ── The threshold screen's wire vocabulary ───────────────────────────────────
// Crossed with `MossEvent::ThresholdPrompt`; mirrors
// `frontend/app/preview/threshold/seam.ts` exactly. The rendezvous (PENDING,
// the ack/decision commands) stays app-side in
// `system/nested_site_guard/wire.rs`, which re-exports these shapes.

/// String-shape classification of the opened root, wire form. Mirrors
/// [`crate::nested_roots::RootClass`]; the screen renders it, it never
/// re-derives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RootClass {
    Normal,
    CloudProviderRoot { provider: String },
    HomeDir,
    SystemSpecial { which: String },
    FilesystemRoot,
}

impl From<&crate::nested_roots::RootClass> for RootClass {
    fn from(class: &crate::nested_roots::RootClass) -> Self {
        use crate::nested_roots;
        match class {
            nested_roots::RootClass::Normal => RootClass::Normal,
            nested_roots::RootClass::CloudProviderRoot { provider } => {
                RootClass::CloudProviderRoot { provider: provider.clone() }
            }
            nested_roots::RootClass::HomeDir => RootClass::HomeDir,
            nested_roots::RootClass::SystemSpecial(which) => {
                RootClass::SystemSpecial { which: which.clone() }
            }
            nested_roots::RootClass::FilesystemRoot => RootClass::FilesystemRoot,
        }
    }
}

/// One nested site the bounded walk found under the opened root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct NestedRootInfo {
    /// '/'-separated, root-relative (build convention).
    pub rel_path: String,
    pub abs_path: String,
    /// Folder basename = the future site name (`VaultRoot::name` semantics).
    pub name: String,
    /// true = published (site_id present) · false = preview-only ·
    /// null = state unreadable (evicted).
    pub published: Option<bool>,
    pub site_id: Option<String>,
}

impl From<&crate::nested_roots::NestedRootInfo> for NestedRootInfo {
    fn from(info: &crate::nested_roots::NestedRootInfo) -> Self {
        NestedRootInfo {
            rel_path: info.rel_path.clone(),
            abs_path: info.abs_path.clone(),
            name: info.name.clone(),
            published: info.published,
            site_id: info.site_id.clone(),
        }
    }
}

/// The detector's report for one opened root, wire form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct NestedRootsReport {
    pub root: String,
    pub root_class: RootClass,
    /// ≤ 5, shallowest-first.
    pub nested: Vec<NestedRootInfo>,
    /// A depth/dir/report cap was hit — the walk may have missed sites.
    pub truncated: bool,
    /// Telemetry, and the live count the checking narration shows.
    pub dirs_visited: u32,
}

impl From<&crate::nested_roots::NestedRootsReport> for NestedRootsReport {
    fn from(report: &crate::nested_roots::NestedRootsReport) -> Self {
        NestedRootsReport {
            root: report.root.clone(),
            root_class: (&report.root_class).into(),
            nested: report.nested.iter().map(Into::into).collect(),
            truncated: report.truncated,
            dirs_visited: report.dirs_visited.min(u32::MAX as usize) as u32,
        }
    }
}

/// Amendment 2's guess, wire form: which side the decision layer believes is
/// the real site. `Nested` names the candidate by absolute path (the wire has
/// no index vocabulary — the screen keys cards by path).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SuggestedSite {
    Outer,
    Nested { abs_path: String },
}

/// Which question the screen asks — see `seam.ts` for the two cases'
/// semantics ('unowned' = rows 4/5, 'owned' = the case-(d) double vault).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ThresholdPromptKind {
    Unowned,
    Owned,
}

/// The one question the guard puts on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ThresholdPrompt {
    /// The guard pass's generation token. The screen's answer (and its ack)
    /// must quote it, so a click on an older prompt can never land in a
    /// newer prompt's slot.
    pub token: u32,
    pub kind: ThresholdPromptKind,
    pub report: NestedRootsReport,
    /// Present only for 'owned' prompts with a confident guess; the screen
    /// makes the suggested card the focused primary. A guess is a default,
    /// never a silent decision.
    pub suggestion: Option<SuggestedSite>,
}

impl ThresholdPrompt {
    /// Rows 4/5: an unowned folder with site(s) inside. No suggestion — the
    /// nested site is already the fast lane in the screen's layout.
    pub fn unowned(report: &crate::nested_roots::NestedRootsReport, token: u32) -> Self {
        ThresholdPrompt {
            token,
            kind: ThresholdPromptKind::Unowned,
            report: report.into(),
            suggestion: None,
        }
    }

    /// Case (d): the double-vault question, led by the decision layer's guess.
    /// The guess arrives already in wire form — the app-side decision layer
    /// maps its `SiteSuggestion` index vocabulary to [`SuggestedSite`] before
    /// calling (that vocabulary stays app-side with the GUI guard).
    pub fn owned(
        report: &crate::nested_roots::NestedRootsReport,
        suggestion: Option<SuggestedSite>,
        token: u32,
    ) -> Self {
        ThresholdPrompt {
            token,
            kind: ThresholdPromptKind::Owned,
            report: report.into(),
            suggestion,
        }
    }
}


/// moss asking for one credential again, on a plugin's behalf.
///
/// Emitted when a plugin calls `rejectSecret`: the stored value is already
/// forgotten by the time this goes out, and the plugin's hook is parked until
/// [`crate::types::events::MossEvent::CredentialRequest`]'s answer comes back.
/// The plugin supplies `key` and `detail` and nothing else — every word that
/// describes the field comes from the manifest, and every pixel from moss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct CredentialRequest {
    /// The parked question's id. The answer must quote it, so a click on a
    /// stale modal cannot resolve a newer question.
    pub token: u32,
    /// Which plugin's store the answer is written to — resolved from the
    /// dispatch seam, never from anything the plugin said.
    pub plugin: String,
    /// Its display name, for the modal's title.
    pub plugin_name: String,
    pub key: String,
    /// From `setup.credentials`, falling back to the key when the plugin
    /// declared none: a field labelled `pinata_jwt` is bad, an unanswerable
    /// modal is worse.
    pub label: String,
    pub help_url: Option<String>,
    /// The plugin's one sentence saying why the old value stopped working.
    /// Rendered as given — moss neither translates it nor trusts it as markup.
    pub detail: Option<String>,
}

/// Where a publish landed. `Other` covers a plugin target moss has no
/// dedicated case for yet — rendered as label + value, never dropped.
///
/// `Deserialize` joined the derive at task 5.4: `get_publish_verdict` grew a
/// `target` parameter so the email composer (a separate webview) can ask for
/// the moss-hosting verdict specifically, instead of always reading the
/// OnionPress supervisor slot — every other use of this type is still
/// outbound-only (it rides `PublishVerdict`/`PublishReceipt` to the
/// frontend), so this is its first inbound crossing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum PublishTarget {
    Moss,
    Onionpress,
    GithubPages,
    Other,
}

/// File-count summary of what a push moved. `pages_*` stay `0` until step 4
/// of the publish-receipt plan backfills them from the change summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PublishReceiptUploaded {
    pub files_uploaded: u32,
    pub files_removed: u32,
    pub pages_added: u32,
    pub pages_updated: u32,
    pub pages_moved: u32,
    pub pages_removed: u32,
}

/// What happened to one page in this publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum PublishReceiptPageKind {
    Added,
    Moved,
    Removed,
}

/// `change_record::PageChangeKind` (the pure merge module's vocabulary) to
/// this wire type. The single conversion both receipt-side consumers —
/// `deploy.rs`'s `page_change_record_to_receipt_page` and `app_seam.rs`'s
/// `PageVerdict` construction — call, so a third `PageChangeKind` variant
/// only needs fixing here. Kept as its own impl rather than folded into one
/// enum: `PageChangeKind` is moss-build's pure merge output and this is the
/// receipt's specta-exported wire type, and collapsing them would let a
/// receipt-shape change ripple into the pure merge module for no reason.
impl From<crate::deploy::change_record::PageChangeKind> for PublishReceiptPageKind {
    fn from(kind: crate::deploy::change_record::PageChangeKind) -> Self {
        use crate::deploy::change_record::PageChangeKind;
        match kind {
            PageChangeKind::Added => PublishReceiptPageKind::Added,
            PageChangeKind::Moved => PublishReceiptPageKind::Moved,
            PageChangeKind::Removed => PublishReceiptPageKind::Removed,
        }
    }
}

/// One row of the receipt's page list. Empty until step 4 backfills it —
/// see `PublishReceipt::from_moss_push`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PublishReceiptPage {
    pub kind: PublishReceiptPageKind,
    pub title: String,
    pub path: String,
    pub old_path: Option<String>,
}

/// The receipt's own reachability verdict for the published URL. Distinct
/// from `PublishVerdictState` (the live-updating toast/panel state machine):
/// this is a snapshot taken once, at receipt-build time — a moss-hosted
/// publish starts at `Checking` (`PublishReceipt::from_moss_push`) and the
/// frontend patches it in place from the `PublishVerdict` event task 3b's
/// burst emits once the check settles (task 3c,
/// `docs/archive/2026-09-10-publish-receipt-design.md` §4a). `Unverifiable`
/// is the only value a non-moss target (or a target step 3 never wires) ever
/// carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PublishReceiptLive {
    Unverifiable,
    Checking,
    Live,
    UnreachableHere,
    Offline,
    Incomplete,
}

/// The armed-article newsletter send this publish carries, when the
/// frontend patched one on (step 5). `None` when no article is armed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PublishReceiptNewsletter {
    pub armed: bool,
    pub subscribers: u32,
}

/// The custom domain this publish is reachable under, when one is
/// configured and folded in (step 6). `None` otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PublishReceiptDomain {
    pub host: String,
    pub confirmed: bool,
}

/// The single record a finished publish hands to the frontend, replacing
/// the `deploy_success_toast`/`announce_deploy_success` pair (step 2 of the
/// publish-receipt plan, `docs/archive/2026-09-10-publish-receipt-design.md`).
/// `generation_id` joins the receipt to the publish-history record (ADR-083)
/// so the two never grow a second, competing "what did I just publish"
/// answer.
#[derive(Debug, Clone, PartialEq, Serialize, Type)] // PartialEq only, not Eq — DeployAddress doesn't derive Eq
pub struct PublishReceipt {
    pub target: PublishTarget,
    pub url: String,
    pub host: String,
    pub first_publish: bool,
    pub generation_id: String,
    pub uploaded: PublishReceiptUploaded,
    pub pages: Vec<PublishReceiptPage>,
    pub live: PublishReceiptLive,
    pub newsletter: Option<PublishReceiptNewsletter>,
    pub domain: Option<PublishReceiptDomain>,
    pub addresses: Vec<crate::config::deployment::DeployAddress>,
}

/// Derive a receipt's `host` from a publish URL — the same reading
/// `deploy_success_toast` used (`src-tauri/src/system/feedback_router.rs:131-137`)
/// and shared by every `PublishReceipt` assembly site (moss-hosted push here,
/// and `plugin_publish_receipt` in `src-tauri/src/preview/commands.rs`) so the
/// two never drift.
pub fn host_from_url(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .split(['/', '?'])
        .next()
        .unwrap_or(url)
        .to_string()
}

impl PublishReceipt {
    /// Assemble the receipt for a completed moss-hosted push. `host` is
    /// derived from `url` via `host_from_url` (see its doc comment) so the two
    /// readings of the same URL never drift while both exist. `live` starts
    /// at `Checking` (task 3b's verification burst always runs after this
    /// commit succeeds, so the receipt never opens on a value the burst is
    /// about to overwrite). `pages`, `newsletter`, `domain` and `addresses`
    /// start at their not-yet-wired defaults — steps 4-6 of the
    /// publish-receipt plan backfill them at this same call site.
    pub fn from_moss_push(
        url: &str,
        files_uploaded: u32,
        files_removed: u32,
        first_publish: bool,
        generation_id: String,
    ) -> Self {
        let host = host_from_url(url);
        Self {
            target: PublishTarget::Moss,
            url: url.to_string(),
            host,
            first_publish,
            generation_id,
            uploaded: PublishReceiptUploaded {
                files_uploaded,
                files_removed,
                pages_added: 0,
                pages_updated: 0,
                pages_moved: 0,
                pages_removed: 0,
            },
            pages: Vec::new(),
            live: PublishReceiptLive::Checking,
            newsletter: None,
            domain: None,
            addresses: Vec::new(),
        }
    }
}

#[cfg(test)]
mod publish_receipt_tests {
    use super::*;

    #[test]
    fn from_moss_push_derives_host_from_url() {
        let receipt = PublishReceipt::from_moss_push(
            "https://okagaki.mosspub.com/some/page?x=1",
            3,
            1,
            true,
            "gen-42".to_string(),
        );

        assert_eq!(receipt.host, "okagaki.mosspub.com");
        assert_eq!(receipt.url, "https://okagaki.mosspub.com/some/page?x=1");
        assert_eq!(receipt.uploaded.files_uploaded, 3);
        assert_eq!(receipt.uploaded.files_removed, 1);
        assert!(receipt.first_publish);
        assert_eq!(receipt.generation_id, "gen-42");
        // Task 3c: the moss target's receipt now opens in Checking, not
        // Unverifiable — task 3b's verification burst always runs after a
        // successful commit, so nothing is left permanently unverifiable.
        assert_eq!(receipt.live, PublishReceiptLive::Checking);
        assert!(receipt.pages.is_empty());
    }
}
