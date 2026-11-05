// Toast vocabulary — the types every other toast module speaks.
//
// This is the DAG's root: it imports nothing. `toast-runtime` (host seam),
// `toast-dom` (element assembly) and `toast-manager` (registry + public API)
// all sit on top of it, in that order. Everything public here is re-exported
// from `toast-manager`, which stays the single import site for callers.

/**
 * Recovery effort of an advisory. Structurally identical to the generated
 * bindings' `Severity` — declared here so this module doesn't import the
 * generated bindings. Drift is caught where the two meet: any call site passing a
 * bindings `Severity` into {@link ToastAdvisory} type-errors if the unions
 * diverge.
 */
export type Severity = "ShippedDegraded" | "NeedsAction" | "Blocking";

/** The generated `Result<T, string>` shape, structurally. */
export type ResultLike<T> =
  | { status: 'ok'; data: T }
  | { status: 'error'; error: string };

/**
 * Action button in a toast
 */
export interface ToastAction {
  /** Button text, e.g., "View site" */
  label: string;
  /** URL to open in system browser when clicked */
  url?: string;
  /** Custom click handler (takes priority over url) */
  onClick?: () => void;
  /**
   * Visual weight of the action, when a toast offers a clear primary/secondary
   * pair (e.g. the update offer's "Update & Restart" vs "Release notes"). Mirrors moss's established filled-primary + muted-text pair
   * (`.moss-setup-btn--primary` / `.moss-link--subtle`):
   *   - `'primary'`   → a filled accent button. It's a command, not a "go to X"
   *                     navigation, so it drops the trailing "→" (same reasoning
   *                     as `copyValue`).
   *   - `'secondary'` → a muted text link (keeps the "→").
   * Omitted → the default accent text link, unchanged. Ignored when `copyValue`
   * is set (a copy affordance has its own canonical rendering).
   */
  variant?: "primary" | "secondary";
  /**
   * Renders this action as the canonical `createCopyButton` (icon + 2s
   * "Copied" feedback + `data-tooltip`) instead of the anchor-link style, and
   * writes this value to the clipboard on click. Takes priority over
   * `url`/`onClick` — a copy affordance is not a navigation, so it doesn't get
   * the trailing "→" arrow the anchor style uses for "go to X" actions.
   *
   * For a value that belongs on the message's OWN line, use
   * {@link ToastOptions.inlineCopy} instead — an action always renders a row
   * below the message.
   */
  copyValue?: string;
}

/**
 * A toast is an Advisory rendered ephemerally ("a toast was
 * never a separate thing"). The renderer derives its surface (icon/class) from
 * the shared `Severity` value object; there is no private toast severity enum.
 *
 * `severity` maps to the visual tier:
 *   - `ShippedDegraded` → `info`    (a quiet, informational notice)
 *   - `NeedsAction`     → `warning` (amber — "you should know")
 *   - `Blocking`        → `error`   (the operation could not proceed)
 *
 * A pure confirmation ("Copied", "Site published") is NOT an advisory — it is
 * the design's `Ack`. Use `showAck` for those; they are success-styled.
 */
export interface ToastAdvisory {
  /** What happened — the toast's message text. */
  what: string;
  /** Recovery effort; drives the icon/class. */
  severity: Severity;
  /** Optional recovery affordance, rendered as a single action button. */
  action?: ToastAction;
  /** Auto-dismiss after this many ms (bounded by the caller's options). */
  durationMs?: number;
}

/**
 * Toast options for extended functionality
 */
export interface ToastOptions {
  /**
   * Auto-dismiss delay in ms. There is no universal default — each surface
   * supplies its own when the caller omits this: `showAck` and
   * `showOrUpdateToast` default to 3000; `showToast` advisories default to
   * 6000. The value is used as-is (no clamping). Omit AND pass
   * `persistent: true` for a toast that never self-dismisses.
   */
  duration?: number;
  /** URL to display with click-to-open and copy button (legacy, use actions instead) */
  url?: string;
  /** Display text for URL link (default: shows raw URL) */
  urlText?: string;
  /** Optional ID for updating existing toast instead of creating new one */
  id?: string;
  /** Action buttons (opens URL in system browser) */
  actions?: ToastAction[];
  /** If true, toast stays until user dismisses (no auto-dismiss) */
  persistent?: boolean;
  /** If false, hide the X close button (default: true) */
  dismissible?: boolean;
  /**
   * Render a progress bar in the toast. `0`–`100` is a determinate fill
   * (clamped); a negative value (e.g. `-1`) is indeterminate. Omit for no
   * bar. On a coalescing `showToast` (same `id`) the bar is updated on the
   * SAME element so its CSS width-transition animates instead of resetting.
   */
  progress?: number;
  /**
   * Overrides the leading icon with a small colored status dot instead of the
   * check/triangle glyph — for a two-phase "pending → confirmed" flow (send
   * logs, check for updates) where the generic checkmark can't distinguish
   * "still working" from "done". `"green"` also shrinks the toast to fit its
   * message on one line (see `.moss-toast--compact`), matching the compact
   * one-liner a confirmation reads as. Omit for the default icon.
   */
  dot?: "yellow" | "green";
  /**
   * Render an icon-only `createCopyButton` that copies this value, placed
   * INSIDE `.moss-toast-message` — on the message's own line.
   *
   * This is the affordance to reach for when the thing to copy is part of the
   * sentence ("Logs sent: LOG-ABCD-0001"), rather than a follow-up step. An
   * {@link ToastAction.copyValue} entry renders the same button but lands in
   * `.moss-toast-actions`, which the locked grid pins to row 2 — i.e. always
   * on the NEXT line, which reads as a second, heavier step. See the layout
   * header in `styles/shared/toast.css`.
   *
   * Empty string clears it. Composes with `actions`: a toast can carry an
   * inline copy AND a real action row.
   */
  inlineCopy?: string;
  /**
   * A link placed INSIDE `.moss-toast-message`, on the message's own line —
   * the same layout slot as {@link inlineCopy}, for the same reason.
   *
   * Reach for this when the destination is the END OF THE SENTENCE rather
   * than a follow-up step: "Published to example-garden.mosspub.com". An
   * {@link ToastAction} with a `url` renders the same navigation as a row-2
   * button, which would force the sentence to name the destination twice
   * ("Site published — example-garden.mosspub.com" + "View site →").
   *
   * Clicks route through the host runtime, so a `.onion` URL still opens in
   * Tor Browser exactly as an action's would. Omit or pass `undefined` to
   * clear it.
   */
  inlineLink?: { label: string; url: string };
  /**
   * Hold the auto-dismiss countdown while the window is hidden or unfocused,
   * so `duration` measures time the user was actually here.
   *
   * The problem this solves: a confirmation for something the user kicked off
   * and then walked away from (a publish, an export) lands on an unwatched
   * screen and a plain timer eats it. The old fix was `persistent: true`,
   * which trades a missed toast for one that never leaves. This keeps the
   * dismissal and only spends it on attention.
   *
   * Ignored when `persistent` is set (there is no countdown to hold).
   */
  awaitAttention?: boolean;
}

/**
 * Toast event payload (used by show-toast event from plugins and Rust backend)
 */
export interface ToastPayload {
  message: string;
  variant?: "success" | "error" | "info" | "warning";
  actions?: ToastAction[];
  duration?: number;
  persistent?: boolean;
  dismissible?: boolean;
  id?: string;
}

/**
 * The internal visual tier a toast renders at. NOT a public input vocabulary —
 * callers speak {@link ToastAdvisory.severity} (no private severity enum on
 * the public surface). `success` exists only for `showAck` confirmations;
 * advisories never produce `success`.
 */
export type ToastVariant = "success" | "error" | "warning" | "info";

/** Map a recovery `Severity` to the visual tier. */
export function severityToVariant(severity: Severity): ToastVariant {
  switch (severity) {
    case "Blocking":
      return "error";
    case "NeedsAction":
      return "warning";
    case "ShippedDegraded":
      return "info";
  }
}
