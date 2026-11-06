/**
 * TaskToastSubscriber — surfaces unrecoverable PanelTask failures as
 * dismissible toasts via the existing `toast-manager` system.
 *
 * Filter rule:
 *
 *     state.state == "failed" && state.advisory.severity == "Blocking"
 *
 * Dedup rule:
 *
 *     Failed tasks remain in the registry until `prune_terminal()` is
 *     called by the renderer that owns the task. Renderers and registry
 *     callers can re-emit the same wire snapshot during that window
 *     (e.g., when the user navigates a panel that re-paints state). The
 *     subscriber MUST dedupe by `TaskId` so the user sees one toast per
 *     failure, not one per re-emit.
 *
 * Tone filtering (Invariant #4):
 *   `tone==="inline"` tasks are suppressed here — their failure is visible
 *   in the inline badge next to the chip bar. A toast would be a double-
 *   signal for a subject-scoped failure. Toast is the cross-surface signal
 *   for non-inline, non-preview failures only (narrated / ambient / awaiting).
 *
 * Retry affordance:
 *   A failed `PanelTaskWire` carries its root-cause `advisory`;
 *   recoverability is the advisory's `severity`. We subscribe ONLY to
 *   `Blocking` (unrecoverable), so `NeedsAction` / `ShippedDegraded`
 *   failures show no Retry affordance here (they retry via the in-place
 *   Inline badge or the action-panel UI).
 *
 * Purity: this module names no host API. The wire payload it reads is
 * described structurally below, and the event subscription is INJECTED by
 * the caller. The app shell owns the one real
 * `onMossEvent("PanelTaskUpdate", …)` subscription, and because it hands the
 * typed `PanelTaskWire` straight to `applyTaskUpdate`, any drift between the
 * generated type and {@link TaskWire} is a type error at that seam.
 *
 * @module task-toast-subscriber
 */

import { showToast, type Severity, type ToastAdvisory } from "./toast-manager";

// ── Structural wire types (stand-ins for the generated bindings) ─────────

/** The `Advisory` fields this module reads, structurally. */
export interface AdvisoryLike {
  what: string;
  severity: Severity;
}

/** The generated `TaskState`, structurally. */
export type TaskStateLike =
  | { state: "failed"; advisory: AdvisoryLike }
  | { state: "succeeded"; advisories?: AdvisoryLike[] }
  | { state: "running" | "awaiting" | "cancelled" };

/**
 * The generated `PanelTaskWire`, structurally. `scope`/`tone` are
 * widened to `string` deliberately: this module only asks "is it `preview`?"
 * and "is it `inline`?"; restating the full enums here would be a second
 * copy to maintain for no gain.
 */
export interface TaskWire {
  id: string;
  scope: string;
  tone: string;
  title: string | null;
  state: TaskStateLike;
}

/** Structurally `@tauri-apps/api`'s `UnlistenFn`. */
export type Unlisten = () => void;

/**
 * The injected subscription seam: hand this module a way to receive
 * `PanelTaskUpdate` payloads and to stop receiving them.
 */
export type TaskUpdateSubscribe = (
  handler: (payload: TaskWire) => void,
) => Promise<Unlisten>;

// ── Internal state ────────────────────────────────────────────────────────

/** Task ids we've already surfaced a toast for. Cleared on unmount. */
const seen: Set<string> = new Set();
let unlistenFn: Unlisten | null = null;

// ── Pure helpers (unit-tested) ────────────────────────────────────────────

/**
 * Decide whether a wire payload deserves a toast. Exported for unit
 * tests so the rule is independently verifiable.
 */
export function shouldToast(payload: TaskWire): boolean {
  // Preview-scope Jobs (Domain, Deploy, Build) render in the glass
  // progress panel — their failures show there as a row. Suppress the toast for
  // them so the user doesn't get a double-render (panel row + toast). The toast
  // stays the cross-surface failure signal for action-panel / workspace tasks.
  if (payload.scope === "preview") return false;
  // Invariant #4: inline-tone failures live in the badge only; no toast.
  if (payload.tone === "inline") return false;
  if (payload.state.state !== "failed") return false;
  // Post-collapse, recoverability lives in the advisory's severity: a
  // `Blocking` root cause is the unrecoverable case the toast surfaces;
  // `NeedsAction` / `ShippedDegraded` failures are recoverable (no toast).
  if (payload.state.advisory.severity !== "Blocking") return false;
  return true;
}

/**
 * A Workspace/Ambient Job can terminate
 * SUCCESSFULLY while carrying a non-blocking `ShippedDegraded` advisory ("N
 * posts couldn't syndicate"). It is stored on `Succeeded.advisories` (the Rust
 * side proves storage in `lifecycle_descriptor_success_advisory_is_stored_not_lost`)
 * but has no panel render surface — Workspace-scope Jobs don't render in the
 * glass progress panel (that's Preview-scope). Per R12 a non-actionable
 * ShippedDegraded is a QUIET signal: surface it as an info-severity EPHEMERAL
 * toast (no panel-pop, no interrupt), reusing the 6.3 Advisory-shaped toast.
 *
 * Returns the {@link ToastAdvisory} to show, or `null` when there's nothing to
 * surface. Exported so the rule is independently unit-testable.
 *
 * Surfaces ONLY:
 *   - a `succeeded` state (a clean success → null; "success makes no sound")
 *   - carrying at least one `ShippedDegraded` advisory (a `NeedsAction` success
 *     advisory is recoverable elsewhere — inline badge / action panel — not here)
 *   - on a non-Preview scope (Preview-scope advisories render in the panel; the
 *     toast must not double-render them)
 */
export function shippedDegradedAdvisory(payload: TaskWire): ToastAdvisory | null {
  // Preview-scope Jobs render their advisories in the glass progress panel.
  if (payload.scope === "preview") return null;
  if (payload.state.state !== "succeeded") return null;
  const advisories = payload.state.advisories ?? [];
  const degraded = advisories.find((a) => a.severity === "ShippedDegraded");
  if (!degraded) return null;
  // R12: info-severity (ShippedDegraded → "info" tier), ephemeral. No recovery
  // action — a ShippedDegraded is non-actionable by definition.
  return { what: degraded.what, severity: "ShippedDegraded" };
}

/**
 * Format the toast message from a failed-task payload. Combines the
 * task's optional title with the error string.
 *
 *   title "Saving index.md" + error "disk full"
 *     → "Saving index.md: disk full"
 *   no title + error "boom"
 *     → "boom"
 */
export function formatToastMessage(payload: TaskWire): string {
  if (payload.state.state !== "failed") return "";
  const err = payload.state.advisory.what;
  if (payload.title) return `${payload.title}: ${err}`;
  return err;
}

// ── DOM-driving layer ─────────────────────────────────────────────────────

/**
 * Apply a single PanelTaskUpdate. Exported for tests.
 *
 * Dedupes via a process-lifetime `Set<TaskId>`. The task lifecycle of
 * "spawn → progress → failed" produces 3 wire emits; subsequent re-emits
 * of the failed state (e.g., when a panel re-mounts and re-emits state)
 * are silently dropped here.
 */
export function applyTaskUpdate(payload: TaskWire): void {
  const id = String(payload.id);

  if (shouldToast(payload)) {
    if (seen.has(id)) return;
    seen.add(id);

    const msg = formatToastMessage(payload);
    // Invariant #2: Blocking toasts must not have a finite duration —
    // they persist until the user explicitly dismisses them. The user
    // can click the X. id dedupes at the toast-manager level so
    // applyTaskUpdate re-emits on the same TaskId can't double-toast
    // (the seen set short-circuits, but defense-in-depth).
    showToast({ what: msg, severity: "Blocking" }, { persistent: true, id: `task-failed-${id}` });
    return;
  }

  // A successful Workspace/Ambient Job carrying a non-blocking
  // ShippedDegraded advisory surfaces QUIETLY — an info-severity ephemeral
  // toast, deduped by TaskId like the failure path.
  const degraded = shippedDegradedAdvisory(payload);
  if (degraded) {
    if (seen.has(id)) return;
    seen.add(id);
    // 6s — a quiet, informational notice. Distinct toast id so a degraded
    // success can't collide with a failure toast for a different task lifecycle.
    showToast(degraded, { duration: 6000, id: `task-degraded-${id}` });
  }
}

// ── Lifecycle ─────────────────────────────────────────────────────────────

/**
 * Start listening. `subscribe` is the caller's binding to the real
 * `PanelTaskUpdate` stream (see {@link TaskUpdateSubscribe}) — this module
 * never reaches for the event API itself.
 */
export async function mountTaskToastSubscriber(
  subscribe: TaskUpdateSubscribe,
): Promise<() => void> {
  if (unlistenFn) return unmountTaskToastSubscriber;
  unlistenFn = await subscribe(applyTaskUpdate);
  return unmountTaskToastSubscriber;
}

export function unmountTaskToastSubscriber(): void {
  if (unlistenFn) {
    unlistenFn();
    unlistenFn = null;
  }
  seen.clear();
}

// ── Test hooks ────────────────────────────────────────────────────────────

export const __resetForTests = unmountTaskToastSubscriber;

export function __seenCountForTests(): number {
  return seen.size;
}
