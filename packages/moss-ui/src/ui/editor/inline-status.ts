/**
 * InlineStatus — Inline renderer for PanelTask events with
 * `tone === "inline"`. Stripe-style status badges anchored adjacent to
 * the element the task describes.
 *
 * This renderer
 * carries two behaviors selected by the task's `fade_after_ms` field:
 *
 *  - `fade_after_ms = null` → persistent live status. The badge stays
 *    visible until the task transitions to a terminal state OR the user
 *    dismisses it. Used for in-progress receipts (e.g., "Saving…").
 *  - `fade_after_ms = N`    → terminal-state receipt. After succeeded /
 *    failed / cancelled, the badge stays for N ms then fades. Used for
 *    "Saved · 2s ago"-style transient confirmations.
 *
 * Anchor:
 *   The badge anchors to the DOM node carrying
 *   `data-inline-task-anchor="<task-kind>"` (e.g., the chip bar carries
 *   `data-inline-task-anchor="save"`). Falls back to `#moss-inline-status-fallback`
 *   if no anchor is present.
 *
 * Multi-task aggregation:
 *   The Inline renderer paints one badge per task. We track a
 *   `Map<TaskId, BadgeState>` and paint each beside the resolved anchor;
 *   if two tasks share the same kind anchor, the most-recently-updated wins
 *   (the older badge is removed). This matches the "Saved" receipt UX —
 *   a fresh save replaces the prior "Saved · 2s ago" rather than stacking.
 *
 * Accessibility:
 *   Each badge carries `role="status" aria-live="polite"`. The fade
 *   transition respects `prefers-reduced-motion: reduce` via CSS (the
 *   badge appears + disappears instantly under reduced motion).
 *
 * @module inline-status
 */

import { onMossEvent } from "../utils/moss-events";
import type { PanelTaskWire, TaskState, TaskKind } from "@symbiosis-lab/moss-ui";
import type { Unlisten } from "../host";
import { t } from "../i18n/index";

// ── Constants ─────────────────────────────────────────────────────────────

/** CSS class applied to the badge wrapper. */
const BADGE_CLASS = "moss-inline-status";

/** Container for badges that don't have a kind-specific anchor. */
const FALLBACK_ANCHOR_ID = "moss-inline-status-fallback";

/** Attribute used to find the anchor element for a given TaskKind. */
const ANCHOR_ATTR = "data-inline-task-anchor";

// ── Pure helpers (unit-tested) ────────────────────────────────────────────

/**
 * Compute the human-readable label shown on the badge. Combines the
 * task's optional title and the state-derived noun (e.g., "Saving",
 * "Saved", "Failed: <error>").
 *
 * Examples:
 *   running + title "Saving index.md" + fraction 0.5
 *     → "Saving index.md"
 *   succeeded + receipt "Saved"
 *     → "Saved"
 *   succeeded + no receipt + title "Save"
 *     → "Save complete"
 *   failed + error "Disk full"
 *     → "Failed: Disk full"
 *   cancelled
 *     → "Cancelled"
 */
export function formatBadgeLabel(payload: PanelTaskWire): string {
  const state = payload.state;
  switch (state.state) {
    case "running":
      // Prefer title if present, otherwise show the state message.
      if (payload.title) return payload.title;
      if (state.message) return state.message;
      return t('status.working');
    case "awaiting":
      return state.directive;
    case "succeeded":
      if (state.receipt) return state.receipt;
      if (payload.title) return t('status.complete').replace('{title}', payload.title);
      return t('status.done');
    case "failed":
      return t('status.failed').replace('{error}', state.advisory.what);
    case "cancelled":
      return t('status.cancelled');
  }
}

/**
 * Map a `TaskState` discriminant to the badge's status class. The class
 * controls glyph + color (filled green dot for success, hollow ring for
 * failed, etc.).
 */
export function badgeStatusClass(state: TaskState): string {
  switch (state.state) {
    case "running":
      return "is-running";
    case "awaiting":
      return "is-awaiting";
    case "succeeded":
      return "is-succeeded";
    case "failed":
      return "is-failed";
    case "cancelled":
      return "is-cancelled";
  }
}

/** Whether a state is terminal (succeeded / failed / cancelled). */
export function isTerminal(state: TaskState): boolean {
  return (
    state.state === "succeeded" ||
    state.state === "failed" ||
    state.state === "cancelled"
  );
}

// ── Internal state ────────────────────────────────────────────────────────

interface BadgeRecord {
  taskId: string;
  kind: TaskKind;
  element: HTMLDivElement;
  fadeTimer: ReturnType<typeof setTimeout> | null;
  removeTimer: ReturnType<typeof setTimeout> | null;
  /** Anchor element the badge currently sits on. */
  anchor: HTMLElement;
}

let unlistenFn: Unlisten | null = null;
const badges: Map<string, BadgeRecord> = new Map();

// ── DOM layer ─────────────────────────────────────────────────────────────

function findAnchor(kind: TaskKind): HTMLElement {
  // Prefer kind-specific anchor (e.g., save chip-bar slot).
  const explicit = document.querySelector(
    `[${ANCHOR_ATTR}="${kind}"]`,
  ) as HTMLElement | null;
  if (explicit) return explicit;
  // Fallback container for tasks without a registered anchor. Created on
  // demand so this module works in jsdom without page-level setup.
  let fallback = document.getElementById(FALLBACK_ANCHOR_ID);
  if (!fallback) {
    fallback = document.createElement("div");
    fallback.id = FALLBACK_ANCHOR_ID;
    fallback.className = "moss-inline-status-fallback";
    document.body.appendChild(fallback);
  }
  return fallback;
}

function makeBadgeElement(payload: PanelTaskWire): HTMLDivElement {
  const el = document.createElement("div");
  el.className = `${BADGE_CLASS} ${badgeStatusClass(payload.state)}`;
  el.setAttribute("role", "status");
  el.setAttribute("aria-live", "polite");
  // The status glyph + label. Pure DOM (no innerHTML) keeps it safe for
  // arbitrary task labels.
  const glyph = document.createElement("span");
  glyph.className = "moss-inline-status__glyph";
  glyph.setAttribute("aria-hidden", "true");
  el.appendChild(glyph);
  const label = document.createElement("span");
  label.className = "moss-inline-status__label";
  label.textContent = formatBadgeLabel(payload);
  el.appendChild(label);
  return el;
}

function updateBadgeElement(el: HTMLDivElement, payload: PanelTaskWire): void {
  el.className = `${BADGE_CLASS} ${badgeStatusClass(payload.state)}`;
  const label = el.querySelector(".moss-inline-status__label");
  if (label) label.textContent = formatBadgeLabel(payload);
}

function removeBadge(taskId: string): void {
  const rec = badges.get(taskId);
  if (!rec) return;
  if (rec.fadeTimer) clearTimeout(rec.fadeTimer);
  if (rec.removeTimer) clearTimeout(rec.removeTimer);
  rec.element.remove();
  badges.delete(taskId);
}

/**
 * Drive a single PanelTaskUpdate. Exported so tests + the visual-regression
 * harness can poke the renderer without going through the Tauri bus.
 */
export function applyTaskUpdate(payload: PanelTaskWire): void {
  const taskId = String(payload.id);

  // Tone escalation: the router may move a
  // task from inline → awaiting (or → ambient/narrated/notify) mid-flight.
  // When that happens the wire payload carries the NEW tone, and the
  // Inline early-return below would otherwise leak our badge — the task
  // is now owned by a different renderer but our DOM node would linger
  // until process death. Drop the badge before the early return so the
  // new owner has a clean slate.
  if (payload.tone !== "inline") {
    if (badges.has(taskId)) removeBadge(taskId);
    return;
  }

  // Silent-save policy: a Save
  // task lights inline-status ONLY on failure. Running/succeeded/cancelled
  // saves render no badge. A non-failed Save update also clears a prior Save
  // badge on the SAME anchor+kind — e.g. a "Failed: …" the user has now
  // resolved (the next save attempt or success clears the stale error). The
  // anchor+kind scope mirrors the existing dedup below so it cannot wipe an
  // unrelated badge.
  //
  // NOTE: like that dedup, this is not window_id-scoped. With a single editor
  // webview (today) that is correct. Multi-editor-window scoping by
  // payload.window_id is a pre-existing inline-status limitation, out of scope
  // here; tracked as a follow-up.
  if (payload.kind === "save" && payload.state.state !== "failed") {
    const anchor = document.querySelector(`[${ANCHOR_ATTR}="${payload.kind}"]`);
    for (const [id, rec] of Array.from(badges)) {
      if (rec.kind === "save" && (!anchor || rec.anchor === anchor)) {
        removeBadge(id);
      }
    }
    return;
  }

  let rec = badges.get(taskId);

  if (!rec) {
    const anchor = findAnchor(payload.kind);
    const element = makeBadgeElement(payload);
    anchor.appendChild(element);
    rec = {
      taskId,
      kind: payload.kind,
      element,
      fadeTimer: null,
      removeTimer: null,
      anchor,
    };
    badges.set(taskId, rec);
    // Replace any older badge on the same anchor for the same kind so we
    // don't stack receipts (one "Saved" replaces the prior "Saved · 2s ago").
    for (const [otherId, other] of badges) {
      if (
        otherId !== taskId &&
        other.anchor === anchor &&
        other.kind === payload.kind
      ) {
        removeBadge(otherId);
      }
    }
  } else {
    updateBadgeElement(rec.element, payload);
  }

  const terminal = isTerminal(payload.state);
  if (terminal) {
    // Persistent (fade_after === null) badge in terminal state still
    // disappears — the task is done, there is nothing to keep alive.
    // Default fade for null fade_after_ms is "stay forever until next
    // task replaces me," matching the spec line "stays visible until the
    // task completes or user dismisses." For terminal-state receipts
    // with a fade_after_ms, we honor that.
    if (rec.fadeTimer) clearTimeout(rec.fadeTimer);
    if (rec.removeTimer) clearTimeout(rec.removeTimer);
    rec.fadeTimer = null;
    rec.removeTimer = null;

    const fadeAfter = payload.fade_after_ms;
    if (fadeAfter != null) {
      // Wait fadeAfter ms then start the CSS fade, then DOM-remove.
      rec.fadeTimer = setTimeout(() => {
        rec!.element.classList.add("is-fading");
        rec!.removeTimer = setTimeout(() => removeBadge(taskId), 400);
      }, Number(fadeAfter));
    }
    // fade_after_ms == null → terminal badge stays. Caller (the save
    // wiring) can explicitly call removeBadge(taskId) on the next task
    // spawn for the same anchor.
  } else {
    // Non-terminal state — cancel any pending fade (rare: a task that
    // went succeeded → running due to a retry).
    if (rec.fadeTimer) {
      clearTimeout(rec.fadeTimer);
      rec.fadeTimer = null;
    }
    if (rec.removeTimer) {
      clearTimeout(rec.removeTimer);
      rec.removeTimer = null;
    }
    rec.element.classList.remove("is-fading");
  }
}

// ── Lifecycle ─────────────────────────────────────────────────────────────

/**
 * Mount the renderer: subscribe to MossEvent::PanelTaskUpdate. Idempotent —
 * calling twice will not register duplicate listeners.
 */
export async function mountInlineStatus(): Promise<() => void> {
  if (unlistenFn) return unmountInlineStatus;
  unlistenFn = await onMossEvent("PanelTaskUpdate", (msg) => {
    applyTaskUpdate(msg.payload);
  });
  return unmountInlineStatus;
}

export function unmountInlineStatus(): void {
  if (unlistenFn) {
    unlistenFn();
    unlistenFn = null;
  }
  for (const id of Array.from(badges.keys())) {
    removeBadge(id);
  }
  document.getElementById(FALLBACK_ANCHOR_ID)?.remove();
}

// ── Test hooks ────────────────────────────────────────────────────────────

/** Reset module-private state without touching the DOM. */
export function __resetForTests(): void {
  if (unlistenFn) {
    unlistenFn();
    unlistenFn = null;
  }
  for (const id of Array.from(badges.keys())) {
    removeBadge(id);
  }
}

/** Read-only badge count for tests. */
export function __badgeCountForTests(): number {
  return badges.size;
}
