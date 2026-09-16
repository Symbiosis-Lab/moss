//! PanelTask — moss's unified task/progress primitive.
//!
//! See [ADR-015](../../../../docs/decisions/ADR-015-panel-task-primitive.md)
//! for the design rationale (Layer 1 internal primitive + Layer 2 plugin
//! contract). This module owns Layer 1 only — the in-process types, the
//! per-`(WindowId, TaskScope)` registry, the plugin-task router, and the
//! wire-snapshot type that flows to frontend renderers via `MossEvent`.
//! Layer 2 (plugin-facing TS API, Tauri command bridge) lands in T8a per
//! the onboarding dispatch plan.
//!
//! ## Wire bridge (added in T2, 2026-05-28)
//!
//! T1 deliberately deferred the frontend event bridge because `PanelTask`
//! contains `Instant` (not `Serialize`). T2 needs *some* event source for
//! the Ambient renderer, so we advance the deferred scope by one step: a
//! sibling `PanelTaskWire` struct strips the non-serializable fields and
//! becomes the payload of `MossEvent::PanelTaskUpdate`. The registry
//! itself owns an optional emitter installed via `set_emitter` (and
//! removed via `clear_emitter`) at startup; mutating methods on
//! `TaskHandle` re-emit a fresh wire snapshot to the frontend after
//! every state change. The breadcrumb subscriber in
//! `frontend/app/editor/breadcrumb-hairline.ts` filters on
//! `tone == "ambient"` && `scope == "action_panel"` and updates DOM
//! accordingly; preview-scope progress is handled by the progress panel
//! (`frontend/app/preview/progress-panel.ts`).
//!
//! Pure-Rust callers (tests, non-Tauri code paths) keep using
//! `TaskRegistry::spawn` / `with_progress`, which carry no emitter and
//! mutate the registry without emitting — same behavior as before T2.
//!
//! The router (`route_plugin_task`) is exhaustive over the closed
//! `(PluginHook, TriggerContext)` cross product — 24 rows, no wildcard.
//! Adding a `PluginHook` or `TriggerContext` variant is a compile-time
//! event that requires explicit routing decisions for every combination.
//!
//! `TaskRegistry` is keyed by `(WindowId, TaskScope)` so multi-window
//! safety is built in from day one, even though current moss has only one
//! main window. Aggregation methods feed the four renderers:
//!
//! - Ambient hairline reads `max_running_fraction()`
//! - Narrated titlebar reads `latest_narrated()`
//! - Awaiting pulse reads `any_awaiting()`
//! - Toast subscriber reads `failed_unrecoverable()`

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::advisory::{Action, Advisory, AppOp, Scope, Severity};
use crate::plugins::types::{PluginHook, TriggerContext};

// ─────────────────────── Job value objects (Step 3 R2) ─────────────────────

/// The past-tense outcome a Job reports: "Built", "Published", "Deployed",
/// "Connected", "Sent", "Synced", "Prepared", or a plugin-supplied verb.
/// A `Verb(String)` newtype (§12b R2) — core constants below, open tail for
/// plugins — never a closed per-producer enum. moss normalizes plugin verbs
/// (R13): a plugin proposes the word, moss owns capitalization/length/glyphs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Verb(pub String);

impl Verb {
    pub const BUILT: &'static str = "Built";
    pub const PUBLISHED: &'static str = "Published";
    pub const DEPLOYED: &'static str = "Deployed";
    pub const CONNECTED: &'static str = "Connected";
    pub const SENT: &'static str = "Sent";
    pub const SYNCED: &'static str = "Synced";
    pub const PREPARED: &'static str = "Prepared";
    /// The pre-verification publish terminal: bytes committed, live verdict
    /// pending. A receipt may only say [`Verb::PUBLISHED`] once the site is
    /// verified serving this publish (publish-verified-live §2.6).
    pub const UPLOADED: &'static str = "Uploaded";

    /// A core verb from a `&'static str` constant above.
    pub fn core(s: &'static str) -> Self {
        Verb(s.to_string())
    }

    /// Normalize a plugin-supplied verb (R13): strip leading AND trailing
    /// non-alphanumerics (emoji/punctuation — e.g. a "🚀 syndicated!!!" spoof),
    /// trim, capitalize the first letter, clamp to 24 chars. moss reserves
    /// accent/amber/glyphs for core — a plugin contributes only the word, never
    /// its own shouting punctuation.
    ///
    /// The invisible strip runs FIRST and over the whole string: trimming the
    /// ends cannot reach an override sitting between two letters.
    pub fn normalized(raw: &str) -> Self {
        // The two `trim()` calls that used to bracket this were redundant —
        // whitespace is not alphanumeric, so `trim_matches` already took it.
        let cleaned: String = moss_core::untrusted_text::stripped(raw)
            .trim_matches(|c: char| !c.is_alphanumeric())
            .chars()
            .take(24)
            .collect();
        let mut chars = cleaned.chars();
        let normalized = match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        };
        Verb(normalized)
    }
}

/// The quantity a Job reports on success: "142 pages", "3 posts". Present iff
/// the run reached `Done` (§4 invariant #4). The renderer formats `count` +
/// `noun`; never a pre-baked string (Hickey §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Amount {
    pub count: u64,
    pub noun: String,
}

// ──────────────────────────── identifiers ─────────────────────────────────

/// Window identifier for multi-window task isolation. Wraps a `String` so
/// callers can pass either a Tauri `WebviewWindow::label()` (already
/// `&str`) or a synthetic id (tests, future launcher window).
///
/// A newtype is preferred over a bare `String` alias because tasks key
/// their registry by `(WindowId, TaskScope)` — the type system catches
/// accidental swaps with other string identifiers (site_id, label, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct WindowId(pub String);

impl WindowId {
    pub fn new(s: impl Into<String>) -> Self {
        WindowId(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for WindowId {
    fn from(s: &str) -> Self {
        WindowId(s.to_string())
    }
}

impl From<String> for WindowId {
    fn from(s: String) -> Self {
        WindowId(s)
    }
}

/// Monotonic per-process task identifier. Wraps a `u64` from an atomic
/// counter (preferred over `Uuid`: tasks live in-process, IDs are used as
/// HashMap keys and for log correlation only — no cross-process routing).
///
/// **Wire format (the load-bearing reason this is a newtype, not a bare
/// `u64`):** specta types Rust `u64` as a TS `string` (JS numbers lose
/// precision above 2^53). The frontend `startTask()` handle therefore
/// carries the id as a *string* and echoes that string back on every
/// subsequent `progress`/`succeeded`/… call. A bare `u64` command parameter
/// then fails to deserialize the JSON string `"1"` (`invalid type: string
/// "1", expected u64`), which crashed every plugin `task.progress()`/terminal
/// call and aborted the whole hook. So this type:
///   - **Serializes to a string** (matches what the frontend reads as the id),
///   - **Deserializes from a string OR a number** (accepts the specta-string
///     `"1"` that comes back as a command argument, and stays robust to a raw
///     number), and
///   - **types as `String` in specta** (so the generated binding keeps the
///     `taskId: string` shape the frontend already uses).
/// Internally it is still a plain `u64` for arithmetic / map keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub u64);

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for TaskId {
    /// Emit as a string so the frontend (which reads ids as strings, since
    /// specta stringifies `u64`) round-trips the value without precision loss.
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for TaskId {
    /// Accept either a JSON string (`"1"` — the specta-stringified form the
    /// frontend sends back) or a JSON number (`1` — robustness for any
    /// non-specta caller).
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = TaskId;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a task id as a u64 or a decimal string")
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<TaskId, E> {
                Ok(TaskId(v))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<TaskId, E> {
                u64::try_from(v)
                    .map(TaskId)
                    .map_err(|_| E::custom("task id out of range"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<TaskId, E> {
                v.parse::<u64>()
                    .map(TaskId)
                    .map_err(|_| E::custom(format!("invalid task id string: {v:?}")))
            }
        }
        d.deserialize_any(V)
    }
}

impl specta::Type for TaskId {
    /// Type as `String` in generated bindings — `u64` already renders as a TS
    /// `string` in specta, so this keeps the existing `taskId: string` shape
    /// the frontend already uses. Delegate to `String`'s own definition.
    fn inline(
        type_map: &mut specta::TypeMap,
        generics: specta::Generics,
    ) -> specta::DataType {
        <String as specta::Type>::inline(type_map, generics)
    }
}

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

/// Mint a fresh `TaskId`. Monotonic for the lifetime of the process.
/// Exposed for tests; production code reaches `TaskRegistry::spawn` instead.
pub fn next_task_id() -> TaskId {
    TaskId(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
}

// ──────────────────────────── core enums ──────────────────────────────────

/// Lifecycle state of a `PanelTask`. The state machine is:
///
/// ```text
///   spawn → Running → Awaiting → Running → Succeeded
///             │           │         │     ↘ Failed
///             │           │         ↘ Cancelled
///             ↘ Succeeded / Failed / Cancelled
/// ```
///
/// `Awaiting` transitions back to `Running` on the next non-terminal
/// progress event (ADR-015: "no explicit resumed() method"). Terminal
/// states (Succeeded, Failed, Cancelled) are final.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TaskState {
    Running {
        fraction: Option<f32>,
        message: Option<String>,
    },
    /// Paused awaiting an out-of-band user action (sign in, click email
    /// link, accept device pairing). Distinct from Running because the
    /// system is idle and the user has work to do elsewhere.
    ///
    /// `action` (Step 3 R3, typed) is the recovery affordance the Awaiting
    /// renderer paints next to the directive. `Action::None` is a bare
    /// cancel; `InApp`/`Command`/`Link` carry a labelled recovery control.
    /// Replaces the legacy stringly `EscapeAction` grammar (deleted Phase 6).
    Awaiting {
        directive: String,
        action: Action,
    },
    /// Terminal: success. `receipt` is the human-readable result ("Saved",
    /// "Imported 12 files"); `advisories` carry done-with-advisories
    /// (invariant #2). Empty `advisories` = non-surfacing success (#6).
    Succeeded {
        receipt: Option<String>,
        #[serde(default)]
        advisories: Vec<Advisory>,
    },
    /// Terminal: failure. `advisory` is the root cause (invariant #3) — it
    /// carries everything the renderer and toast subscriber need: `what` (the
    /// human message, formerly `error`), `severity` (a `Blocking` advisory is
    /// the unrecoverable case that fires the toast — formerly `recoverable:
    /// false`), `scope`, and the recovery `action`. There is no separate
    /// `error`/`recoverable` field: every failure IS an advisory (design §9).
    Failed {
        advisory: Advisory,
    },
    /// Terminal: explicit user cancellation.
    Cancelled,
}

impl TaskState {
    /// Terminal-success smart constructor enforcing §4 invariant #1: if ANY
    /// advisory is `Blocking`, the run is `Failed` with the first blocking
    /// advisory as root cause (a "succeeded-but-blocking" Job is
    /// unrepresentable). Otherwise `Succeeded` carrying the advisories
    /// (invariant #2; empty = non-surfacing success, #6).
    pub fn done(receipt: Option<String>, advisories: Vec<Advisory>) -> Self {
        if let Some(blocking) = advisories
            .iter()
            .find(|a| matches!(a.severity, Severity::Blocking))
            .cloned()
        {
            return TaskState::Failed { advisory: blocking };
        }
        TaskState::Succeeded {
            receipt,
            advisories,
        }
    }

    /// Failure smart constructor enforcing §4 invariant #3: the `Failed` state
    /// carries the root-cause `advisory` directly (recoverability and the
    /// human message are projections of `advisory.severity` / `advisory.what`).
    pub fn failed_with(advisory: Advisory) -> Self {
        TaskState::Failed { advisory }
    }
}

/// Synthesize the root-cause [`Advisory`] for a failure that has only a bare
/// `error` string and a `recoverable` flag (the unstructured failure path —
/// editor saves, `with_progress`, the plugin/dev fallback). Every failure IS
/// an advisory (design §9), so the bare path builds one: `what` from the
/// message, `severity` from recoverability (unrecoverable → `Blocking`, the
/// case the toast subscriber surfaces; recoverable → `NeedsAction`), a neutral
/// `Config` scope (no file/account axis is implied by a bare error), and
/// `Action::None` (the caller offers no structured recovery control —
/// re-running the operation is the implicit retry).
pub fn advisory_from_error(what: impl Into<String>, recoverable: bool) -> Advisory {
    Advisory {
        scope: Scope::Config,
        severity: if recoverable {
            Severity::NeedsAction
        } else {
            Severity::Blocking
        },
        item: None,
        what: what.into(),
        action: Action::None,
    }
}

/// Map the plugin / dev-harness stringly escape grammar
/// (`"cancel" | "resend:<label>" | "recheck:<label>"`) to a typed [`Action`]
/// at the producer boundary. The plugin SDK still speaks the string dialect
/// (same "plugin proposes, moss types it" pattern as `PluginAdvisory`);
/// internal `TaskState::Awaiting` only ever carries the typed `Action`.
///
/// - `"cancel"` / unknown → `Action::None` (a bare cancel — conservative
///   default so a plugin typo can never wedge the UI without an escape hatch).
/// - `"recheck:<label>"` → `Action::InApp { op: RecheckDns, label }` (the
///   re-check-an-out-of-band-state affordance).
/// - `"resend:<label>"` → `Action::InApp { op: SignIn, label }` (re-trigger
///   the awaited action; routed through the in-app op table).
pub fn escape_spec_to_action(spec: &str) -> Action {
    match spec.split_once(':') {
        Some(("recheck", label)) => Action::InApp {
            op: AppOp::RecheckDns,
            args: serde_json::Value::Null,
            label: label.to_string(),
        },
        Some(("resend", label)) => Action::InApp {
            op: AppOp::SignIn,
            args: serde_json::json!({ "reason": "resend" }),
            label: label.to_string(),
        },
        _ => Action::None,
    }
}

/// Where a `PanelTask` surfaces in the UI. Combined with `WindowId`, it
/// keys the `TaskRegistry` — multi-window safety is built in from day one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskScope {
    /// Action panel (left webview): editor, file tree, save badges.
    ActionPanel,
    /// Preview (main webview): build, deploy, publish progress.
    Preview,
    /// Workspace-wide background work: imports, syndications, scheduled jobs.
    Workspace,
}

/// Which renderer subscribes to this task. See ADR-015 § "Four UI
/// surfaces". `Receipt` was merged into `Inline` per round-3 review —
/// the behavior split is the per-task `fade_after` field, not a separate
/// tone variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskTone {
    /// Hairline at the panel/preview edge — high-frequency, no text in
    /// chrome (tooltip on hover reveals numerals).
    Ambient,
    /// Status anchored adjacent to the affected element. Both terminal
    /// receipts (`fade_after: Some(30s)`) and persistent live status
    /// (`fade_after: None`) use this tone.
    Inline,
    /// Titlebar status thread (the `moss-titlebar-plugin-section` slot).
    Narrated,
    /// Titlebar status thread + hollow-ring pulse + escape affordance.
    /// Only for out-of-band waits (NOT when the user's action surface
    /// is rendered in the panel below them).
    Awaiting,
}

/// Operation kind. **Closed enum, no `Custom` variant** — internal moss
/// code picks one explicitly; plugin tasks derive `TaskKind` from
/// `PluginHook` via `From<PluginHook>`. Adding a variant is a deliberate
/// platform decision, not a free-for-all per the WordPress
/// `admin_notices` failure mode (ADR-015).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    // ── Internal-only ─────────────────────────────────────────────────
    Save,
    Validate,
    Lint,
    Format,
    Resolve,
    Build,
    Rebuild,
    AssetTransform,
    // ── Pluggable (matches PluginHook 1:1) ────────────────────────────
    Import,
    Publish,
    Deploy,
    Syndicate,
    Process,
}

impl From<PluginHook> for TaskKind {
    fn from(h: PluginHook) -> TaskKind {
        match h {
            PluginHook::Import => TaskKind::Import,
            PluginHook::Publish => TaskKind::Publish,
            PluginHook::Deploy => TaskKind::Deploy,
            PluginHook::Syndicate => TaskKind::Syndicate,
            PluginHook::Process => TaskKind::Process,
        }
    }
}

// ─────────────────────────── PanelTask struct ─────────────────────────────

/// A single tracked task. In-memory only — `started_at: Instant` is not
/// `Serialize`, so PanelTask is not a wire type. The Layer-2 event payload
/// (emitted via Tauri to renderers) is a separate snapshot struct added
/// in T6 / T8a; this struct stays internal to the registry.
#[derive(Debug, Clone)]
pub struct PanelTask {
    pub id: TaskId,
    pub window_id: WindowId,
    pub scope: TaskScope,
    pub kind: TaskKind,
    /// Human-readable task title ("Saving index.md", "Importing 42 files
    /// from Matters"). `None` on spawn — set via `TaskHandle::title(...)`
    /// or supplied implicitly by `TaskRegistry::with_progress(..., label, _)`.
    /// Narrated and Inline renderers display the title; Ambient may show
    /// it in tooltip-on-hover. Per ADR-015 § "Internal moss code calls
    /// PanelTask directly" the title is part of the fluent spawn chain:
    /// `ctx.tasks(scope).spawn(kind, tone).title("…")`.
    pub title: Option<String>,
    /// For `Inline` terminal-state receipts ("Saved · 2s ago" =
    /// `fade_after: Some(30s)`). `None` for persistent live status.
    pub fade_after: Option<Duration>,
    /// The past-tense outcome for the receipt header (Step 3 R2). `None` until
    /// a producer sets it; the renderer falls back to deriving from `kind`
    /// during migration.
    pub verb: Option<Verb>,
    /// The success quantity (R2); present **iff** terminal `Succeeded`
    /// (invariant #4). **Private (R-b)** — settable ONLY by the `done_with`
    /// terminal transition, never by a free field-set on a `Running` task.
    /// Read via `PanelTask::amount`.
    amount: Option<Amount>,
    /// The frozen measured span at terminal (R-a, design §3) — a `Duration`,
    /// NOT a formatted "2.1s". Distinct from the live-ticking `started_at_ms`
    /// on the wire (which keeps measuring for fade logic); captured once at the
    /// terminal transition and is what the receipt header renders.
    elapsed: Option<Duration>,
    /// Parent Job for sub-jobs (media under Build, Step 3 Phase 4).
    pub parent: Option<TaskId>,
    pub started_at: Instant,
    pub state: TaskState,
}

/// Ubiquitous-language alias (Evans §4): the model's name is `Job`. `PanelTask`
/// is retained as the in-memory struct name during the Step 3 migration; the
/// rename is a post-Step-3 cleanup. New code should read this as "a Job".
pub type Job = PanelTask;

impl PanelTask {
    fn new(id: TaskId, window_id: WindowId, scope: TaskScope, kind: TaskKind) -> Self {
        PanelTask {
            id,
            window_id,
            scope,
            kind,
            title: None,
            fade_after: None,
            verb: None,
            amount: None,
            elapsed: None,
            parent: None,
            started_at: Instant::now(),
            state: TaskState::Running {
                fraction: None,
                message: None,
            },
        }
    }

    /// Read the success quantity. The only writer is `done_with` (invariant #4).
    pub fn amount(&self) -> Option<&Amount> {
        self.amount.as_ref()
    }

    /// Read the frozen terminal span (R-a). The only writer is the terminal
    /// transition; the renderer formats this into the receipt.
    pub fn elapsed(&self) -> Option<Duration> {
        self.elapsed
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            TaskState::Succeeded { .. } | TaskState::Failed { .. } | TaskState::Cancelled
        )
    }

    pub fn is_running(&self) -> bool {
        matches!(self.state, TaskState::Running { .. })
    }

    pub fn is_awaiting(&self) -> bool {
        matches!(self.state, TaskState::Awaiting { .. })
    }

    /// The renderer surface this Job belongs to — **computed, never stored**
    /// (Step 3 Phase 6 R5; design §3 "tone/surface/display become functions
    /// the renderer computes"). Replaces the old stored `tone` field: a Job is
    /// a producer-agnostic value, and the *window* owns presentation.
    ///
    /// Precedence (most-specific first), reproducing what `route_plugin_task`
    /// and the internal producers encoded into the old stored field:
    ///
    /// 1. **Awaiting state → `Awaiting`.** The one interrupt — it needs *her*
    ///    action. State-driven so the moment a Job enters `Awaiting` the wire
    ///    tone flips and the awaiting renderer takes ownership (inline-status
    ///    drops its badge on the same flip — see `inline-status.ts` §"Tone
    ///    escalation"). This subsumes `route_plugin_task`'s `awaiting_user`
    ///    escalation (which forced `Awaiting` at spawn) AND the mid-flight
    ///    `handle.awaiting(...)` transition.
    /// 2. **Child Job (`parent` set) → `Ambient`.** Media sub-jobs (images /
    ///    videos under the Build parent) live and die at the hairline; success
    ///    makes no sound. Matches `spawn_child`'s producers.
    /// 3. **Otherwise, `(scope, kind)` picks the resting tone**, per the ADR-015
    ///    Layer-2 table and the internal `spawn` producers. Where the router had
    ///    a trigger-dependent collision on a `(scope, kind)` pair (e.g. Import
    ///    is Ambient under OnboardingFlow/Background but Inline/Narrated under
    ///    ManualOne/SettingsManual), moss picks the dominant resting tone — the
    ///    trigger is a spawn-time context the Job value deliberately does not
    ///    retain.
    pub fn tone(&self) -> TaskTone {
        // (1) Awaiting is state-driven and outranks everything —
        // EXCEPT Syndicate jobs, which always surface on their rail lamp
        // (Inline) even while awaiting the user in the matters room.
        // The Awaiting window thread is for blocking operations the user
        // must resolve before work can continue; a matters room is a
        // non-blocking editor the user closes on their own time.
        if self.is_awaiting() {
            if matches!(self.kind, TaskKind::Syndicate) {
                return TaskTone::Inline;
            }
            return TaskTone::Awaiting;
        }
        // (2) A child Job is always Ambient (hairline-only).
        if self.parent.is_some() {
            return TaskTone::Ambient;
        }
        // (3) Resting tone from (scope, kind), reproducing the route_plugin_task
        // table + the internal `spawn` producers. The router keyed tone on
        // `(hook, trigger)`; the Job value does not retain `trigger`, so where a
        // `(scope, kind)` pair was trigger-split moss picks the resting tone that
        // the surviving surface expects (documented per arm below).
        use TaskKind::*;
        use TaskScope::*;
        match (self.scope, self.kind) {
            // Internal editor lifecycle: status anchored to the element.
            (ActionPanel, Save | Validate | Lint | Format) => TaskTone::Inline,

            // Build / resolve / asset transforms are background-class hairline.
            (_, Build | Rebuild | Resolve | AssetTransform | Process) => {
                TaskTone::Ambient
            }

            // Publish / Deploy on the Preview surface anchor adjacent to the
            // publish UI (the ADR Inline rows; deploy.rs producer + the domain
            // ACL). The orchestrator's domain Job rides here too — its Awaiting
            // email-verify state is caught by rule (1) above.
            (Preview, Publish | Deploy) => TaskTone::Inline,

            // Workspace Import is the titlebar narrator's source: the ADR
            // `(Import, SettingsManual)` Narrated row. The router's
            // `(Import, Background)` Ambient case shares this `(scope, kind)`
            // and is now narrated; the live import plugin (Matters) runs
            // OnboardingFlow → `(ActionPanel, Import)`, not this pair, so the
            // narrator stays the dominant meaning here.
            (Workspace, Import) => TaskTone::Narrated,

            // Onboarding/manual import + syndicate in the action panel: hairline
            // for import (ADR `(Import, OnboardingFlow)` Ambient — the Matters
            // hairline), inline for the syndicate rows.
            (ActionPanel, Import) => TaskTone::Ambient,
            (ActionPanel, Syndicate) => TaskTone::Inline,

            // Remaining Workspace jobs (Publish/Deploy/Syndicate background) sit
            // at the hairline per the ADR Background rows.
            (Workspace, _) => TaskTone::Ambient,

            // Any remaining Preview-scoped internal work is ambient.
            (Preview, _) => TaskTone::Ambient,

            // Remaining ActionPanel kinds (Publish/Deploy if ever routed there).
            (ActionPanel, _) => TaskTone::Inline,
        }
    }
}

// ─────────────────────── plugin task router ───────────────────────────────

/// Pure input to `route_plugin_task` — everything the router needs to pick
/// (scope, kind, tone). Plugins emit semantically (`hook + trigger +
/// awaiting?`); moss decides the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginTaskSignal {
    pub hook: PluginHook,
    pub trigger: TriggerContext,
    pub has_progress: bool,
    pub cancellable: bool,
    pub awaiting_user: bool,
}

/// Routes a plugin task signal to `(scope, kind, tone)`. Exhaustive over
/// the closed `(PluginHook, TriggerContext)` cross product — 24 rows, no
/// wildcard arm. Adding a variant of either enum is a compile-time event
/// that requires explicit routing decisions for every combination.
///
/// **Tone-selection rules:**
/// - `awaiting_user = true` escalates the tone to `Awaiting` but
///   **preserves** the original scope. A Deploy task awaiting DNS
///   propagation surfaces on the Preview (where the deploy lives), not
///   the action panel.
/// - All other `(hook, trigger)` pairs map per the table below.
///
/// See ADR-015 § "Layer 2 — plugin contract".
pub fn route_plugin_task(signal: PluginTaskSignal) -> (TaskScope, TaskKind, TaskTone) {
    use PluginHook::*;
    use TriggerContext::*;

    let kind: TaskKind = signal.hook.into();

    // Base (scope, tone) from (hook, trigger). Exhaustive: 6 hooks × 4
    // triggers = 24 rows. No wildcard — adding a variant fails compile.
    let (scope, tone) = match (signal.hook, signal.trigger) {
        (Import, OnboardingFlow) => (TaskScope::ActionPanel, TaskTone::Ambient),
        (Import, SettingsManual) => (TaskScope::Workspace, TaskTone::Narrated),
        (Import, Background) => (TaskScope::Workspace, TaskTone::Ambient),
        (Import, ManualOne) => (TaskScope::ActionPanel, TaskTone::Inline),

        (Publish, OnboardingFlow) => (TaskScope::Preview, TaskTone::Inline),
        (Publish, SettingsManual) => (TaskScope::Preview, TaskTone::Inline),
        (Publish, Background) => (TaskScope::Workspace, TaskTone::Ambient),
        (Publish, ManualOne) => (TaskScope::Preview, TaskTone::Inline),

        (Deploy, OnboardingFlow) => (TaskScope::Preview, TaskTone::Inline),
        (Deploy, SettingsManual) => (TaskScope::Preview, TaskTone::Inline),
        (Deploy, Background) => (TaskScope::Workspace, TaskTone::Ambient),
        (Deploy, ManualOne) => (TaskScope::Preview, TaskTone::Inline),

        (Syndicate, OnboardingFlow) => (TaskScope::ActionPanel, TaskTone::Inline),
        (Syndicate, SettingsManual) => (TaskScope::ActionPanel, TaskTone::Inline),
        // Gap 2 (R12 — Workspace+Ambient success-advisory surface): a
        // (Syndicate, Background) SUCCESS Job that carries a non-blocking
        // `ShippedDegraded` advisory ("N posts couldn't syndicate") routes HERE,
        // to (Workspace, Ambient) — the progress panel only renders Preview-scope,
        // so this Job has no panel row. Per R12 a non-actionable ShippedDegraded
        // is a QUIET signal (not a panel-pop), so moss STORES the clamped advisory
        // on the Job's terminal `Succeeded.advisories` (proven by
        // `lifecycle_descriptor_success_advisory_is_stored_not_lost` in
        // plugins::runtime). SURFACED via the frontend `task-toast-subscriber`
        // (`shippedDegradedAdvisory`): a Workspace Succeeded Job carrying a
        // ShippedDegraded advisory becomes an info-severity ephemeral toast
        // (Phase 6 PIECE B). Do NOT build a new Workspace render surface here.
        (Syndicate, Background) => (TaskScope::Workspace, TaskTone::Ambient),
        (Syndicate, ManualOne) => (TaskScope::ActionPanel, TaskTone::Inline),

        // Process is by definition background-class — it transforms content
        // during build, not in response to a user-driven import/publish
        // gesture. Expanded explicitly (not via wildcard) so future
        // divergence (a Process card surfacing in OnboardingFlow) is a
        // one-line edit. `Enhance` had the same four rows until ADR-055.
        (Process, OnboardingFlow) => (TaskScope::Workspace, TaskTone::Ambient),
        (Process, SettingsManual) => (TaskScope::Workspace, TaskTone::Ambient),
        (Process, Background) => (TaskScope::Workspace, TaskTone::Ambient),
        (Process, ManualOne) => (TaskScope::Workspace, TaskTone::Ambient),
    };

    let final_tone = if signal.awaiting_user {
        TaskTone::Awaiting
    } else {
        tone
    };

    (scope, kind, final_tone)
}

// ──────────────────────────── wire snapshot ───────────────────────────────

/// Serializable snapshot of a `PanelTask` for the frontend event bus.
///
/// `PanelTask` itself isn't `Serialize` because `started_at: Instant` cannot
/// cross the FFI boundary. This struct mirrors every other field plus a
/// `started_at_ms` derived from `Instant::elapsed()` (milliseconds since
/// spawn — frontend uses it to compute "this task is N ms old" without
/// needing wall-clock alignment with Rust).
///
/// Emitted via `MossEvent::PanelTaskUpdate` after every state transition.
/// Frontend renderers (Ambient hairline, Inline badge, Narrator) filter on
/// `tone` and `scope` to decide whether to react.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PanelTaskWire {
    pub id: TaskId,
    pub window_id: WindowId,
    pub scope: TaskScope,
    pub kind: TaskKind,
    pub tone: TaskTone,
    pub title: Option<String>,
    /// `Some(ms)` for Inline terminal receipts ("Saved · 2s ago" =
    /// `fade_after_ms: Some(30000)`). `None` for persistent live status.
    pub fade_after_ms: Option<u64>,
    /// Milliseconds elapsed since the task was spawned (derived from
    /// `Instant::elapsed()`). Renderers use this for fade-after-N-ms logic.
    pub started_at_ms: u64,
    /// The Job's past-tense verb for the receipt header (Step 3 R2).
    pub verb: Option<Verb>,
    /// The success quantity (R2); present iff terminal `Succeeded` (#4).
    pub amount: Option<Amount>,
    /// The frozen terminal span in millis (R-a) — the receipt's elapsed,
    /// NOT the live-ticking `started_at_ms`. `None` until terminal.
    pub elapsed_ms: Option<u64>,
    /// Parent Job id for sub-jobs (media under Build, Phase 4).
    pub parent: Option<TaskId>,
    pub state: TaskState,
}

impl From<&PanelTask> for PanelTaskWire {
    fn from(task: &PanelTask) -> Self {
        PanelTaskWire {
            id: task.id,
            window_id: task.window_id.clone(),
            scope: task.scope,
            kind: task.kind,
            tone: task.tone(),
            title: task.title.clone(),
            fade_after_ms: task.fade_after.map(|d| d.as_millis() as u64),
            started_at_ms: task.started_at.elapsed().as_millis() as u64,
            verb: task.verb.clone(),
            amount: task.amount.clone(),
            elapsed_ms: task.elapsed.map(|d| d.as_millis() as u64),
            parent: task.parent,
            state: task.state.clone(),
        }
    }
}

// ──────────────────────────── TaskRegistry ────────────────────────────────

/// Boxed closure called after every task mutation with a fresh wire
/// snapshot. The registry invokes this so that pure-Rust mutations
/// (registry-side state changes) automatically propagate to the Tauri
/// event bus when a Tauri runtime is available, without forcing the
/// registry to depend on Tauri types directly.
///
/// In production this is set to a closure that emits
/// `MossEvent::PanelTaskUpdate(snapshot)` via `emit_moss_event(...)`.
/// In tests it is left `None`. The closure must be `Send + Sync` because
/// the registry is `Send + Sync` and the closure is invoked from any
/// thread that mutates a task.
pub type TaskEmitter = Box<dyn Fn(PanelTaskWire) + Send + Sync>;

/// Per-`(WindowId, TaskScope)` task list. The registry is the single
/// source of truth for "what tasks are running where" — renderers read
/// aggregations off it, callers mutate through `TaskHandle`.
///
/// Concurrency: the inner `HashMap` lives behind a `Mutex` so the
/// registry is `Send + Sync`. Tasks themselves do not hold the mutex
/// while running — `TaskHandle` keeps an `Arc<Mutex<…>>` reference and
/// reaches in for short, non-overlapping critical sections.
///
/// The optional `emitter` is invoked once per mutation with a fresh
/// `PanelTaskWire` snapshot. Native Tauri callers install one at
/// startup via `set_emitter` (and remove it via `clear_emitter`);
/// tests + pure-Rust callers leave it `None`.
#[derive(Default)]
pub struct TaskRegistry {
    inner: Mutex<HashMap<(WindowId, TaskScope), Vec<PanelTask>>>,
    emitter: Mutex<Option<TaskEmitter>>,
}

impl std::fmt::Debug for TaskRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskRegistry")
            .field("inner", &self.inner)
            .field("emitter", &"<emitter>")
            .finish()
    }
}

impl TaskRegistry {
    pub fn new() -> Self {
        TaskRegistry::default()
    }

    /// Install (or replace) the wire-snapshot emitter. The closure runs
    /// once per task mutation with a fresh `PanelTaskWire`. Native Tauri
    /// callers wire this to `emit_moss_event(handle,
    /// MossEvent::PanelTaskUpdate(snapshot))`; pure-Rust callers leave it
    /// unset.
    ///
    /// Idempotent — calling twice replaces the previous closure. Returning
    /// `&Arc<Self>` so the call can be chained inline:
    /// `let reg = Arc::new(TaskRegistry::new()); reg.set_emitter(...);`.
    pub fn set_emitter(&self, emitter: TaskEmitter) {
        let mut slot = self
            .emitter
            .lock()
            .expect("TaskRegistry emitter mutex poisoned");
        *slot = Some(emitter);
    }

    /// Test/debug helper: clear the emitter so subsequent mutations don't
    /// re-emit.
    pub fn clear_emitter(&self) {
        let mut slot = self
            .emitter
            .lock()
            .expect("TaskRegistry emitter mutex poisoned");
        *slot = None;
    }

    /// Spawn a new task on `(window_id, scope)`. Returns a handle for
    /// subsequent state transitions. Internal callers pick scope/kind/
    /// tone explicitly; plugin callers route through `route_plugin_task`
    /// first.
    ///
    /// `_tone` is **no longer stored** (Step 3 Phase 6 R5): a Job's tone is
    /// computed by [`PanelTask::tone`] from its nature, not carried as data.
    /// The argument is retained so producers/the router can keep expressing
    /// (and logging) their intended surface without churning every call site;
    /// it is intentionally discarded here.
    ///
    /// Spawn itself emits one wire snapshot so renderers see the task
    /// arrive in `Running { fraction: None, message: None }` state.
    pub fn spawn(
        self: &Arc<Self>,
        window_id: WindowId,
        scope: TaskScope,
        kind: TaskKind,
        _tone: TaskTone,
    ) -> TaskHandle {
        let id = next_task_id();
        let task = PanelTask::new(id, window_id.clone(), scope, kind);
        let key = (window_id, scope);
        let snapshot = PanelTaskWire::from(&task);
        {
            let mut map = self.inner.lock().expect("TaskRegistry mutex poisoned");
            map.entry(key.clone()).or_default().push(task);
        }
        self.emit(snapshot);
        TaskHandle {
            id,
            key,
            registry: Arc::clone(self),
        }
    }

    /// Spawn a task that nests under `parent` (Step 3 Phase 4). Identical to
    /// [`spawn`](Self::spawn) except the new task carries `parent: Some(parent)`,
    /// so the frontend renders it as a child row (and `handleJobUpdate` treats it
    /// as ambient — child Jobs do not auto-surface the panel). Used for the media
    /// sub-jobs (images/videos) that nest under the Build parent Job.
    pub fn spawn_child(
        self: &Arc<Self>,
        window_id: WindowId,
        scope: TaskScope,
        kind: TaskKind,
        _tone: TaskTone,
        parent: TaskId,
    ) -> TaskHandle {
        let id = next_task_id();
        let mut task = PanelTask::new(id, window_id.clone(), scope, kind);
        task.parent = Some(parent);
        let key = (window_id, scope);
        let snapshot = PanelTaskWire::from(&task);
        {
            let mut map = self.inner.lock().expect("TaskRegistry mutex poisoned");
            map.entry(key.clone()).or_default().push(task);
        }
        self.emit(snapshot);
        TaskHandle {
            id,
            key,
            registry: Arc::clone(self),
        }
    }

    /// Run a closure with a fresh `TaskProgress` handle, marking the task
    /// succeeded on `Ok` and failed on `Err`. Convenience wrapper for the
    /// VS Code-style `withProgress` pattern.
    ///
    /// `label` populates the task's `title` field (skipped if empty so the
    /// task stays `title: None` rather than carrying an empty string).
    /// Mirrors ADR-015 § "closure form with progress + cancellation":
    /// `ctx.tasks(scope).with_progress(kind, tone, "Importing 42 files", |…|)`.
    pub fn with_progress<F, T, E>(
        self: &Arc<Self>,
        window_id: WindowId,
        scope: TaskScope,
        kind: TaskKind,
        tone: TaskTone,
        label: &str,
        f: F,
    ) -> Result<T, E>
    where
        F: FnOnce(&TaskHandle) -> Result<T, E>,
        E: std::fmt::Display,
    {
        let mut handle = self.spawn(window_id, scope, kind, tone);
        if !label.is_empty() {
            handle.title(label);
        }
        match f(&handle) {
            Ok(value) => {
                handle.succeeded(None);
                Ok(value)
            }
            Err(err) => {
                handle.failed(err.to_string(), true);
                Err(err)
            }
        }
    }

    /// Snapshot all tasks in a `(window, scope)` slot. Returned `Vec` is
    /// a clone — callers can iterate without holding the registry lock.
    pub fn tasks(&self, window_id: &WindowId, scope: TaskScope) -> Vec<PanelTask> {
        let map = self.inner.lock().expect("TaskRegistry mutex poisoned");
        map.get(&(window_id.clone(), scope))
            .cloned()
            .unwrap_or_default()
    }

    /// Max fraction across running tasks in `(window, scope)` — feeds the
    /// Ambient hairline width. Returns `None` if no running task carries
    /// a fraction (indeterminate state → hairline pulses rather than fills).
    pub fn max_running_fraction(&self, window_id: &WindowId, scope: TaskScope) -> Option<f32> {
        let map = self.inner.lock().expect("TaskRegistry mutex poisoned");
        let mut best: Option<f32> = None;
        if let Some(tasks) = map.get(&(window_id.clone(), scope)) {
            for t in tasks {
                if let TaskState::Running {
                    fraction: Some(f), ..
                } = &t.state
                {
                    best = Some(best.map_or(*f, |b| b.max(*f)));
                }
            }
        }
        best
    }

    /// Most recently started narrated task in `(window, scope)` — feeds
    /// the titlebar status thread. Picks the highest `TaskId` (monotonic
    /// counter → newest spawn) among `tone == Narrated` running/awaiting
    /// tasks. Returns `None` if nothing is currently narrated.
    pub fn latest_narrated(&self, window_id: &WindowId, scope: TaskScope) -> Option<PanelTask> {
        let map = self.inner.lock().expect("TaskRegistry mutex poisoned");
        map.get(&(window_id.clone(), scope))?
            .iter()
            .filter(|t| t.tone() == TaskTone::Narrated && !t.is_terminal())
            .max_by_key(|t| t.id)
            .cloned()
    }

    /// Any task in `(window, scope)` currently in `Awaiting` state. Feeds
    /// the hollow-ring pulse renderer.
    pub fn any_awaiting(&self, window_id: &WindowId, scope: TaskScope) -> bool {
        let map = self.inner.lock().expect("TaskRegistry mutex poisoned");
        map.get(&(window_id.clone(), scope))
            .map(|tasks| tasks.iter().any(|t| t.is_awaiting()))
            .unwrap_or(false)
    }

    /// All tasks across the whole registry in an UNRECOVERABLE `Failed` state
    /// — i.e. `Failed` carrying a `Blocking` root-cause advisory (post-collapse
    /// the recoverability lives in `advisory.severity`, not a separate flag).
    /// Feeds the toast subscriber. Returns one clone per failed task.
    ///
    /// **Consumers polling this method must dedupe by `TaskId`.** Failed
    /// tasks remain in the registry (and therefore in this method's output)
    /// until a subsequent `prune_terminal(window_id, scope)` call removes
    /// them. A subscriber that re-fires the toast on every poll without
    /// tracking already-toasted `TaskId`s will surface the same failure
    /// repeatedly — see T6's toast subscriber design notes.
    pub fn failed_unrecoverable(&self) -> Vec<PanelTask> {
        let map = self.inner.lock().expect("TaskRegistry mutex poisoned");
        map.values()
            .flat_map(|v| v.iter())
            .filter(|t| {
                matches!(
                    &t.state,
                    TaskState::Failed { advisory }
                        if matches!(advisory.severity, Severity::Blocking)
                )
            })
            .cloned()
            .collect()
    }

    /// Locate the `(WindowId, TaskScope)` bucket that owns a given task
    /// id. Returns `None` if no bucket contains the id. Used by the
    /// escape-affordance Tauri commands (`cancel_task`, `task_resend`,
    /// `task_recheck`) which only know the task id — they don't carry
    /// the scope across the FFI boundary.
    ///
    /// O(buckets) scan; tasks within each bucket are checked by id.
    /// With at most a few buckets and a handful of tasks per bucket,
    /// this is fast enough that we accept the linear scan over indexing
    /// complexity.
    pub fn locate_by_id(&self, task_id: TaskId) -> Option<(WindowId, TaskScope)> {
        let map = self.inner.lock().expect("TaskRegistry mutex poisoned");
        for (key, tasks) in map.iter() {
            if tasks.iter().any(|t| t.id == task_id) {
                return Some(key.clone());
            }
        }
        None
    }

    /// Directly set the state of a task identified by `(window, scope, id)`.
    /// Used by the escape-affordance Tauri commands when the original
    /// `TaskHandle` is not reachable (it was moved into the spawner's
    /// closure / dropped). Emits a fresh wire snapshot like every other
    /// mutation path.
    ///
    /// Silently no-ops if the task is not found (the bucket scan in
    /// `locate_by_id` already returned the canonical key, but a race
    /// between locate + set could remove the task; we don't want the
    /// command to panic in that window).
    pub fn set_state(
        &self,
        window_id: &WindowId,
        scope: TaskScope,
        task_id: TaskId,
        new_state: TaskState,
    ) {
        let key = (window_id.clone(), scope);
        self.with_task(&key, task_id, |task| {
            task.state = new_state;
        });
    }

    /// Drive a task's terminal-success receipt **by id**, with a producer-supplied
    /// elapsed span (Step 3 Phase 4). The Build parent Job's receipt is applied from
    /// the post-join barrier, which holds only the `Arc<TaskRegistry>` + the parent
    /// `TaskId` — never a `&mut TaskHandle` (that was moved into the spawner). It sets
    /// `verb`/`amount`/`elapsed` on the terminal transition only, enforces invariant #4
    /// + the C2 fix (a Blocking advisory flips to `Failed` via `TaskState::done`, and a
    /// `Failed` run keeps `amount = None`), and emits a fresh wire snapshot.
    ///
    /// No-ops if the task is not found (e.g. a generation race pruned it).
    pub fn done_with_elapsed_by_id(
        &self,
        task_id: TaskId,
        verb: Verb,
        amount: Option<Amount>,
        receipt: Option<String>,
        advisories: Vec<Advisory>,
        elapsed: Duration,
    ) {
        let Some(key) = self.locate_by_id(task_id) else {
            return;
        };
        self.with_task(&key, task_id, |task| {
            task.verb = Some(verb);
            task.state = TaskState::done(receipt, advisories);
            // #4 + C2: amount attaches ONLY if the run truly succeeded.
            task.amount = if matches!(task.state, TaskState::Succeeded { .. }) {
                amount
            } else {
                None
            };
            task.elapsed = Some(elapsed);
        });
    }

    /// Drive a task to `Cancelled` **by id** (Step 3 Phase 4, review fix). Used by
    /// the post-join barrier's Drop guard: when a build is superseded / early-returns
    /// without producing a "Built" receipt, the parent Job must still reach a terminal
    /// state (a leaked `Running` parent never prunes — `prune_terminal` only removes
    /// terminal tasks). A genuine cancel/supersede is `Cancelled`, NOT a Succeeded
    /// "Built" receipt. No-ops if the task is not found.
    pub fn cancel_by_id(&self, task_id: TaskId) {
        let Some(key) = self.locate_by_id(task_id) else {
            return;
        };
        self.with_task(&key, task_id, |task| {
            task.state = TaskState::Cancelled;
        });
    }

    /// Whether the task identified by `task_id` is in a terminal state (Step 3
    /// Phase 4, review fix). Returns `false` if the task is not found (already
    /// pruned ⇒ treat as terminal-handled). The post-join barrier's Drop guard
    /// reads this to avoid clobbering a Succeeded receipt with a Cancelled state.
    pub fn is_terminal_by_id(&self, task_id: TaskId) -> bool {
        let map = self.inner.lock().expect("TaskRegistry mutex poisoned");
        for tasks in map.values() {
            if let Some(t) = tasks.iter().find(|t| t.id == task_id) {
                return t.is_terminal();
            }
        }
        false
    }

    /// Remove all terminal tasks from `(window, scope)`. Renderers call
    /// this after the fade animation completes so the registry doesn't
    /// grow unbounded.
    pub fn prune_terminal(&self, window_id: &WindowId, scope: TaskScope) {
        let mut map = self.inner.lock().expect("TaskRegistry mutex poisoned");
        if let Some(tasks) = map.get_mut(&(window_id.clone(), scope)) {
            tasks.retain(|t| !t.is_terminal());
        }
    }

    /// Internal helper: mutate a specific task in place and emit a fresh
    /// wire snapshot. Single critical-section + single emit means
    /// renderers never observe a mutation that hasn't been broadcast.
    fn with_task<F>(&self, key: &(WindowId, TaskScope), id: TaskId, f: F)
    where
        F: FnOnce(&mut PanelTask),
    {
        let snapshot = {
            let mut map = self.inner.lock().expect("TaskRegistry mutex poisoned");
            let Some(tasks) = map.get_mut(key) else {
                return;
            };
            let Some(task) = tasks.iter_mut().find(|t| t.id == id) else {
                return;
            };
            f(task);
            PanelTaskWire::from(&*task)
        };
        self.emit(snapshot);
    }

    /// Internal helper: dispatch the snapshot to the emitter if one is
    /// installed. Held in a short critical section so the emitter call
    /// happens outside the registry's data mutex.
    fn emit(&self, snapshot: PanelTaskWire) {
        let emitter = self
            .emitter
            .lock()
            .expect("TaskRegistry emitter mutex poisoned");
        if let Some(f) = emitter.as_ref() {
            f(snapshot);
        }
    }
}

// ──────────────────────────── TaskHandle ──────────────────────────────────

/// Owned handle to a single task in a `TaskRegistry`. Each transition
/// method mutates the registry entry and (in future phases) emits a
/// matching event to renderers — that event wiring lands in T6/T8a.
///
/// Dropping a handle does NOT auto-cancel the task. Plugins / native
/// callers must call one of the terminal methods explicitly. The
/// `with_progress` helper handles the common Ok/Err → succeeded/failed
/// pattern.
pub struct TaskHandle {
    pub id: TaskId,
    key: (WindowId, TaskScope),
    registry: Arc<TaskRegistry>,
}

impl TaskHandle {
    /// Set the task's human-readable title ("Saving index.md", "Importing
    /// 42 files"). Fluent on spawn:
    ///
    /// ```ignore
    /// let task = ctx.tasks(scope).spawn(kind, tone).title("Saving index.md");
    /// ```
    ///
    /// Returns `&mut Self` so it can be chained inline with `spawn(...)`
    /// the same way `with_progress(..., label, _)` derives the title from
    /// its `label` argument. `&mut self` (not consuming `self`) so the
    /// caller can keep using the handle for `progress` / `succeeded` /
    /// `failed` after setting the title.
    pub fn title(&mut self, label: impl Into<String>) -> &mut Self {
        let label = label.into();
        self.registry.with_task(&self.key, self.id, |task| {
            task.title = Some(label);
        });
        self
    }

    /// Update progress. `fraction` should be in `[0.0, 1.0]` when known;
    /// `None` means indeterminate (the Ambient renderer pulses instead
    /// of fills). A `progress()` after `awaiting()` implicitly transitions
    /// back to `Running` (no explicit `resumed()` per ADR-015).
    pub fn progress(&self, fraction: Option<f32>, message: Option<String>) {
        self.registry.with_task(&self.key, self.id, |task| {
            task.state = TaskState::Running { fraction, message };
        });
    }

    /// Set the Job's past-tense verb (Step 3 R2) without driving a terminal
    /// transition. Used for media child Jobs, whose terminal state is set
    /// separately via `set_state(media_child_job_state(...))` — the child carries
    /// a task-named verb but its `amount`/`elapsed` are not the receipt header's
    /// (that belongs to the parent Build Job).
    pub fn set_verb(&self, verb: Verb) {
        self.registry.with_task(&self.key, self.id, |task| {
            task.verb = Some(verb);
        });
    }

    /// Apply a full `TaskState` to the task. Used by ACL adapters (Step 3)
    /// that produce a complete state from a producer phase — e.g. the domain
    /// orchestrator driving `domain::progress::to_job_state(phase)`, whose
    /// `Awaiting` carries a typed `action` that `awaiting()` cannot set.
    pub fn set_state(&self, state: TaskState) {
        self.registry.with_task(&self.key, self.id, |task| {
            task.state = state;
        });
    }

    /// Pause for an out-of-band user action. The Awaiting renderer paints
    /// "Waiting for you to [directive]" + the recovery affordance derived from
    /// `action` (`Action::None` for a bare cancel).
    pub fn awaiting(&self, directive: impl Into<String>, action: Action) {
        let directive = directive.into();
        self.registry.with_task(&self.key, self.id, |task| {
            task.state = TaskState::Awaiting { directive, action };
        });
    }

    /// Terminal: success. Optional `receipt` shows in the Inline renderer
    /// ("Saved", "Imported 12 files"). Carries no advisories — use
    /// `TaskState::done` (via a producer's terminal builder) for the
    /// done-with-advisories case.
    pub fn succeeded(&self, receipt: Option<String>) {
        self.registry.with_task(&self.key, self.id, |task| {
            task.state = TaskState::Succeeded {
                receipt,
                advisories: Vec::new(),
            };
        });
    }

    /// Terminal: failure from a bare `error` string + `recoverable` flag.
    /// Synthesizes the root-cause advisory via [`advisory_from_error`] (every
    /// failure IS an advisory, design §9): an unrecoverable failure becomes a
    /// `Blocking` advisory — the case the toast subscriber surfaces. For a
    /// caller that already holds a structured `Advisory`, use `failed_with`.
    pub fn failed(&self, error: impl Into<String>, recoverable: bool) {
        let advisory = advisory_from_error(error, recoverable);
        self.registry.with_task(&self.key, self.id, |task| {
            task.state = TaskState::Failed { advisory };
        });
    }

    /// Terminal: success carrying advisories, through the `TaskState::done`
    /// smart constructor (`&self`, for callers that hold only a shared handle —
    /// e.g. the plugin lifecycle store, whose `HashMap<TaskId, TaskHandle>`
    /// yields `&TaskHandle`). Enforces invariant #1: if any advisory is
    /// `Blocking`, the run becomes `Failed` with that advisory as root cause
    /// (a "succeeded-but-blocking" Job is unrepresentable). Distinct from the
    /// fluent `done_with` (`&mut self`, which also stamps verb/amount/elapsed);
    /// this variant is for the no-receipt-header terminal where only advisories
    /// matter.
    pub fn done(&self, receipt: Option<String>, advisories: Vec<Advisory>) {
        self.registry.with_task(&self.key, self.id, |task| {
            task.state = TaskState::done(receipt, advisories);
        });
    }

    /// Terminal: failure carrying a structured root-cause advisory (`&self`).
    /// Upholds invariant #3 (the `Failed` state always carries its cause). The
    /// advisory IS the failure — its `what` is the human message, its
    /// `severity` the recoverability — so there is no separate free-text error
    /// to thread through (collapsed in Step 3 Phase 6).
    pub fn failed_with(&self, advisory: Advisory) {
        self.registry.with_task(&self.key, self.id, |task| {
            task.state = TaskState::Failed { advisory };
        });
    }

    /// Terminal-success builder carrying the receipt fields (Step 3). Sets
    /// `verb`/`amount`/`elapsed` on the terminal transition only. Enforces
    /// invariant #4 + the C2 fix: a Blocking advisory flips the state to
    /// `Failed` via `TaskState::done`, and a `Failed` run keeps `amount = None`.
    /// `elapsed` is the frozen measured span (R-a).
    pub fn done_with(
        &mut self,
        verb: Verb,
        amount: Option<Amount>,
        receipt: Option<String>,
        advisories: Vec<Advisory>,
    ) -> &mut Self {
        self.registry.with_task(&self.key, self.id, |task| {
            task.verb = Some(verb);
            task.state = TaskState::done(receipt, advisories);
            // #4 + C2: amount attaches ONLY if the run truly succeeded.
            task.amount = if matches!(task.state, TaskState::Succeeded { .. }) {
                amount
            } else {
                None
            };
            task.elapsed = Some(task.started_at.elapsed());
        });
        self
    }

    // `TaskHandle::done_with_elapsed` was removed in the Phase-4 review fix
    // (FIX 4 — "every module has a consumer"). It had ZERO production consumers:
    // the Build parent's terminal receipt is driven registry-level via
    // `TaskRegistry::done_with_elapsed_by_id` (the post-join barrier holds only an
    // `Arc<TaskRegistry>` + the parent `TaskId`, never a `&mut TaskHandle`).

    /// Terminal-success builder stamping the typed `verb` + `amount` on the
    /// Job, through a shared (`&self`) handle (Step 3 Phase 5, §8 + R13).
    ///
    /// This is the `&self` analog of [`TaskHandle::done_with`]: the plugin
    /// lifecycle store yields `&TaskHandle` (its `HashMap<TaskId, TaskHandle>`
    /// gives shared refs), so the descriptor-driven plugin Job path cannot use
    /// the `&mut self` `done_with`. The `verb` is moss's OWN normalized verb
    /// (R13 already applied at descriptor resolution — see
    /// `plugins::runtime::ResolvedJobDescriptor`); `amount` is
    /// `Amount { count, noun }` built from the plugin's reported count + the
    /// descriptor's noun. moss renders "Syndicated · N posts" from these value
    /// objects, never the plugin's free-text string.
    ///
    /// Enforces invariant #4 + the C2 fix exactly as `done_with`: a Blocking
    /// advisory flips the state to `Failed` via `TaskState::done`, and a
    /// `Failed` run keeps `amount = None`. `elapsed` is the frozen span since
    /// spawn.
    pub fn done_with_verb_amount(
        &self,
        verb: Verb,
        amount: Option<Amount>,
        receipt: Option<String>,
        advisories: Vec<Advisory>,
    ) {
        self.registry.with_task(&self.key, self.id, |task| {
            task.verb = Some(verb);
            task.state = TaskState::done(receipt, advisories);
            // #4 + C2: amount attaches ONLY if the run truly succeeded.
            task.amount = if matches!(task.state, TaskState::Succeeded { .. }) {
                amount
            } else {
                None
            };
            task.elapsed = Some(task.started_at.elapsed());
        });
    }

    /// Terminal: explicit user cancellation.
    pub fn cancelled(&self) {
        self.registry.with_task(&self.key, self.id, |task| {
            task.state = TaskState::Cancelled;
        });
    }

    /// Set the fade duration for Inline terminal receipts ("Saved · 2s
    /// ago" = `fade_after: Some(Duration::from_secs(30))`).
    pub fn set_fade_after(&self, fade: Option<Duration>) {
        self.registry.with_task(&self.key, self.id, |task| {
            task.fade_after = fade;
        });
    }
}

// ─────────────────────────────── tests ────────────────────────────────────

#[cfg(test)]
#[path = "tasks_tests.rs"]
mod tests;
