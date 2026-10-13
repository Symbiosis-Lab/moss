//! Toast payload types for `MossEvent::ShowToast` (ex `system/utils.rs`,
//! M5a landing 2). 03-module-tree is silent on these; they are typed-event
//! payloads, so they sit beside the event surface until M7a re-homes chrome.

/// A link rendered INSIDE the toast's message, on the message's own line —
/// for a toast whose sentence ends in a thing you can open ("Published to
/// sample-site.mosspub.com"). An action button would put the same words on a
/// second row and say them twice.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, specta::Type)]
pub struct ToastInlineLink {
    /// The link's visible text — usually the thing itself (a host, a filename).
    pub label: String,
    /// Opened via the host runtime (system browser, or Tor for an onion URL).
    pub url: String,
}

/// Toast action button (for toasts that surface a follow-up command).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, specta::Type)]
pub struct ToastAction {
    pub label: String,
    /// What the button does. `open-url:<url>` opens that link; anything else
    /// is a key the host app resolves against its own fixed list of actions,
    /// so a toast can only ask for what the host already knows how to do.
    pub event: String,
    /// How much weight the button carries. None (the default) is a plain
    /// follow-up link.
    #[serde(default)]
    pub variant: Option<ToastActionVariant>,
}

/// Visual weight of a [`ToastAction`], for a toast that offers a choice: the
/// one thing it asks the user to do, and what else they could do instead.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ToastActionVariant {
    Primary,
    Secondary,
}

impl ToastAction {
    pub fn new(label: impl Into<String>, event: impl Into<String>) -> Self {
        Self { label: label.into(), event: event.into(), variant: None }
    }

    pub fn with_variant(mut self, variant: ToastActionVariant) -> Self {
        self.variant = Some(variant);
        self
    }
}

/// Payload for `MossEvent::ShowToast` (and `ShowToastUpdate`, with non-null `id`).
///
/// Step 3 §12b R1: **a toast is an Advisory rendered ephemerally** — there is no
/// private toast severity enum. The visual tier derives from the shared
/// [`Severity`](crate::advisory::Severity) value (`ShippedDegraded`→info,
/// `NeedsAction`→warning, `Blocking`→error). A pure confirmation ("Sent",
/// "Site is live!") is NOT an advisory — it is the design's `Ack`; set
/// `ack: true` and the frontend renders it success-styled via `showAck`.
///
/// Fields mirror `ToastAdvisory` + the options bag in
/// the desktop app's toast manager. Per architecture decision 9:
/// `Option<T>` serializes as
/// `T | null` required, not `T?` optional. The TS listener accepts `null` the
/// same as `undefined`.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, specta::Type)]
pub struct ToastPayload {
    /// What happened — the toast's message text (the advisory's `what`).
    pub what: String,
    /// Recovery effort; drives the icon/class. Ignored when `ack` is true.
    pub severity: crate::advisory::Severity,
    /// True for a pure confirmation (`Ack`) — rendered success-styled, ignoring
    /// `severity`. False (default) for an advisory.
    pub ack: bool,
    /// Action buttons. None or empty for plain toasts.
    pub actions: Option<Vec<ToastAction>>,
    /// A link on the message's own line — see [`ToastInlineLink`].
    #[serde(default)]
    pub inline_link: Option<ToastInlineLink>,
    /// Auto-dismiss duration in ms. None for persistent.
    pub duration: Option<i64>,
    /// If true, the auto-dismiss countdown only advances while the moss window
    /// is visible. A confirmation that lands while the user is in another app is
    /// therefore still on screen when they come back, instead of having expired
    /// unwitnessed — which is the failure `persistent` used to be set to avoid,
    /// at the cost of a toast that never leaves.
    ///
    /// Deliberately NOT gated on focus: moss's shell hosts sibling child
    /// webviews (action panel, plugin browser), and one of them taking focus
    /// blurs the shell while the toast is still fully visible to the user.
    #[serde(default)]
    pub await_attention: Option<bool>,
    /// If true, no auto-dismiss timer (overrides `duration`).
    pub persistent: Option<bool>,
    /// If false, hide the X close button (default: true).
    pub dismissible: Option<bool>,
    /// Stable id — required for `ShowToastUpdate` and toast deduplication.
    pub id: Option<String>,
    /// Overrides the icon with a small colored status dot — for a two-phase
    /// "pending → confirmed" flow (send logs, check for updates) where the
    /// generic checkmark can't distinguish "still working" from "done".
    /// None (the default) keeps the existing check/triangle icon logic; a
    /// one-shot `ack` toast ("Copied", "Site published") should leave this
    /// unset.
    #[serde(default)]
    pub dot: Option<ToastDot>,
    /// A progress bar under the message, for a toast that reports work with a
    /// measurable end (a download). `0`–`100` is a determinate fill; a
    /// negative value is indeterminate, for work whose total is not known
    /// yet. None (the default) draws no bar, and on a `ShowToastUpdate`
    /// removes one.
    #[serde(default)]
    pub progress: Option<i32>,
}

/// Colored status dot for [`ToastPayload::dot`]. Yellow = pending/in-flight,
/// green = confirmed — the pair a two-phase toast flow cycles through.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ToastDot {
    Yellow,
    Green,
}

impl ToastPayload {
    /// An advisory toast at the given recovery `severity`.
    pub fn advisory(what: impl Into<String>, severity: crate::advisory::Severity) -> Self {
        Self {
            what: what.into(),
            severity,
            ack: false,
            actions: None,
            inline_link: None,
            duration: None,
            await_attention: None,
            persistent: None,
            dismissible: None,
            id: None,
            dot: None,
            progress: None,
        }
    }

    /// A pure confirmation (`Ack`) toast — success-styled, no recovery effort.
    pub fn ack(what: impl Into<String>) -> Self {
        Self {
            what: what.into(),
            // Severity is ignored when `ack` is true; pick the quietest tier.
            severity: crate::advisory::Severity::ShippedDegraded,
            ack: true,
            actions: None,
            inline_link: None,
            duration: None,
            await_attention: None,
            persistent: None,
            dismissible: None,
            id: None,
            dot: None,
            progress: None,
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Render with a colored status dot instead of the default icon — see
    /// [`ToastDot`].
    pub fn with_dot(mut self, dot: ToastDot) -> Self {
        self.dot = Some(dot);
        self
    }

    /// Draw a progress bar — see [`ToastPayload::progress`].
    pub fn with_progress(mut self, percent: i32) -> Self {
        self.progress = Some(percent);
        self
    }

    pub fn with_duration(mut self, ms: i64) -> Self {
        self.duration = Some(ms);
        self
    }

    pub fn persistent(mut self) -> Self {
        self.persistent = Some(true);
        self
    }

    /// Put `label` on the message's own line as a link to `url`.
    pub fn with_inline_link(mut self, label: impl Into<String>, url: impl Into<String>) -> Self {
        self.inline_link = Some(ToastInlineLink { label: label.into(), url: url.into() });
        self
    }

    /// Hold the auto-dismiss countdown while the window is away — see
    /// [`ToastPayload::await_attention`].
    pub fn await_attention(mut self) -> Self {
        self.await_attention = Some(true);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A payload written before `progress` and `variant` existed still reads,
    /// with no bar and plain buttons.
    #[test]
    fn a_payload_without_the_newer_fields_still_deserializes() {
        let json = r#"{
            "what": "Sent", "severity": "ShippedDegraded", "ack": true,
            "actions": [{ "label": "Open", "event": "open-url:https://example.com" }],
            "duration": null, "persistent": null, "dismissible": null, "id": null
        }"#;
        let toast: ToastPayload = serde_json::from_str(json).expect("an older payload");
        assert_eq!(toast.progress, None);
        assert_eq!(toast.actions.unwrap()[0].variant, None);
    }

    /// The wire names a renderer matches on.
    #[test]
    fn progress_and_button_weight_cross_the_wire_as_a_number_and_a_lowercase_word() {
        let toast = ToastPayload {
            actions: Some(vec![ToastAction::new("Install", "install").with_variant(ToastActionVariant::Primary)]),
            ..ToastPayload::ack("Downloading").with_progress(-1)
        };
        let json = serde_json::to_value(&toast).unwrap();
        assert_eq!(json["progress"], -1);
        assert_eq!(json["actions"][0]["variant"], "primary");
        assert_eq!(json["actions"][0]["event"], "install");
    }
}
