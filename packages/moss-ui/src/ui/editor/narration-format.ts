/**
 * narration-format.ts — Pure formatting helpers for narrated and awaiting
 * PanelTask rendering in the pill popover.
 *
 * Extracted from `narrator.ts` and `awaiting.ts` so the logic can be reused
 * by `progress-panel.ts` without importing the DOM/mount/timer machinery those
 * modules carried.  All functions are pure (no side-effects, no DOM access,
 * no timers) and are directly unit-testable.
 *
 * @module narration-format
 */

import type { Action, TaskState } from "@symbiosis-lab/moss-ui";
import { t } from "../i18n/index";

// ── Types re-exported from here for test + panel convenience ──────────────

export interface NarratedTask {
  id: string;
  title: string | null;
  startedAtMs: number;
  state: TaskState;
}

// ── Narrated task helpers ─────────────────────────────────────────────────

/**
 * Pick the headline task from a collection of narrated tasks. Mirrors
 * `TaskRegistry::latest_narrated`:
 *   - prefers non-terminal tasks
 *   - among non-terminal, picks the one with the highest startedAtMs
 *   - if no non-terminal exists, returns the most recently-terminal task
 *     (so the renderer can show its completion message before fading)
 *   - returns null if no tasks at all
 */
export function pickHeadline(tasks: NarratedTask[]): NarratedTask | null {
  let nonTerminal: NarratedTask | null = null;
  let mostRecentTerminal: NarratedTask | null = null;
  for (const t of tasks) {
    const term =
      t.state.state === "succeeded" ||
      t.state.state === "failed" ||
      t.state.state === "cancelled";
    if (term) {
      if (!mostRecentTerminal || t.startedAtMs > mostRecentTerminal.startedAtMs) {
        mostRecentTerminal = t;
      }
    } else {
      if (!nonTerminal || t.startedAtMs > nonTerminal.startedAtMs) {
        nonTerminal = t;
      }
    }
  }
  return nonTerminal ?? mostRecentTerminal;
}

/**
 * Format a narrated task into a single-line phase label.
 *
 *   running + title          → title
 *   running + message only   → message
 *   running (neither)        → "Working…"
 *   awaiting + directive     → directive (no "Waiting for you" prefix — that's
 *                              the Awaiting renderer's job; if a plugin author
 *                              chose Narrated for a quiet in-band wait per the
 *                              spec's in-band rule, the directive *is* the
 *                              label)
 *   succeeded + receipt      → receipt
 *   succeeded + title        → "<title> complete"
 *   succeeded (neither)      → "Done"
 *   failed + error           → "Failed: <error>"
 *   cancelled                → "Cancelled"
 */
export function formatNarration(task: NarratedTask): string {
  const s = task.state;
  switch (s.state) {
    case "running":
      if (task.title) return task.title;
      if (s.message) return s.message;
      return t("status.working");
    case "awaiting":
      return s.directive;
    case "succeeded":
      if (s.receipt) return s.receipt;
      if (task.title) return t("status.complete", { title: task.title });
      return t("status.done");
    case "failed":
      return t("status.failed", { error: s.advisory.what });
    case "cancelled":
      return t("status.cancelled");
  }
}

// ── Awaiting task helpers ─────────────────────────────────────────────────

/**
 * Format the awaiting copy. Sources directly from the task's
 * `TaskState::Awaiting { directive }`. Per spec the directive is the
 * full clause after "Waiting for you to" (e.g., "click the link in your
 * email" → "Waiting for you to click the link in your email."), so the
 * renderer's job is just to prefix and add a period.
 *
 * If a non-Awaiting state slips through (defensive), returns empty
 * string — the renderer should not paint anything.
 */
export function formatAwaitingCopy(state: TaskState): string {
  if (state.state !== "awaiting") return "";
  const directive = state.directive.trim();
  if (!directive) return t("narration.awaiting_generic");
  // Some directives may already include their own punctuation; keep the
  // prefix simple and let the directive carry trailing punctuation.
  const needsPeriod = !/[.!?]$/.test(directive);
  return t("narration.awaiting_directive", {
    directive: `${directive}${needsPeriod ? "." : ""}`,
  });
}

/**
 * Label for the escape affordance button, read from the typed `Action`
 * `Action::None` is a bare cancel; every other variant carries
 * its own `label`.
 */
export function escapeLabel(action: Action): string {
  if (action === "None") return t("narration.escape_cancel");
  if ("Command" in action) return action.Command.label;
  if ("InApp" in action) return action.InApp.label;
  // `Acknowledge` never reaches a narrated task's escape affordance — it is a
  // notice-rail action — but the label is carried the same way, so read it
  // rather than making the compiler's exhaustiveness a runtime surprise.
  if ("Acknowledge" in action) return action.Acknowledge.label;
  return action.Link.label;
}

/**
 * Name of the Tauri command for the escape action. `Action::None` cancels;
 * the in-app `RecheckDns` op re-checks an out-of-band state; any other in-app
 * op (e.g. `SignIn`, the domain "resend" affordance) re-triggers the awaited
 * action through `task_resend`. The renderer routes `Command`/`Link` actions
 * to their own effect (copy / open) rather than a registry command.
 */
export function escapeCommand(action: Action): string {
  if (action === "None") return "cancel_task";
  if ("InApp" in action && action.InApp.op === "RecheckDns") return "task_recheck";
  return "task_resend";
}

/**
 * The `data-escape-kind` attribute value for an escape action — preserves the
 * legacy "cancel" / "resend" / "recheck" classification for styling + tests,
 * derived from the typed `Action`.
 */
export function escapeKind(action: Action): string {
  if (action === "None") return "cancel";
  if ("InApp" in action && action.InApp.op === "RecheckDns") return "recheck";
  return "resend";
}
