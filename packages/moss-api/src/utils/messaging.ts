/**
 * Plugin messaging utilities for communicating with moss
 *
 * Uses events (fire-and-forget) for log/progress messages to avoid
 * blocking the IPC channel. Uses commands (request-response) for
 * complete/error messages that require acknowledgment.
 */

import type { PluginMessage } from "../types/messages.js";
import { getTauriCore, isTauriAvailable } from "./tauri.js";
import { emitEvent, isEventApiAvailable } from "./events.js";

let currentPluginName = "";
let currentHookName = "";

/**
 * Set the message context for subsequent messages
 * This is typically called automatically by the plugin runtime
 * @category Messaging
 */
export function setMessageContext(pluginName: string, hookName: string): void {
  currentPluginName = pluginName;
  currentHookName = hookName;
}


/**
 * Send a message to moss
 *
 * Log and progress messages use events (fire-and-forget) to avoid blocking IPC.
 * Complete and error messages use commands (request-response) for acknowledgment.
 * @category Messaging
 */
export async function sendMessage(message: PluginMessage): Promise<void> {
  // For log and progress messages, use events (fire-and-forget)
  if (message.type === "log" || message.type === "progress") {
    if (!isEventApiAvailable()) {
      return;
    }
    try {
      await emitEvent("plugin-message", {
        pluginName: currentPluginName,
        hookName: currentHookName,
        message,
      });
    } catch {
      // Silently ignore event failures for non-critical messages
    }
    return;
  }

  // For complete/error messages, use commands (must be acknowledged)
  if (!isTauriAvailable()) {
    return;
  }

  try {
    await getTauriCore().invoke("plugin_message", {
      pluginName: currentPluginName,
      hookName: currentHookName,
      message,
    });
  } catch (error) {
    // Log errors - these are important for debugging serialization issues
    console.error("❌ [SDK] Failed to send message:", message.type, "–", error);
  }
}

/**
 * Report progress to moss
 * @category Messaging
 */
export async function reportProgress(
  phase: string,
  current: number,
  total: number,
  message?: string
): Promise<void> {
  await sendMessage({ type: "progress", phase, current, total, message });
}

/**
 * Report an error to moss
 * @category Messaging
 */
export async function reportError(
  error: string,
  context?: string,
  fatal = false
): Promise<void> {
  await sendMessage({ type: "error", error, context, fatal });
}


// ============================================================================
// PanelTask lifecycle API (ADR-015 Phase 2 — T8a, 2026-05-28)
// ============================================================================
//
// `startTask()` is the preferred path for plugin progress / completion
// reporting going forward. It returns a `TaskHandle` whose methods map
// 1:1 to the `PluginTaskLifecycle` enum on the Rust side
// (`src-tauri/src/plugins/runtime.rs`). Each call invokes the
// `report_plugin_task_lifecycle_command` Tauri command which routes
// through the PanelTask registry and emits typed `PanelTaskUpdate`
// events to the four UI renderers (Ambient, Inline, Narrated, Awaiting).
//
// `reportProgress` / `reportError` / `reportComplete` (above) keep working
// untouched — they are LEGACY but supported. ADR-015 § Migration plan
// schedules the 151-call-site sweep for Phase 3; until then, both APIs
// coexist. Plugin authors writing NEW code should prefer `startTask()`.

/**
 * `PluginHook` mirrors the closed Rust enum in
 * `src-tauri/src/plugins/types.rs`. The router (T1) cross-products
 * `PluginHook × TriggerContext` to pick a UI surface for the task.
 * Plugin authors pick the hook that matches what they're doing; they
 * do NOT pick the surface (the router owns that).
 * @category Messaging
 */
export type PluginHook =
  | "import"
  | "publish"
  | "deploy"
  | "syndicate"
  | "process"
  | "enhance";

/**
 * `TriggerContext` mirrors the closed Rust enum. Tells the router *why*
 * the task was invoked so it can pick a surface that matches the user's
 * focus context (onboarding cards → ActionPanel; background sync →
 * Workspace).
 * @category Messaging
 */
export type TriggerContext =
  | "onboarding_flow"
  | "settings_manual"
  | "background"
  | "manual_one";

/**
 * Escape kind for `awaiting()` calls. The string variants take a
 * free-text affordance label after the colon, mirroring the dev harness
 * dialect (e.g., `"resend:Resend email"`, `"recheck:Recheck DNS"`).
 * Plain `"cancel"` carries no label.
 * @category Messaging
 */
export type EscapeSpec = "cancel" | `resend:${string}` | `recheck:${string}`;

// ── Advisory proposal vocabulary (Step 3 Phase 5, §8 + R13) ───────────────
//
// These TS types mirror the Rust serde wire shapes 1:1 (hand-written here,
// matching the `PluginHook`/`TriggerContext` mirror pattern above — moss-api
// is a standalone npm package and does not import the app's generated
// `bindings.ts`). A plugin PROPOSES an advisory; moss holds the severity gavel
// (`clamp_plugin_advisory`, R13) — the proposal is never the final word.

/**
 * Which axis of the system an advisory is about. Mirrors the Rust
 * `advisory::Scope` (externally-tagged unit enum → bare string).
 * @category Messaging
 */
export type AdvisoryScope =
  | "File"
  | "Config"
  | "Environment"
  | "Remote"
  | "Account";

/**
 * How serious the plugin proposes an advisory is. moss CLAMPS this (R13): a
 * `Blocking` proposal with no actionable affordance is demoted to a quiet
 * `NeedsAction` hairline dot. Mirrors the Rust `advisory::Severity`.
 * @category Messaging
 */
export type AdvisorySeverity = "ShippedDegraded" | "NeedsAction" | "Blocking";

/**
 * A closed set of in-app operations an `AdvisoryAction.InApp` can request.
 * Mirrors the Rust `advisory::AppOp`.
 * @category Messaging
 */
export type AdvisoryAppOp = "MoveFile" | "OpenBilling" | "SignIn" | "RecheckDns";

/**
 * The recovery affordance for an advisory, expressed as data — mirrors the
 * Rust `advisory::Action` (externally-tagged: `"None"` for the unit variant,
 * `{ Variant: {...} }` for data variants). `Action !== "None"` is the gavel's
 * deciding input for whether a `Blocking` proposal may pop the panel.
 * @category Messaging
 */
export type AdvisoryAction =
  | "None"
  | { Command: { run: string; label: string } }
  | { InApp: { op: AdvisoryAppOp; args: unknown; label: string } }
  | { Link: { href: string; label: string } };

/**
 * A plugin's PROPOSED advisory (pre-clamp). Mirrors the Rust
 * `plugins::types::PluginAdvisory` wire shape exactly so it deserializes
 * directly into `PluginTaskLifecycle::Succeeded/Failed { advisories }`. moss
 * is the only constructor of a final `Advisory` — a plugin can never hand moss
 * one (R13).
 * @category Messaging
 */
export interface AdvisoryProposal {
  /** Which axis of the system this advisory is about. */
  scope: AdvisoryScope;
  /** The severity the plugin REQUESTS. moss clamps it (R13). */
  severity: AdvisorySeverity;
  /** The item this is about — usually a filename. `null` for build-wide. */
  item: string | null;
  /** What happened (free text). */
  what: string;
  /** The recovery affordance the plugin proposes. */
  action: AdvisoryAction;
}

/** @category Messaging */
export interface StartTaskOptions {
  /**
   * Hook the task belongs to. Defaults to "import" — the most common
   * onboarding case. Plugins should pass an explicit hook for non-import
   * work (e.g., a syndicator passes "syndicate").
   */
  hook?: PluginHook;
  /**
   * Trigger context. Defaults to "background" — the safest fallback
   * because Background routes to Workspace+Ambient, the quietest surface.
   * Plugins running inside the onboarding flow should pass
   * "onboarding_flow" explicitly so they reach the ActionPanel hairline.
   */
  trigger?: TriggerContext;
  /**
   * Hint to the router that this task will emit `progress()` updates
   * with fractions. Routers and renderers MAY use this to choose between
   * "fills" vs "pulses" visualizations. Defaults to true.
   */
  hasProgress?: boolean;
  /**
   * Whether the user can cancel this task from the UI. Cancellation
   * plumbing lands in a later ADR phase; the flag is recorded today so
   * renderers can show / hide a cancel affordance.
   */
  cancellable?: boolean;
  /**
   * Plugin-local job id referencing `contributes.jobs[id]` in the plugin's
   * manifest (Step 3 Phase 5, §8 + R13). When set, moss looks up the declared
   * descriptor (a past-tense `verb` + an amount `noun`), normalizes the verb
   * (`Verb::normalized` — moss owns capitalization/length/glyphs), and on a
   * `succeeded(receipt, count)` stamps its OWN typed `Verb` + `Amount { count,
   * noun }` on the Job — rendering "Syndicated · N posts" from moss's value
   * objects, never the plugin's pre-formatted `receipt` string. Omit it for
   * free-text-receipt tasks (the legacy path, byte-identical).
   */
  job?: string;
}

/**
 * Lifecycle handle returned by `startTask()`. Calls are fire-and-await:
 * each method returns a promise that resolves once the Rust side has
 * applied the transition to the PanelTask registry. Terminal calls
 * (`succeeded`, `failed`, `cancelled`) remove the task from the registry's
 * tracking store; calling any further method on the same handle will
 * reject with "unknown plugin task id".
 *
 * The state machine matches ADR-015 § Layer 2:
 *
 *   Running ↔ Awaiting → Succeeded | Failed | Cancelled
 *
 * `progress()` after `awaiting()` implicitly transitions back to Running
 * (no explicit `resumed()`).
 * @category Messaging
 */
export interface TaskHandle {
  /**
   * In-process task id minted by the Rust registry on `Started`.
   * Exposed for log correlation and tests.
   *
   * Rust-side this is `u64`; specta types `u64` as `string` because
   * JS numbers lose precision above 2^53. The handle carries the
   * exact string through subsequent transitions so no precision is
   * lost in the round-trip.
   */
  readonly id: string;
  /** Push a progress update. `fraction` in [0,1] if known, else undefined for indeterminate. */
  progress(fraction?: number, message?: string): Promise<void>;
  /**
   * Pause for an out-of-band user action. `directive` describes what
   * the user needs to do ("click the link in your email"); `venue`
   * names where ("your email") — both feed the Awaiting renderer's
   * "Waiting for you to [directive] in [venue]" copy.
   *
   * `escape` defaults to "cancel". For non-cancel escapes, pass
   * `"resend:<label>"` or `"recheck:<label>"`.
   */
  awaiting(directive: string, venue: string, escape?: EscapeSpec): Promise<void>;
  /**
   * PROPOSE an advisory on this task (Step 3 Phase 5, §8 + R13). Accumulates
   * the proposal on the handle; it is flushed into the next terminal call
   * (`succeeded`/`failed`) as `advisories: PluginAdvisory[]`. moss holds the
   * severity gavel server-side: a `Blocking` proposal with no actionable
   * `action` is clamped to a quiet `NeedsAction` dot; an actionable `Blocking`
   * on a `succeeded()` flips the run to `Failed` (invariant #1 — the smart
   * constructor decides, not the plugin).
   *
   * `advise()` does NOT emit on its own; advisories ride the terminal IPC so
   * moss applies them atomically with the success/failure transition. Calling
   * `advise()` after a terminal call has no effect — the handle is spent (the
   * terminal methods set a spent flag and `advise()` no-ops once spent).
   */
  advise(advisory: AdvisoryProposal): Promise<void>;
  /**
   * Terminal: success.
   *
   * @param receipt Optional human-readable receipt (the legacy free-text path).
   *   IGNORED for the verb/amount when the task declared a `job` descriptor —
   *   moss renders the receipt from its OWN normalized verb + amount instead.
   * @param amount Optional success COUNT. Only meaningful when `startTask` was
   *   given a `job` id: moss pairs this count with the descriptor's `noun` to
   *   stamp `Amount { count, noun }` and renders "Syndicated · N posts" from its
   *   value objects.
   */
  succeeded(receipt?: string, amount?: number): Promise<void>;
  /**
   * Terminal: failure. `recoverable=false` (default) also fires the
   * toast subscriber (ADR-015 § "Plugin-originated failure toasts").
   */
  failed(error: string, recoverable?: boolean): Promise<void>;
  /** Terminal: explicit user cancellation. */
  cancelled(): Promise<void>;
}

/**
 * Internal helper for invoking the Rust command. Resolves to the task
 * id Rust echoes back; throws if the invoke fails (caller decides how
 * to surface it — typically Awaiting / progress events fail-silent so a
 * dropped narrator update doesn't crash a plugin).
 *
 * Task id is `string` end-to-end (u64 in Rust → string in specta) to
 * preserve precision above 2^53.
 */
/**
 * Sentinel task id returned when running outside a Tauri context
 * (unit tests, browser preview of plugin). Subsequent `TaskHandle`
 * method calls short-circuit when they see this id so they don't
 * issue invokes with a fake taskId that the Rust router would
 * reject as "unknown task id".
 *
 * Exported solely so the corresponding test can assert against it
 * by symbol; production code should never compare against this
 * literal — use `TaskHandle.id === OUT_OF_TAURI_TASK_ID` is the
 * only legitimate check, and the `TaskHandle` methods already
 * encapsulate it.
 * @category Messaging
 */
export const OUT_OF_TAURI_TASK_ID = "-1";

async function invokeLifecycle(
  pluginName: string,
  hook: PluginHook,
  trigger: TriggerContext,
  taskId: string | undefined,
  lifecycle: Record<string, unknown>
): Promise<string> {
  if (!isTauriAvailable()) {
    // Out-of-Tauri (unit tests, browser preview of plugin) — skip.
    // Return the sentinel so callers don't accidentally treat 0
    // as a valid id; `TaskHandle` methods detect this and no-op.
    return OUT_OF_TAURI_TASK_ID;
  }
  const raw = await getTauriCore().invoke<string | number>(
    "report_plugin_task_lifecycle_command",
    {
      pluginName,
      hook,
      trigger,
      taskId,
      lifecycle,
    }
  );
  // Tauri's specta-generated path serializes u64 as a JSON string;
  // some test mocks return a JS number. Coerce both to string.
  return typeof raw === "number" ? String(raw) : raw;
}

/**
 * Start a plugin task. Returns a `TaskHandle` whose methods drive the
 * lifecycle (progress → awaiting → succeeded/failed/cancelled).
 *
 * The hook + trigger pair flows into the Rust-side `route_plugin_task`
 * router, which picks `(TaskScope, TaskKind, TaskTone)` — i.e., which
 * UI renderer (Ambient hairline / Inline badge / Narrated titlebar /
 * Awaiting pulse) surfaces the task. Plugin authors do NOT pick the
 * surface; they just describe what they're doing and why.
 *
 * Preferred over `reportProgress()` for new code. The legacy API stays
 * supported until ADR-015 Phase 3 sweeps all 151 call sites.
 *
 * @example
 * const task = await startTask("Importing 42 articles", {
 *   hook: "import",
 *   trigger: "onboarding_flow",
 * });
 * for (let i = 0; i < articles.length; i++) {
 *   await task.progress(i / articles.length, `Article ${i + 1}/${articles.length}`);
 *   await importOne(articles[i]);
 * }
 * await task.succeeded(`Imported ${articles.length} articles`);
 * @category Messaging
 */
export async function startTask(
  label: string,
  options: StartTaskOptions = {}
): Promise<TaskHandle> {
  const hook: PluginHook = options.hook ?? "import";
  const trigger: TriggerContext = options.trigger ?? "background";
  const hasProgress = options.hasProgress ?? true;
  const cancellable = options.cancellable ?? false;
  // Capture plugin name at start so subsequent transitions (which may
  // arrive after setMessageContext has rolled to the next hook) still
  // route to the right task.
  const pluginName = currentPluginName;

  const started: Record<string, unknown> = {
    type: "started",
    label,
    has_progress: hasProgress,
    cancellable,
  };
  // Reference the manifest job descriptor so moss can stamp its OWN
  // normalized verb + amount on the terminal (Step 3 Phase 5, §8 + R13).
  if (options.job !== undefined) {
    started.job = options.job;
  }
  const id = await invokeLifecycle(pluginName, hook, trigger, undefined, started);

  // Out-of-Tauri context (unit tests, browser preview): startTask
  // already returned the sentinel id. Hand back a fully no-op
  // handle so plugin code can be exercised without poisoning
  // every subsequent transition with a fake id and without
  // emitting invokes the host can't satisfy.
  if (id === OUT_OF_TAURI_TASK_ID) {
    return {
      id,
      async progress(): Promise<void> {},
      async awaiting(): Promise<void> {},
      async advise(): Promise<void> {},
      async succeeded(): Promise<void> {},
      async failed(): Promise<void> {},
      async cancelled(): Promise<void> {},
    };
  }

  // Advisories proposed via `advise()` accumulate here and ride the next
  // terminal IPC (`succeeded`/`failed`) so moss applies them atomically with
  // the success/failure transition through the smart constructors (R13).
  const pendingAdvisories: AdvisoryProposal[] = [];
  // Set by any terminal call (`succeeded`/`failed`/`cancelled`). Once spent,
  // `advise()` no-ops — the handle is dead and a late advisory would otherwise
  // silently ride a SECOND terminal call (or leak). This enforces the
  // "advise() after a terminal call has no effect" contract the JSDoc promises.
  let spent = false;

  return {
    id,
    async progress(fraction?: number, message?: string): Promise<void> {
      await invokeLifecycle(pluginName, hook, trigger, id, {
        type: "progress",
        fraction,
        message,
      });
    },
    async awaiting(
      directive: string,
      venue: string,
      escape: EscapeSpec = "cancel"
    ): Promise<void> {
      // Directive copy includes the venue for the Awaiting renderer.
      // The renderer formats "Waiting for you to <directive> in <venue>";
      // we splice `venue` into `directive` here so the Rust side only
      // needs to store one string. The verb-place split is a TS-side
      // ergonomic affordance for plugin authors.
      const fullDirective = venue ? `${directive} in ${venue}` : directive;
      await invokeLifecycle(pluginName, hook, trigger, id, {
        type: "awaiting",
        directive: fullDirective,
        escape,
      });
    },
    async advise(advisory: AdvisoryProposal): Promise<void> {
      // No-op once the handle is spent (a terminal call already fired): a late
      // advisory has nowhere to ride. Honors the "advise() after a terminal
      // call has no effect" contract.
      if (spent) return;
      // Accumulate only — the proposal is flushed on the next terminal call so
      // moss applies it atomically with the success/failure transition.
      pendingAdvisories.push(advisory);
    },
    async succeeded(receipt?: string, amount?: number): Promise<void> {
      spent = true;
      // Only attach `advisories` when the plugin actually advised, so a run
      // that never calls advise() keeps the legacy bare wire shape (serde
      // defaults the field to []).
      const lifecycle: Record<string, unknown> = { type: "succeeded", receipt };
      if (pendingAdvisories.length > 0) {
        lifecycle.advisories = pendingAdvisories.slice();
      }
      // The COUNT for a descriptor-driven job (Step 3 Phase 5): moss pairs it
      // with the declared `noun` to render "Syndicated · N posts" from its own
      // value objects. Omitted ⇒ serde defaults to None (no amount stamped).
      if (amount !== undefined) {
        lifecycle.amount = amount;
      }
      await invokeLifecycle(pluginName, hook, trigger, id, lifecycle);
    },
    async failed(error: string, recoverable: boolean = false): Promise<void> {
      spent = true;
      const lifecycle: Record<string, unknown> = {
        type: "failed",
        error,
        recoverable,
      };
      if (pendingAdvisories.length > 0) {
        lifecycle.advisories = pendingAdvisories.slice();
      }
      await invokeLifecycle(pluginName, hook, trigger, id, lifecycle);
    },
    async cancelled(): Promise<void> {
      spent = true;
      await invokeLifecycle(pluginName, hook, trigger, id, {
        type: "cancelled",
      });
    },
  };
}
