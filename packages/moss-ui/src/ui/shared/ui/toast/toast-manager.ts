// Toast lifecycle: the live-toast registry, the create/update-in-place render
// core, and the public API every surface calls.
//
// This is the top of a four-module DAG, and the ONE import site for callers —
// everything public from the modules below is re-exported here, so no call site
// needs to know the split exists:
//
//   toast-types    vocabulary; imports nothing
//   toast-runtime  the injected host seam (log sink, openers)
//   toast-dom      element assembly + reconcilers, no lifecycle
//   toast-manager  registry, dismissal timing, removal, public API  ← you are here
//
// The whole stack is pure: no host globals, importable by every host. What a toast needs from the host is injected via
// `configureToastRuntime`; see `toast-runtime.ts` for why that seam is a
// one-time global rather than a per-call parameter.

import { t } from '../../../i18n';
import { getToastRuntime } from './toast-runtime';
import {
  applyProgressBar,
  buildActionsElement,
  reconcileCloseButton,
  reconcileInlineCopy,
  reconcileInlineLink,
  setToastIcon,
  setToastMessage,
  toastIconHtml,
} from './toast-dom';
import { severityToVariant, type ToastVariant } from './toast-types';
import type {
  Severity,
  ToastAction,
  ToastAdvisory,
  ToastOptions,
  ToastPayload,
} from './toast-types';

// The public surface is re-exported, not redefined: `toast-manager` remains the
// single module callers import from.
export type {
  ResultLike,
  Severity,
  ToastAction,
  ToastAdvisory,
  ToastOptions,
  ToastPayload,
} from './toast-types';
export { configureToastRuntime, _resetToastRuntime, type ToastRuntime, type UrlOpenRoute } from './toast-runtime';

// ── Dismissal timing ────────────────────────────────────────────────────────
// Toasts have exactly one lifecycle question — "when does this go away?" — and
// two answers. Most want a plain timer. A few (a publish confirmation, an
// export receipt) fire for work the user started and then walked away from,
// where a plain timer measures an empty screen: the toast is gone before they
// ever look. `attentionGated` advances the countdown only while this document
// is VISIBLE, so `duration` means "seconds the toast was on screen".
//
// Both shapes share one handle type, so the registry has a single field to
// cancel and no call site branches — the choice is made once, at arm time.

/** A running countdown. `cancel()` is idempotent and safe after it has fired. */
export interface DismissTimer {
  cancel(): void;
}

/**
 * True when this document is on screen.
 *
 * Visibility, NOT `document.hasFocus()` — and that is the whole subtlety.
 * moss's shell hosts SIBLING child webviews (the editor/composer action panel,
 * the plugin browser panel), and DOM focus moving into one of them blurs the
 * shell while the window stays frontmost and the toast stays in plain sight.
 * Gating on focus would therefore pause the countdown the moment the author
 * clicks back into the editor — turning a 9s toast into a permanent one, which
 * is precisely the behaviour `awaitAttention` was added to replace.
 *
 * The failure direction matters too: if a platform never reports `hidden`, this
 * degrades to a plain wall-clock timer. That is the safe way to be wrong.
 *
 * A true "is the moss window frontmost?" signal would have to come from Tauri's
 * window focus events via the `toast-runtime` host seam; nothing needs that
 * precision yet.
 */
function userIsPresent(): boolean {
  return document.visibilityState === "visible";
}

/**
 * Arm a countdown for `durationMs`, calling `onElapsed` once when it runs out.
 *
 * With `attentionGated`, the countdown pauses whenever the document goes hidden
 * and resumes with the remaining time when it comes back — so a toast that
 * arrives while moss is minimised or hidden is still there when the user
 * returns, and only then starts its clock.
 */
export function armDismissTimer(
  durationMs: number,
  onElapsed: () => void,
  attentionGated = false,
): DismissTimer {
  if (!attentionGated) {
    const id = setTimeout(onElapsed, durationMs);
    return { cancel: () => clearTimeout(id) };
  }

  let remaining = durationMs;
  let startedAt = 0;
  let id: ReturnType<typeof setTimeout> | undefined;
  let done = false;

  const fire = () => {
    done = true;
    detach();
    onElapsed();
  };

  const pause = () => {
    if (id === undefined) return;
    clearTimeout(id);
    id = undefined;
    // Date.now() rather than a monotonic clock: a wall-clock jump can only
    // make a toast leave sooner, which is the harmless direction.
    remaining = Math.max(0, remaining - (Date.now() - startedAt));
  };

  const resume = () => {
    if (done || id !== undefined) return;
    startedAt = Date.now();
    id = setTimeout(fire, remaining);
  };

  const sync = () => (userIsPresent() ? resume() : pause());


  function detach(): void {
    document.removeEventListener("visibilitychange", sync);
  }

  document.addEventListener("visibilitychange", sync);
  sync();

  return {
    cancel: () => {
      done = true;
      clearTimeout(id);
      id = undefined;
      detach();
    },
  };
}

/** Tracked toast state for updatable toasts */
interface TrackedToast {
  element: HTMLDivElement;
  /** Auto-dismiss countdown; absent for persistent toasts. Always cancel via
   *  `timer?.cancel()` — it may be attention-gated and holding a listener. */
  timer?: DismissTimer;
  /** Whether `timer` is attention-gated, so a duration-only patch can re-arm
   *  the same KIND of countdown instead of silently downgrading it to
   *  wall-clock (see updateToast). */
  awaitAttention?: boolean;
  container: HTMLElement;
}

/** Track active toasts by ID for update-in-place functionality */
const activeToasts: Map<string, TrackedToast> = new Map();

/**
 * Clear all tracked toasts (used for testing)
 * @internal
 */
export function _clearTrackedToasts(): void {
  for (const [_id, tracked] of activeToasts) {
    tracked.timer?.cancel();
  }
  activeToasts.clear();
}

/**
 * Update a tracked toast in place to match the given message/type/options,
 * WITHOUT recreating the element (no slide-in replay). This is the coalescing
 * path taken by `showToast` when called again with an existing `id`. It is a
 * faithful replace — actions and the progress bar reflect the new options
 * exactly (omitted ⇒ removed), matching what a fresh toast would render.
 */
function updateExistingToast(
  tracked: TrackedToast,
  message: string,
  type: ToastVariant,
  options: ToastOptions
): void {
  const { element } = tracked;
  const id = options.id as string;
  const dismissible = options.dismissible ?? true;
  // `persistent` overrides any duration → no auto-dismiss timer.
  const duration = options.persistent ? undefined : options.duration;

  element.className = `moss-toast ${type}`;
  setToastIcon(element, type, options.dot);

  // A faithful full replace: an omitted `inlineCopy`/`inlineLink` clears a
  // prior one, so pass them through rather than letting setToastMessage carry
  // the live values forward.
  setToastMessage(element, message, options.inlineCopy ?? "", options.inlineLink ?? null);
  const messageEl = element.querySelector(".moss-toast-message");

  // Actions: replace entirely so a call with none clears prior buttons.
  element.querySelector(".moss-toast-actions")?.remove();
  if (options.actions?.length) {
    messageEl?.after(buildActionsElement(options.actions));
  }

  reconcileCloseButton(element, dismissible, () => removeToastById(id));
  applyProgressBar(element, options.progress);

  // Reset the auto-dismiss timer: clear the old one, arm a new one only when
  // a duration is set (omitted ⇒ persistent ⇒ no timer).
  tracked.timer?.cancel();
  tracked.awaitAttention = options.awaitAttention;
  tracked.timer =
    duration !== undefined
      ? armDismissTimer(duration, () => removeToastById(id), options.awaitAttention)
      : undefined;
}

/**
 * Render a toast at a given visual `variant`. Internal core shared by the
 * public {@link showToast} (advisory), {@link showAck} (confirmation), and the
 * legacy {@link showOrUpdateToast} alias.
 *
 * If `options.id` matches a live toast, this COALESCES: it updates that toast
 * in place (a faithful full replace — message, type/icon, actions, dismissible
 * and progress all reflect the new options; omitted ⇒ removed) instead of
 * recreating the element. That avoids the slide-in replay (flicker) and toast
 * stacking (spam) that a per-event create loop would otherwise cause. For a
 * partial patch of only some fields, use {@link updateToast} instead.
 */
function renderToast(
  message: string,
  variant: ToastVariant,
  options: ToastOptions = {}
): void {
  const type = variant;
  const container = document.getElementById("moss-toast-container");
  if (!container) return;

  // No message content — that's the payload, not the diagnostic. id/variant
  // only, and the id doubles as the key a duplicate/coalesce is keyed on.
  // Single choke point: every showToast/showAck/showOrUpdateToast call, plus
  // the overlay-toast-gate's deferred flush, ends up here.
  // Goes through the
  // injected runtime, not a direct `log.ts` import — toast-manager
  // stays pure, like `logError` right below it already does.
  getToastRuntime().logInfo(`[toast] shown id=${options.id ?? '(none)'} variant=${variant}`);

  // `persistent` overrides any duration → no auto-dismiss timer. (undefined
  // duration is also persistent.)
  const duration = options.persistent ? undefined : options.duration;
  const id = options.id;
  const actions = options.actions;
  const dismissible = options.dismissible ?? true;

  // Coalesce: if a live toast with this id already exists, update it in
  // place rather than tearing it down and rebuilding (which replays the
  // slide-in animation → flicker, and with no id stacks duplicates → spam).
  // Progress toasts and any caller that reuses an id get this for free.
  if (id) {
    const existing = activeToasts.get(id);
    if (existing && container.contains(existing.element)) {
      updateExistingToast(existing, message, type, options);
      return;
    }
  }

  // Create toast element
  const toast = document.createElement("div");
  toast.className = `moss-toast ${type}`;
  if (options.dot === "green") toast.classList.add("moss-toast--compact");
  if (id) toast.dataset.toastId = id;

  // SVG icons by type — `dot` overrides with a colored status dot.
  const iconSvg = toastIconHtml(type, options.dot);

  // The message is TEXT, set after the shell rather than interpolated into it.
  // Toast text carries vault data — a filename in a delete confirmation, an
  // email in a subscriber prompt — and `innerHTML` would execute markup a file
  // name can contain. The update path (`setToastMessage`) has always used
  // `textContent`; this is the create path agreeing with it.
  toast.innerHTML = `
    ${iconSvg}
    <span class="moss-toast-message"></span>
    ${dismissible ? `<button class="moss-toast-close" aria-label="${t('editmisc.close_notification')}">\u2715</button>` : ''}
  `;
  const messageSpan = toast.querySelector(".moss-toast-message");
  if (messageSpan) messageSpan.textContent = message;

  container.appendChild(toast);

  // Inline copy / inline link: inside the message, on its line (see
  // ToastOptions.inlineCopy / .inlineLink).
  reconcileInlineCopy(toast, options.inlineCopy);
  reconcileInlineLink(toast, options.inlineLink);

  // Actions: built as real DOM elements (mixes anchors + the canonical copy
  // button - see buildActionsElement) and inserted right after the message,
  // before the close button. Wiring (anchor click handlers, copy button's own
  // click handler) happens inside buildActionsElement/createCopyButton.
  if (actions?.length) {
    const messageEl = toast.querySelector(".moss-toast-message");
    messageEl?.after(buildActionsElement(actions));
  }

  // Render progress bar if requested
  if (options.progress !== undefined) {
    applyProgressBar(toast, options.progress);
  }

  // Close button handler
  const closeButton = toast.querySelector(".moss-toast-close");
  const removeThisToast = () => {
    // Cancel the LIVE countdown, not the one armed below. A coalescing
    // re-render (updateExistingToast) or an updateToast({duration}) replaces
    // `tracked.timer` with a fresh handle; closing the toast must cancel
    // whichever one is current, or an attention-gated timer outlives the toast
    // with its visibilitychange listener still attached. Falls back to the
    // create-time handle for an untracked (id-less) toast.
    if (id) activeToasts.get(id)?.timer?.cancel();
    else autoRemoveTimer?.cancel();
    toast.classList.add("fade-out");
    setTimeout(() => {
      // A same-id coalesce may have revived this toast (clearing `fade-out`)
      // after removal was scheduled — if so, don't delete it out from under it.
      if (!toast.classList.contains("fade-out")) return;
      if (container.contains(toast)) {
        container.removeChild(toast);
      }
      // Clean up tracking if this toast was tracked by ID
      if (id) {
        activeToasts.delete(id);
      }
    }, 300); // Match fade-out animation duration
  };

  if (closeButton) {
    closeButton.addEventListener("click", removeThisToast);
  }

  // Auto-remove after duration (only if duration is set - undefined means persistent)
  let autoRemoveTimer: DismissTimer | undefined;
  if (duration !== undefined) {
    autoRemoveTimer = armDismissTimer(duration, removeThisToast, options.awaitAttention);
  }

  // Track toast by ID if provided (for update-in-place functionality)
  if (id) {
    // Remove any existing toast with same ID first
    const existing = activeToasts.get(id);
    if (existing) {
      existing.timer?.cancel();
      existing.element.remove();
    }
    activeToasts.set(id, {
      element: toast,
      timer: autoRemoveTimer,
      awaitAttention: options.awaitAttention,
      container
    });
  }
}

/**
 * Show a toast for an Advisory, rendered ephemerally ("a toast
 * is an Advisory the renderer chooses to show ephemerally"). `severity` drives
 * the visual tier (info/warning/error); a recovery `action` renders as a single
 * action button.
 *
 * Coalesces on a repeated `options.id` (see {@link renderToast}). For a pure
 * confirmation ("Copied", "Site published") use {@link showAck} — those are NOT
 * advisories.
 *
 * @param advisory - `{ what, severity, action?, durationMs? }`
 * @param options  - Extra toast options (id, persistent, progress, etc.)
 */
export function showToast(advisory: ToastAdvisory, options: ToastOptions = {}): void {
  const merged: ToastOptions = { ...options };
  // The advisory's own durationMs wins unless the caller overrode duration.
  if (advisory.durationMs !== undefined && merged.duration === undefined && !merged.persistent) {
    merged.duration = advisory.durationMs;
  }
  // Advisories (error/warning/degraded) self-dismiss after ~6s by default —
  // longer than the 3s Acks so a failure has read time. Without this every
  // advisory was sticky: renderToast only arms a timer when a duration is set,
  // and showToast (unlike showAck/showOrUpdateToast) applied no default, so
  // error/warning toasts never went away on their own. `persistent: true` is
  // the explicit opt-out for a toast that must stay.
  if (advisory.durationMs === undefined && merged.duration === undefined && !merged.persistent) {
    merged.duration = 6000;
  }
  // A single recovery affordance becomes the one action button.
  if (advisory.action && !merged.actions) {
    merged.actions = [advisory.action];
  }
  renderToast(advisory.what, severityToVariant(advisory.severity), merged);
}

/**
 * Show a pure-confirmation toast (the design's `Ack`) — "Copied", "Site
 * published", "You're up to date". Always success-styled and ephemeral; it is
 * NOT an advisory (no recovery effort, no severity). Defaults to a 3s
 * auto-dismiss unless the caller overrides it or passes `persistent`.
 *
 * @param what    - The confirmation message.
 * @param options - Extra toast options (id, actions, duration, etc.)
 */
export function showAck(what: string, options: ToastOptions = {}): void {
  renderToast(what, "success", { duration: 3000, ...options });
}

/**
 * Show a coalescing info/progress toast, or update an existing one if `id`
 * matches. Used by the long-running progress surfaces (update download bar,
 * import progress) that are neither an advisory nor a confirmation — they are a
 * live, in-place status line. Defaults to a 3s auto-dismiss; pass `persistent`
 * for a status line that stays until explicitly replaced/removed.
 *
 * @param message - Toast message text
 * @param options - Toast options including required `id` for tracking
 */
export function showOrUpdateToast(
  message: string,
  options: ToastOptions & { id: string }
): void {
  renderToast(message, "info", { duration: 3000, ...options });
}

/**
 * Update an existing toast by ID.
 *
 * Partial patch: only the fields you pass change; everything else (actions,
 * progress bar, dismissible, timer) is left untouched. This is the right tool
 * when you hold an `id` and want to tweak one thing — contrast with the
 * coalescing {@link showToast}, which does a faithful FULL replace. No-op if
 * the id isn't live.
 *
 * @param id - The toast ID to update
 * @param updates - Partial updates to apply
 */
export function updateToast(
  id: string,
  updates: {
    message?: string;
    type?: ToastVariant;
    actions?: ToastAction[];
    duration?: number;
    /** `true` stops the countdown: the toast stays until it is closed. */
    persistent?: boolean;
    dismissible?: boolean;
    /** Omitted ⇒ untouched; `null` ⇒ the bar is removed. */
    progress?: number | null;
    /** See {@link ToastOptions.dot}. Applied together with `type` (or alone,
     * against whatever type the toast already has). */
    dot?: "yellow" | "green";
    /** See {@link ToastOptions.inlineCopy}. Omitted ⇒ untouched (it survives a
     * message-only patch); `""` ⇒ cleared. */
    inlineCopy?: string;
    /** See {@link ToastOptions.inlineLink}. Omitted ⇒ untouched (it survives a
     * message-only patch); `null` ⇒ cleared. */
    inlineLink?: { label: string; url: string } | null;
    /** See {@link ToastOptions.awaitAttention}. Omitted ⇒ the toast keeps
     * whatever gating it already had. Only consulted alongside `duration`. */
    awaitAttention?: boolean;
  }
): void {
  const existing = activeToasts.get(id);
  if (!existing) return;

  const { message, type, actions, duration, persistent, dismissible, progress, dot, inlineCopy, inlineLink, awaitAttention } =
    updates;
  const el = existing.element;

  // Update message if provided; the inline affordances ride along with it.
  if (message !== undefined) {
    setToastMessage(el, message, inlineCopy, inlineLink);
  } else {
    if (inlineCopy !== undefined) reconcileInlineCopy(el, inlineCopy);
    if (inlineLink !== undefined) reconcileInlineLink(el, inlineLink ?? undefined);
  }

  // Update type/class + icon if provided
  if (type !== undefined) {
    el.className = `moss-toast ${type}`;
    setToastIcon(el, type, dot);
  } else if (dot !== undefined) {
    setToastIcon(el, severityToVariant("ShippedDegraded"), dot);
  }

  // Replace actions if provided (empty array clears them)
  if (actions !== undefined) {
    el.querySelector('.moss-toast-actions')?.remove();
    if (actions.length > 0) {
      const messageEl = el.querySelector('.moss-toast-message');
      const span = buildActionsElement(actions);
      if (messageEl) messageEl.after(span);
      else el.appendChild(span);
    }
  }

  // Update progress bar if provided
  if (progress !== undefined) {
    applyProgressBar(el, progress ?? undefined);
  }

  // Update dismissible state
  if (dismissible !== undefined) {
    reconcileCloseButton(el, dismissible, () => removeToastById(id));
  }

  if (persistent) {
    existing.timer?.cancel();
    existing.timer = undefined;
  } else if (duration !== undefined) {
    existing.timer?.cancel();
    // Carry the gating forward. A partial patch promises untouched fields stay
    // untouched, and "does this countdown wait for the user?" is one of them —
    // re-arming plain would let a toast the user is away for expire unseen.
    existing.awaitAttention = awaitAttention ?? existing.awaitAttention;
    existing.timer = armDismissTimer(
      duration,
      () => removeToastById(id),
      existing.awaitAttention,
    );
  }
}

/**
 * Remove a toast by its ID.
 *
 * Use this to programmatically dismiss a tracked toast, e.g., when
 * replacing a progress toast with a final result toast.
 *
 * @param id - The toast ID to remove
 */
export function removeToastById(id: string): void {
  const existing = activeToasts.get(id);
  if (existing) {
    existing.timer?.cancel();
    existing.element.classList.add("fade-out");
    setTimeout(() => {
      // A same-id coalesce may have revived this toast (clearing `fade-out`)
      // after removal was scheduled — if so, leave it (and its tracking) alone.
      if (!existing.element.classList.contains("fade-out")) return;
      if (existing.container.contains(existing.element)) {
        existing.container.removeChild(existing.element);
      }
      activeToasts.delete(id);
    }, 300);
  }
}
