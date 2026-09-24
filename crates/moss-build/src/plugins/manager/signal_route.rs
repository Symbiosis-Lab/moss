//! Engine-agnostic hook-signal routing (Phase 2).
//!
//! Every hook signal — from the `plugin_message` Tauri command or from the
//! QuickJS host dispatch's interception of the same call — lands in the SAME
//! `PluginHookState` through this one function, so the inactivity watchdog
//! stays engine-agnostic. State first, then the reporter: Progress is also a
//! `PipelineEvent`, painted by whatever shell the reporter faces.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::build::ports::reporter::BuildReporter;
use crate::build::progress::PipelineEvent;

use crate::plugins::hook_state::{CompletionResult, PluginHookState};
use crate::plugins::types::{HookResult, PluginProgressEvent};

/// The four multiplexed signal kinds a hook emits. The serde shape is the
/// wire format moss-api's `sendMessage` sends (`packages/moss-api/src/types/
/// messages.ts`); the `plugin_message` command parses straight into it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookSignal {
    Progress {
        phase: String,
        current: u32,
        total: u32,
        message: Option<String>,
    },
    Log {
        level: String,
        message: String,
    },
    Error {
        error: String,
        context: Option<String>,
        fatal: bool,
    },
    /// The hook has finished executing.
    Complete {
        success: bool,
        error: Option<String>,
        /// Optional hook result carrying deployment info (publisher plugins).
        #[serde(default)]
        result: Option<HookResult>,
    },
}

/// Route one signal: into the hook state, then Progress on to the reporter.
///
/// Every kind stamps activity, Log included — any word from the plugin means
/// it is alive (Phase-3 plan D7). The command path once skipped the stamp for
/// Log while the engine path made it, which is the drift one arm ends.
pub fn route_signal(
    state: &PluginHookState,
    reporter: &dyn BuildReporter,
    plugin_name: &str,
    hook_name: &str,
    signal: HookSignal,
) {
    match signal {
        HookSignal::Progress { phase, current, total, message } => {
            state.update_activity(plugin_name, hook_name);
            let detail = message.as_deref().map(|m| format!(": {m}")).unwrap_or_default();
            if current == 0 || current == total {
                log::info!(target: "plugin", "✅ [{plugin_name}] {phase} ({current}/{total}){detail}");
            } else {
                log::debug!(target: "plugin", "[{plugin_name}] {phase} ({current}/{total}){detail}");
            }
            reporter.report(&PipelineEvent::PluginProgress(PluginProgressEvent {
                plugin_name: plugin_name.to_string(),
                hook_name: hook_name.to_string(),
                phase,
                current,
                total,
                message,
                completed: false,
            }));
        }
        HookSignal::Log { level, message } => {
            match level.as_str() {
                "error" => log::error!(target: "plugin", "❌ [{plugin_name}] {message}"),
                "warn" => log::warn!(target: "plugin", "⚠️ [{plugin_name}] {message}"),
                _ => log::info!(target: "plugin", "✅ [{plugin_name}] {message}"),
            }
            state.update_activity(plugin_name, hook_name);
        }
        HookSignal::Error { error, context, fatal } => {
            let error_msg = match &context {
                Some(ctx) => format!("[{ctx}] {error}"),
                None => error,
            };
            log::error!(target: "plugin", "❌ [{plugin_name}] {error_msg}");
            state.add_error(plugin_name, hook_name, error_msg, fatal);
        }
        HookSignal::Complete { success, error, result } => {
            let status = if success { "success" } else { "failed" };
            log::info!(target: "plugin", "✅ [{plugin_name}] Hook {hook_name} complete ({status})");
            state.update_activity(plugin_name, hook_name);
            state.send_completion(plugin_name, hook_name, CompletionResult { success, error, result });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::ports::reporter::discarding;

    #[tokio::test]
    async fn complete_signal_fires_completion_oneshot() {
        let state = PluginHookState::default();
        let (tx, rx) = tokio::sync::oneshot::channel();
        state.set_completion_sender("p", "process", tx);
        route_signal(
            &state,
            discarding(),
            "p",
            "process",
            HookSignal::Complete { success: true, error: None, result: None },
        );
        let got = rx.await.unwrap();
        assert!(got.success);
    }

    /// The wire shape moss-api sends parses straight in — no second enum.
    #[test]
    fn parses_the_sdk_wire_shape() {
        let sig: HookSignal =
            serde_json::from_str(r#"{"type":"complete","success":true,"error":null}"#).unwrap();
        assert!(matches!(sig, HookSignal::Complete { success: true, result: None, .. }));
        let sig: HookSignal = serde_json::from_str(
            r#"{"type":"progress","phase":"fetch","current":1,"total":3,"message":null}"#,
        )
        .unwrap();
        assert!(matches!(sig, HookSignal::Progress { current: 1, total: 3, .. }));
    }
}
