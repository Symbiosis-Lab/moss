/**
 * Rendering of Rust-sent toast payloads (`ShowToast` / `ShowToastUpdate`), and
 * the one mount every window uses to receive them.
 *
 * Shared because both webview bundles need it: the shell's feedback layer
 * (preview, launcher) and the editor panel, which a dependency rule keeps
 * from importing the shell. The Tauri calls are injected: shared/ is pure.
 */

import * as toastManager from "./toast-manager";
import type { Unlisten } from "./task-toast-subscriber";

type Listen = (event: string, handler: (payload: unknown) => void) => Promise<Unlisten>;

/**
 * Runs the action a Rust-built toast button names. Every surface wires it to
 * the host's one toast-action command, so a button does the same thing
 * whichever window drew it.
 */
export type OnToastAction = (key: string) => void;

// The wire shape of a Rust `ToastPayload`, stated structurally in toast-manager's
// own types: shared/ is pure (no bindings, no platform seam), so the Tauri calls are injected.
// `bindings.ToastPayload` is assignable to it, which the callers prove.
type ToastOpts = NonNullable<Parameters<typeof toastManager.showAck>[1]>;
export interface ToastPayload {
  what: string;
  severity: Parameters<typeof toastManager.showToast>[0]["severity"];
  ack: boolean;
  /** `event` is `open-url:<url>` for a link, else an action key for Rust. */
  actions: { label: string; event: string; variant?: "primary" | "secondary" | null }[] | null;
  inline_link?: ToastOpts["inlineLink"] | null;
  duration: string | null;
  await_attention?: boolean | null;
  persistent?: boolean | null;
  dismissible?: boolean | null;
  id?: string | null;
  dot?: ToastOpts["dot"] | null;
  /** `0`–`100`, or negative for indeterminate; absent draws no bar. */
  progress?: number | null;
}

/** A queued event; only the toast kinds are drawn. */
export type QueuedEvent = { kind: string; payload?: unknown };

/** A wire action as a toast button: a link opens its URL, a key goes back to Rust. */
function toButtons(actions: ToastPayload["actions"], onAction: OnToastAction): ToastOpts["actions"] {
  // An action with no event names nothing to do: a button for it would be dead.
  return actions?.filter(({ event }) => event).map(({ label, event, variant }) => ({
    label,
    variant: variant ?? undefined,
    ...(event.startsWith("open-url:")
      ? { url: event.slice("open-url:".length) }
      : { onClick: () => onAction(event) }),
  }));
}

function renderToast(payload: ToastPayload, onAction: OnToastAction): void {
  const { what, severity, ack, actions, inline_link, duration, await_attention, persistent, dismissible, id, dot, progress } = payload;

  // Bound duration to 2000–30000 ms (duration is i64 → string | null).
  const boundedDuration = duration
    ? Math.min(30000, Math.max(2000, Number(duration)))
    : 5000;

  const opts = {
    duration: persistent ? undefined : boundedDuration,
    actions: toButtons(actions, onAction),
    inlineCopy: sendLogTicketId(what),
    inlineLink: inline_link ?? undefined,
    // Spend the duration on attention rather than wall-clock — see
    // ToastOptions.awaitAttention. Only meaningful with a duration, which is
    // exactly when `persistent` is unset.
    awaitAttention: await_attention ?? undefined,
    dismissible: dismissible ?? true,
    id: id ?? undefined,
    dot: dot ?? undefined,
    progress: progress ?? undefined,
  };

  // an ack is a pure confirmation; anything else is an advisory.
  if (ack) {
    toastManager.showAck(what, opts);
  } else {
    toastManager.showToast({ what, severity }, opts);
  }
}

function renderToastUpdate(payload: ToastPayload, onAction: OnToastAction): void {
  const { id, what, severity, ack, actions, inline_link, duration, await_attention, persistent, dismissible, dot, progress } = payload;
  if (!id) return; // updates require an id

  // Map to a visual type for updateToast
  const uiType: "success" | "error" | "warning" | "info" | undefined =
    ack ? "success"
    : severity === "Blocking" ? "error"
    : severity === "NeedsAction" ? "warning"
    : severity === "ShippedDegraded" ? "info" : undefined;

  // The send-log success ticket id gets an inline copy button ON the
  // message's line. `""` on every other update clears a stale one (e.g. a
  // retry that fails after a success).
  const inlineCopy = sendLogTicketId(what) ?? "";

  toastManager.updateToast(id, {
    message: what,
    type: uiType,
    actions: toButtons(actions, onAction),
    inlineCopy,
    // `null` (not undefined) so an update that carries no link CLEARS a stale
    // one — an update is a faithful statement of the toast's current shape.
    inlineLink: inline_link ?? null,
    awaitAttention: await_attention ?? undefined,
    duration: persistent ? undefined : (duration ? Number(duration) : undefined),
    dismissible: dismissible ?? undefined,
    dot: dot ?? undefined,
    // `null` removes a bar the update no longer carries, like `inlineLink`.
    progress: progress ?? null,
  });
}

/**
 * If `what` is the send-log success message ("Logs sent: <ticketId>"),
 * return the reference id; otherwise `undefined`. An older server reply
 * carries no ticket id, a bare "Logs sent" — nothing to copy.
 *
 * The id goes to the toast as `inlineCopy`, NOT as an action: an action lands in
 * `.moss-toast-actions`, which the locked toast grid pins to row 2, so the
 * button would sit on a second line under a message that reads as one line.
 * `inlineCopy` renders the same canonical copy button inside the message.
 */
export function sendLogTicketId(what: string): string | undefined {
  return what.match(/^Logs sent:\s*(\S+)/)?.[1];
}

/** What a surface injects to receive Rust-sent toasts: shared/ names no Tauri API. */
export interface ToastSurface {
  /**
   * This webview's label. The host addresses a surface by event NAMES that
   * carry it (one ping and one dismiss event per label), because every
   * webview hears every event: a shared name would make each window drain, and
   * a shared dismiss would also remove the copy a toast just moved to.
   */
  label: "launcher" | "preview" | "action-panel";
  listen: Listen;
  /**
   * Takes what Rust queued for THIS surface (Rust: `take_pending_toasts`).
   * `mounted` is true on the one drain a mount makes: Rust then also hands
   * over the app's standing toast (the update offer).
   */
  take: (mounted: boolean) => Promise<QueuedEvent[]>;
  onAction: OnToastAction;
  /** Decides when a toast is drawn, and whether; the shell holds toasts back under its waiting overlay. */
  gate?: (payload: ToastPayload, patch: boolean, draw: () => void) => void;
}

/**
 * Receive the toasts Rust routes to this surface. Rust queues a toast for the
 * one surface the user can see and pings it; nothing is broadcast, so no two
 * windows draw the same toast and the editor panel is a destination like any
 * other.
 *
 * Drains once at mount, so a ping that landed while this webview was booting or
 * reloading is not lost. `take` returns only this surface's own queue, so the
 * hidden pre-warmed editor of a site takes nothing meant for another window.
 */
export async function mountToasts(surface: ToastSurface): Promise<Unlisten[]> {
  const drain = async (mounted: boolean): Promise<void> => {
    for (const event of await surface.take(mounted)) renderQueued(surface, event);
  };
  // Listeners first: a ping landing mid-drain is then not lost.
  const unlistenPing = await surface.listen(`app-feedback-pending-${surface.label}`, () => {
    void drain(false).catch(() => {});
  });
  const unlistenDismiss = await surface.listen(`app-feedback-dismiss-${surface.label}`, (payload) => {
    const id = (payload as { id?: string } | null)?.id;
    if (id) toastManager.removeToastById(id);
  });
  void drain(true).catch(() => {});
  return [unlistenPing, unlistenDismiss];
}

/** Draw one queued toast on `surface`, through its gate. */
export function renderQueued(surface: ToastSurface, event: QueuedEvent): void {
  const patch = event.kind === "ShowToastUpdate";
  if (!patch && event.kind !== "ShowToast") return;
  const payload = event.payload as ToastPayload;
  if (patch && !payload.id) return; // a patch names the toast it changes
  const draw = (): void => (patch ? renderToastUpdate : renderToast)(payload, surface.onAction);
  if (surface.gate) surface.gate(payload, patch, draw);
  else draw();
}
