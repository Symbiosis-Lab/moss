//! The headless half of the plugin runtime — everything here runs with no
//! Tauri dependency (open-CLI plan slice 1,
//! docs/archive/2026-08-28-open-cli-engine-and-thin-binary-plan.md). At slice 2
//! this file moves verbatim under `crates/moss-build/src/plugins/runtime/`;
//! every `crate::` path below spells the same in both crates (the app's
//! `crate::build`, `crate::tasks`, `crate::advisory`, `crate::plugins::types`
//! are re-export shims of the moss-build originals), and the one
//! `crate::` path becomes `crate::` in the move.
//!
//! The `#[tauri::command]` wrappers for these fns stay in `runtime.rs`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::build::features::comment::resolve_matters_domain;
use crate::plugins::types::{PluginHook, TriggerContext};
use crate::tasks::{
    escape_spec_to_action, route_plugin_task, Amount, PluginTaskSignal, TaskHandle,
    TaskId, TaskRegistry, Verb, WindowId,
};
use crate::config::environment::HostingEnvironment;

use super::download;

// ============================================================================
// Wire types (moved from commands.rs — the SDK-facing result/input shapes)
// ============================================================================

/// Result of an HTTP fetch operation
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FetchResult {
    /// HTTP status code
    pub status: u16,
    /// Whether the request was successful (2xx status)
    pub ok: bool,
    /// Response body as base64-encoded bytes (for binary data like images)
    pub body_base64: String,
    /// Content-Type header from response
    pub content_type: Option<String>,
}

/// One ordered text field in a multipart/form-data POST.
///
/// Order is preserved (a `Vec`, not a map) because the GraphQL multipart request
/// spec requires the `operations` field before `map` before the file parts.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MultipartTextField {
    /// Form field name (e.g. "operations", "map").
    pub name: String,
    /// Field value (e.g. the JSON-encoded GraphQL operation).
    pub value: String,
}

/// One file part in a multipart/form-data POST. Bytes are carried as base64 so
/// the value survives the JS↔Rust IPC boundary (plugins read them via
/// `read_site_file`, which already returns base64).
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MultipartFilePart {
    /// Form field name for this file (e.g. "0" per the GraphQL multipart spec).
    pub field: String,
    /// File name reported in the part's `Content-Disposition`.
    pub filename: String,
    /// MIME type for the part's `Content-Type` header.
    pub content_type: String,
    /// File contents, base64-encoded.
    pub content_base64: String,
}

/// Result of downloading and saving an asset
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DownloadAssetResult {
    /// HTTP status code
    pub status: u16,
    /// Whether the request was successful (2xx status)
    pub ok: bool,
    /// Content-Type header from response
    pub content_type: Option<String>,
    /// Number of bytes written
    pub bytes_written: u64,
    /// Actual relative path where the file was saved (may differ from requested path)
    pub actual_path: String,
}

/// Result of executing an external binary
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BinaryExecutionResult {
    /// Process exit code
    pub exit_code: i32,

    /// Standard output as string
    pub stdout: String,

    /// Standard error as string
    pub stderr: String,

    /// Whether the process exited successfully (exit_code == 0)
    pub success: bool,
}

// ============================================================================
// HTTP fetch + markdown conversion (bodies behind the runtime.rs commands and
// the QuickJS host arms)
// ============================================================================

/// Fetch a URL using Rust (bypasses WebKit CORS restrictions).
pub async fn fetch_url(url: String, timeout_ms: Option<u64>) -> Result<FetchResult, String> {
    download::do_http_request(url, download::HttpMethod::Get, None, timeout_ms, "fetch_url").await
}

/// HTTP POST request with JSON body.
pub async fn http_post(
    url: String,
    body: String,
    headers: Option<HashMap<String, String>>,
    timeout_ms: Option<u64>,
) -> Result<FetchResult, String> {
    download::do_http_request(
        url,
        download::HttpMethod::Post { body },
        headers,
        timeout_ms,
        "http_post",
    )
    .await
}

/// HTTP GET request with optional headers.
pub async fn http_get(
    url: String,
    headers: Option<HashMap<String, String>>,
    timeout_ms: Option<u64>,
) -> Result<FetchResult, String> {
    download::do_http_request(url, download::HttpMethod::Get, headers, timeout_ms, "http_get")
        .await
}

/// HTTP POST with a `multipart/form-data` body (ordered text fields + file
/// parts). File bytes arrive base64-encoded (they cross the JS↔Rust IPC and
/// plugins read them via `read_site_file`, which is already base64).
pub async fn http_post_multipart(
    url: String,
    text_fields: Vec<MultipartTextField>,
    files: Vec<MultipartFilePart>,
    headers: Option<HashMap<String, String>>,
    timeout_ms: Option<u64>,
) -> Result<FetchResult, String> {
    use base64::Engine;

    let text: Vec<(String, String)> =
        text_fields.into_iter().map(|f| (f.name, f.value)).collect();

    let mut decoded_files: Vec<(String, String, String, Vec<u8>)> =
        Vec::with_capacity(files.len());
    for f in files {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(f.content_base64.as_bytes())
            .map_err(|e| format!("Invalid base64 for file '{}': {}", f.filename, e))?;
        decoded_files.push((f.field, f.filename, f.content_type, bytes));
    }

    let (body, content_type) = download::build_multipart_body(&text, &decoded_files);
    download::do_http_request(
        url,
        download::HttpMethod::PostBytes { body, content_type },
        headers,
        timeout_ms,
        "http_post_multipart",
    )
    .await
}

/// Convert an HTML fragment to Markdown using moss's bundled `htmd` converter
/// — ONE converter for the whole app (B4); on conversion error the input HTML
/// is returned unchanged so a malformed fragment never fails an import.
pub async fn html_to_markdown(html: String) -> Result<String, String> {
    Ok(htmd::convert(&html).unwrap_or_else(|_| html.clone()))
}

#[cfg(test)]
mod html_to_markdown_tests {
    /// B3 regression: the hand-rolled converter emitted `\\\n` for every `<br>`
    /// and didn't special-case empty `<p><br></p>` spacers, producing 119 lone
    /// `\` lines in @guo's import. htmd must NOT produce lone-backslash lines.
    #[test]
    fn br_and_empty_paragraph_produce_no_lone_backslash() {
        let html = "<p>First line<br>second line</p><p><br></p><p>After blank</p>";
        let md = htmd::convert(html).expect("htmd convert");
        let lone = md.lines().filter(|l| l.trim() == "\\").count();
        assert_eq!(lone, 0, "htmd emitted lone-backslash line(s); output:\n{md}");
        // Content is preserved.
        assert!(md.contains("First line"));
        assert!(md.contains("second line"));
        assert!(md.contains("After blank"));
    }
}


// ============================================================================
// Plugin env-var allow-list
// ============================================================================

/// Shared body for get_plugin_env_var — called by the Tauri command and the engine arm.
/// The allow-list is enforced here so the engine path can't be abused as a generic
/// env-var leak. New entries require an explicit edit + code review.
pub fn get_plugin_env_var_impl(
    name: &str,
    env: &HostingEnvironment,
) -> Option<String> {
    // Allow-list — keep tight. Anything not here returns None even if
    // the var is set in the process environment.
    const ALLOWED: &[&str] = &["MOSS_MATTERS_TEST_PROFILE", "MOSS_MATTERS_DOMAIN"];
    if !ALLOWED.contains(&name) {
        log::warn!("get_plugin_env_var: refusing non-allow-listed var '{}'", name);
        return None;
    }
    let explicit = std::env::var(name).ok();
    if name == "MOSS_MATTERS_DOMAIN" {
        return resolve_matters_domain(env, explicit);
    }
    explicit
}

// ============================================================================
// PanelTask lifecycle bridge (ADR-015 Phase 2 entry — T8a)
// ============================================================================
//
// `report_plugin_task_lifecycle` is the Rust entry point for the
// `startTask()` TS API in `packages/moss-api/src/utils/messaging.ts`. The
// frontend plugin SDK serializes lifecycle calls as `PluginTaskLifecycle`
// payloads; this command translates each into a `TaskRegistry` mutation.
//
// Wiring:
// 1. `Started` payload constructs a `PluginTaskSignal`, calls
//    `route_plugin_task` to pick `(TaskScope, TaskKind, TaskTone)`, spawns
//    a fresh task on the registry, stores the handle in
//    `PluginTaskHandleStore` keyed by `TaskId`, and returns the id.
// 2. Subsequent payloads (`Progress`, `Awaiting`, `Succeeded`, `Failed`,
//    `Cancelled`) carry the `TaskId` from step 1; the store looks up the
//    stored handle and applies the matching mutator.
// 3. Terminal payloads remove the handle from the store after applying.
//
// The store is the only stateful piece T8a adds to Layer-2; `TaskHandle`
// already owns its `(WindowId, TaskScope)` key so no per-call routing
// state is needed. Pure-Rust callers continue to use
// `TaskRegistry::spawn` / `with_progress` directly — this command is the
// JS bridge only.

/// A plugin job descriptor (`contributes.jobs[id]`) resolved against the
/// loaded manifest and ALREADY NORMALIZED (Step 3 Phase 5, §8 + R13).
///
/// This is moss's OWN typed verb — `verb` is the output of
/// [`Verb::normalized`], run in PRODUCTION on the plugin-supplied word at
/// descriptor-resolution time (the command resolves it from the
/// `ManagerCache`). A plugin that ships `"🚀 syndicated!!!"` in its
/// manifest cannot reach the surface verbatim: by the time it lands here it is
/// the clamped, emoji-stripped, capitalized `Verb("Syndicated")`. `noun` is the
/// amount unit (e.g. `"posts"`) for the receipt "Syndicated · N posts".
#[derive(Debug, Clone)]
pub struct ResolvedJobDescriptor {
    /// moss's normalized verb (R13 applied) — NOT the raw plugin string.
    pub verb: Verb,
    /// The amount noun (e.g. `"posts"`).
    pub noun: String,
}

/// In-process map from `TaskId` → live `TaskHandle` for plugin-spawned
/// tasks. `Started` inserts; terminal lifecycles remove. Survives across
/// command invocations because the handle holds an `Arc<TaskRegistry>`
/// reference.
///
/// `descriptors` is the parallel store of resolved `contributes.jobs`
/// descriptors (Step 3 Phase 5): when a plugin's `startTask` references a job
/// id, the Tauri command resolves the manifest descriptor (running
/// `Verb::normalized` on the plugin's word — R13), and stashes the typed
/// `ResolvedJobDescriptor` here keyed by the same `TaskId`. A terminal
/// `Succeeded { amount }` then reads it back to stamp moss's OWN
/// `Verb` + `Amount { count, noun }` on the Job — moss renders
/// "Syndicated · N posts" from its own value objects, never the plugin's
/// pre-formatted string.
#[derive(Default)]
pub struct PluginTaskHandleStore {
    inner: std::sync::Mutex<HashMap<TaskId, TaskHandle>>,
    descriptors: std::sync::Mutex<HashMap<TaskId, ResolvedJobDescriptor>>,
}

impl PluginTaskHandleStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stash a resolved (already-normalized) job descriptor for `task_id`.
    /// Called by the Tauri command right after a `Started` whose `startTask`
    /// referenced a `contributes.jobs` id. Read back on the terminal
    /// `Succeeded { amount }` to stamp moss's typed verb + amount.
    pub fn set_descriptor(&self, task_id: TaskId, descriptor: ResolvedJobDescriptor) {
        if let Ok(mut map) = self.descriptors.lock() {
            map.insert(task_id, descriptor);
        }
    }

    /// Inspection accessors for the app-side lifecycle tests
    /// (src-tauri runtime_tests.rs); the maps themselves stay private so
    /// prod code cannot bypass the store's API.
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn task_count(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn descriptor_count(&self) -> usize {
        self.descriptors.lock().unwrap().len()
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn has_descriptor(&self, task_id: &TaskId) -> bool {
        self.descriptors.lock().unwrap().contains_key(task_id)
    }
}

/// Lifecycle payload emitted from the TS `TaskHandle` shim in
/// `packages/moss-api/src/utils/messaging.ts`. Mirrors ADR-015 § Layer 2
/// `TaskLifecycle` — see that section for the state machine.
///
/// `Started` is the spawn signal (carries hook + trigger + label + flags).
/// Other variants carry only the `TaskId` returned by `Started` plus
/// transition-specific data; the server-side store resolves the id back
/// to a live `TaskHandle`.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginTaskLifecycle {
    Started {
        label: String,
        has_progress: bool,
        cancellable: bool,
        /// Optional plugin-local job id referencing `contributes.jobs[id]`
        /// in the plugin's manifest (Step 3 Phase 5, §8 + R13). When present,
        /// the Tauri command resolves the manifest descriptor (running
        /// `Verb::normalized` on the plugin's word) and stashes the typed
        /// `ResolvedJobDescriptor`; a terminal `Succeeded { amount }` then
        /// stamps moss's OWN verb + `Amount { count, noun }` on the Job.
        /// `#[serde(default)]` keeps legacy `startTask` calls (no job ref)
        /// wire-identical.
        #[serde(default)]
        job: Option<String>,
    },
    Progress {
        fraction: Option<f32>,
        message: Option<String>,
    },
    Awaiting {
        directive: String,
        /// "cancel" | "resend:<affordance>" | "recheck:<affordance>".
        /// Parsed by the shared `tasks::parse_escape_action` helper so
        /// plugins and the dev harness speak the same dialect.
        escape: String,
    },
    Succeeded {
        receipt: Option<String>,
        /// Advisories the plugin PROPOSES on success (Step 3 Phase 5, §8 +
        /// R13). Each is clamped through `clamp_plugin_advisory` (the severity
        /// gavel) and attached to the Job's terminal state via the
        /// `TaskState::done` smart constructor — so an actionable `Blocking`
        /// proposal flips the run to `Failed` (invariant #1), while a
        /// non-actionable one is demoted to a quiet `NeedsAction` dot.
        /// `#[serde(default)]` keeps legacy `succeeded()` calls (no advisories)
        /// wire-compatible.
        #[serde(default)]
        advisories: Vec<crate::plugins::types::PluginAdvisory>,
        /// The COUNT a descriptor-driven job reports on success (Step 3 Phase 5,
        /// §8 + R13). When the run started with a `job` ref AND a stashed
        /// descriptor exists, moss pairs this count with the descriptor's noun
        /// to stamp `Amount { count, noun }` + the typed `Verb` on the Job — so
        /// moss renders "Syndicated · N posts" from its OWN value objects, not
        /// the plugin's `receipt` string. `#[serde(default)]` keeps the legacy
        /// free-text `receipt`-only path byte-identical for plugins that don't
        /// declare a descriptor.
        #[serde(default)]
        amount: Option<u64>,
    },
    Failed {
        error: String,
        recoverable: bool,
        /// Advisories the plugin PROPOSES on failure (Step 3 Phase 5, §8 +
        /// R13) — typically the blocking root cause. Clamped through the gavel;
        /// the first surviving advisory becomes the Job's `Failed` root cause
        /// (invariant #3). `#[serde(default)]` keeps legacy `failed()` calls
        /// wire-compatible.
        #[serde(default)]
        advisories: Vec<crate::plugins::types::PluginAdvisory>,
    },
    Cancelled,
}

impl PluginTaskLifecycle {
    /// Whether this report means the task is waiting on the person.
    ///
    /// `Awaiting` opens the wait and every other report closes it — which is
    /// what re-arms the inactivity watchdog once a question has been answered.
    /// Both lifecycle bridges (the Tauri command and the QuickJS host fn) ask
    /// here, so the rule cannot drift between the two engines.
    ///
    /// Written out rather than `matches!` on purpose: a seventh lifecycle would
    /// otherwise default to "not awaiting" in silence. Here it is a compile
    /// error at the one place that decides.
    pub fn is_awaiting(&self) -> bool {
        match self {
            Self::Awaiting { .. } => true,
            Self::Started { .. }
            | Self::Progress { .. }
            | Self::Succeeded { .. }
            | Self::Failed { .. }
            | Self::Cancelled => false,
        }
    }
}

/// Pure-Rust report function. Splits out so the Tauri command is a thin
/// shim and the routing logic stays unit-testable without a Tauri runtime.
///
/// On `Started`, spawns a task, stores the handle, returns the `TaskId`.
/// On non-`Started`, looks up the handle in the store and applies the
/// transition; returns the same `TaskId` echoed back so the caller can
/// confirm. Terminal lifecycles remove the handle from the store after
/// applying.
///
/// `task_id` is `None` for `Started` (the registry mints a fresh id) and
/// `Some` for every other variant. Returning `Result<TaskId, String>` so the
/// frontend can surface "unknown task id" as an explicit error rather
/// than silently dropping the call. `TaskId` serializes as a string on the
/// wire (specta stringifies `u64`) and deserializes from string-or-number,
/// so the id the frontend echoes back round-trips correctly.
pub fn report_plugin_task_lifecycle(
    registry: &std::sync::Arc<TaskRegistry>,
    store: &PluginTaskHandleStore,
    window_id: WindowId,
    plugin_name: &str,
    hook: PluginHook,
    trigger: TriggerContext,
    task_id: Option<TaskId>,
    lifecycle: PluginTaskLifecycle,
) -> Result<TaskId, String> {
    match lifecycle {
        PluginTaskLifecycle::Started {
            label,
            has_progress,
            cancellable,
            // Descriptor resolution is the COMMAND's job (it has the
            // `ManagerCache`); the pure function only spawns + stores the
            // handle. The command calls `store.set_descriptor(id, ...)` after
            // this returns the minted id.
            job: _,
        } => {
            let signal = PluginTaskSignal {
                hook,
                trigger,
                has_progress,
                cancellable,
                awaiting_user: false,
            };
            let (scope, kind, tone) = route_plugin_task(signal);
            let mut handle = registry.spawn(window_id, scope, kind, tone);
            if !label.is_empty() {
                handle.title(label);
            }
            let id = handle.id;
            log::debug!(
                target: "plugin",
                "PanelTask started: plugin={} hook={:?} trigger={:?} → id={} scope={:?} tone={:?}",
                plugin_name, hook, trigger, id, scope, tone,
            );
            store
                .inner
                .lock()
                .map_err(|e| format!("store mutex poisoned: {e}"))?
                .insert(id, handle);
            Ok(id)
        }
        other => {
            let id = task_id.ok_or_else(|| {
                "task_id required for non-started lifecycle payloads".to_string()
            })?;
            // Terminal payloads remove the handle after applying. We do
            // the apply-then-remove dance inside one critical section so
            // the handle can't be operated on by another caller after
            // we've decided it's done.
            let mut guard = store
                .inner
                .lock()
                .map_err(|e| format!("store mutex poisoned: {e}"))?;
            let handle = guard
                .get(&id)
                .ok_or_else(|| format!("unknown plugin task id {id}"))?;
            let is_terminal = matches!(
                other,
                PluginTaskLifecycle::Succeeded { .. }
                    | PluginTaskLifecycle::Failed { .. }
                    | PluginTaskLifecycle::Cancelled
            );
            match other {
                PluginTaskLifecycle::Started { .. } => unreachable!("matched above"),
                PluginTaskLifecycle::Progress { fraction, message } => {
                    handle.progress(fraction, message);
                }
                PluginTaskLifecycle::Awaiting { directive, escape } => {
                    handle.awaiting(directive, escape_spec_to_action(&escape));
                }
                PluginTaskLifecycle::Succeeded {
                    receipt,
                    advisories,
                    amount,
                } => {
                    // The severity gavel (R13): a plugin PROPOSES advisories;
                    // moss clamps each into a final `Advisory` — the only path
                    // by which a plugin-origin advisory exists. The clamped
                    // values flow through `TaskState::done` (via the terminal
                    // helpers), so an actionable `Blocking` proposal flips the
                    // run to `Failed` (invariant #1) rather than the renderer
                    // deciding.
                    let clamped: Vec<_> = advisories
                        .into_iter()
                        .map(crate::plugins::types::clamp_plugin_advisory)
                        .collect();
                    // Descriptor-driven path (R13): if `Started` referenced a
                    // `contributes.jobs` id, the command stashed the resolved
                    // (already `Verb::normalized`) descriptor. Pair it with the
                    // reported `count` to stamp moss's OWN typed `Verb` +
                    // `Amount { count, noun }` — moss renders the receipt from
                    // its value objects, NOT the plugin's free-text string.
                    let descriptor = store
                        .descriptors
                        .lock()
                        .ok()
                        .and_then(|mut d| d.remove(&id));
                    match (descriptor, amount) {
                        (Some(desc), Some(count)) => {
                            // Stamp the typed verb + amount on the terminal
                            // (invariant #4 + C2 enforced inside the helper: a
                            // Blocking advisory flips to Failed and drops the
                            // amount).
                            handle.done_with_verb_amount(
                                desc.verb.clone(),
                                Some(Amount {
                                    count,
                                    noun: desc.noun.clone(),
                                }),
                                receipt,
                                clamped,
                            );
                        }
                        (Some(desc), None) => {
                            // Descriptor present but no count reported: stamp the
                            // typed verb, no amount (invariant #4 — amount only
                            // when a count is offered).
                            handle.done_with_verb_amount(
                                desc.verb.clone(),
                                None,
                                receipt,
                                clamped,
                            );
                        }
                        (None, _) => {
                            // No descriptor (legacy / non-declaring plugins):
                            // byte-identical to the pre-Phase-5 behavior — a bare
                            // receipt with no advisories is the plain-success
                            // fast path; advisories flow through `done`.
                            if clamped.is_empty() {
                                handle.succeeded(receipt);
                            } else {
                                handle.done(receipt, clamped);
                            }
                        }
                    }
                }
                PluginTaskLifecycle::Failed {
                    error,
                    recoverable,
                    advisories,
                } => {
                    // Clamp the proposed advisories (R13). The Job's structured
                    // root cause (invariant #3) should be a genuine failure
                    // signal, so prefer the first `Blocking`/`NeedsAction`
                    // clamped advisory — a `ShippedDegraded` ("shipped, but a
                    // file is unoptimized") is NOT a failure root cause and must
                    // not present as one. If none qualifies, synthesize the
                    // advisory from the plugin's free-text `error` + recoverable
                    // flag (every failure IS an advisory, design §9).
                    use crate::advisory::Severity;
                    let clamped: Vec<_> = advisories
                        .into_iter()
                        .map(crate::plugins::types::clamp_plugin_advisory)
                        .collect();
                    let root_cause = clamped.into_iter().find(|a| {
                        matches!(a.severity, Severity::Blocking | Severity::NeedsAction)
                    });
                    match root_cause {
                        Some(root_cause) => handle.failed_with(root_cause),
                        None => handle.failed(error, recoverable),
                    }
                }
                PluginTaskLifecycle::Cancelled => {
                    handle.cancelled();
                }
            }
            if is_terminal {
                guard.remove(&id);
                // Clear any stashed `contributes.jobs` descriptor (R13). The
                // `Succeeded` arm above already removed it (it consumes the
                // descriptor to stamp the verb/amount); `Failed`/`Cancelled`
                // never did. Without this, a failed or cancelled
                // descriptor-driven run orphans its `ResolvedJobDescriptor`
                // keyed by a now-dead `TaskId` for the process lifetime — no
                // correctness bug (TaskIds are monotonic, never reused), but an
                // unbounded leak across repeated failed syndications. Idempotent
                // (a no-op on the `Succeeded` path, which already removed it).
                if let Ok(mut descriptors) = store.descriptors.lock() {
                    descriptors.remove(&id);
                }
            }
            Ok(id)
        }
    }
}

/// The `contributes.jobs[id]` descriptor a `Started { job }` lifecycle names,
/// R13-normalized: `Verb::normalized` on the plugin's proposed verb, and the
/// noun bounded like every other manifest string that reaches the surface.
/// `None` for every other lifecycle, and when `lookup` knows no such job.
///
/// Both lifecycle bridges resolve through here — the Tauri command over the
/// manager cache, the QuickJS host arm over the caller's grant snapshot — so
/// the normalization has one owner and the two routes cannot drift.
pub fn resolve_started_job_descriptor(
    lifecycle: &PluginTaskLifecycle,
    lookup: impl FnOnce(&str) -> Option<crate::plugins::contributions::JobDescriptor>,
) -> Option<ResolvedJobDescriptor> {
    use moss_core::untrusted_text;
    match lifecycle {
        PluginTaskLifecycle::Started { job: Some(id), .. } => {
            lookup(id).map(|descriptor| ResolvedJobDescriptor {
                verb: Verb::normalized(&descriptor.verb),
                noun: untrusted_text::bounded(&descriptor.noun, untrusted_text::MAX_NAME),
            })
        }
        _ => None,
    }
}

/// Shared body behind BOTH lifecycle bridges — the Tauri command
/// (webview-routed plugins) and the QuickJS engine host arm
/// (`engine::host_fns`). One fn, both callers: the watchdog record, the R13
/// descriptor stash and the spawn must stay identical on the two routes, or
/// quickjs-routed plugins silently lose Job verb/amount stamping
/// ("Syndicated · N posts") — or, as once happened, their `Started` stopped
/// counting as proof of life on one route only.
///
/// `attended` is whether a person could answer a question this hook asks; see
/// [`PluginHookState::record_lifecycle`](crate::plugins::hook_state::PluginHookState::record_lifecycle).
/// `resolved` comes from [`resolve_started_job_descriptor`]; it only stamps a
/// Job's verb and amount for the task UI, so a host with no windows loses
/// nothing it could have rendered — the task is spawned and tracked either way.
pub fn report_plugin_task_lifecycle_with_descriptor(
    state: &crate::engine::host_fns::HostState,
    attended: bool,
    resolved: Option<ResolvedJobDescriptor>,
    window_id: WindowId,
    plugin_name: &str,
    hook: PluginHook,
    trigger: TriggerContext,
    task_id: Option<TaskId>,
    lifecycle: PluginTaskLifecycle,
) -> Result<TaskId, String> {
    state.hooks.record_lifecycle(
        plugin_name,
        hook.hook_name(),
        task_id.as_ref().map(|id| id.0),
        &lifecycle,
        attended,
    );
    let store = &*state.task_handles;
    let id = report_plugin_task_lifecycle(
        &state.tasks,
        store,
        window_id,
        plugin_name,
        hook,
        trigger,
        task_id,
        lifecycle,
    )?;

    // Stash the resolved descriptor against the freshly minted id so the
    // terminal `Succeeded { amount }` can stamp moss's typed verb + amount.
    if let Some(descriptor) = resolved {
        store.set_descriptor(id, descriptor);
    }

    Ok(id)
}


// ============================================================================
// Git binary resolution (headless: `on_progress: None` skips the app's
// download-progress events; the resolver itself is build-tree code)
// ============================================================================

use crate::build::assets::binary_resolver::resolve_binary;
use crate::build::assets::download::DownloadProgress;
use crate::build::assets::git::git_binary_config;

/// Resolve the path to a usable git binary, reporting download progress to
/// `on_progress` (bytes downloaded, total if the server declared one).
///
/// Shared body behind both seams. The progress sink is the ONLY thing the app
/// was ever needed for here, so the headless path (#1019) passes `None` and
/// resolves the same binary — quietly.
pub async fn resolve_git_path_impl(
    on_progress: Option<std::sync::Arc<DownloadProgress>>,
) -> Result<String, String> {
    let config = git_binary_config();
    let result = tokio::task::spawn_blocking(move || {
        resolve_binary(&config, None, true, on_progress.as_deref())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?;

    result.map(|r| r.path)
}

#[cfg(test)]
mod awaiting_tests {
    use super::PluginTaskLifecycle as L;

    /// The rule both lifecycle bridges read. `Awaiting` is the only report that
    /// means "waiting on the person" — everything else, including a `Progress`
    /// after the question, hands the hook back to the inactivity watchdog.
    #[test]
    fn only_the_question_itself_holds_the_watchdog_off() {
        assert!(L::Awaiting { directive: "sign in".into(), escape: "cancel".into() }.is_awaiting());

        for other in [
            L::Started { label: "x".into(), has_progress: false, cancellable: false, job: None },
            L::Progress { fraction: None, message: None },
            L::Succeeded { receipt: None, advisories: vec![], amount: None },
            L::Failed { error: "e".into(), recoverable: false, advisories: vec![] },
            L::Cancelled,
        ] {
            assert!(!other.is_awaiting(), "{other:?} does not hold the watchdog off");
        }
    }
}
