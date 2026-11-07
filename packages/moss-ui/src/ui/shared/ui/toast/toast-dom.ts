// How a toast's parts are built and reconciled — icon, message, inline copy,
// progress bar, action row, close button.
//
// Every function here takes the toast element and mutates one region of it, so
// the create path and the update-in-place path can share the same reconcilers
// and stay faithful to each other. None of them own lifecycle: no timers, no
// registry, no removal — that is `toast-manager`, which sits above this module.
// Nothing here imports it, which is what keeps the DAG acyclic (the close
// button takes its dismiss action as a callback).
//
// The LOCKED LAYOUT these functions build against is documented in
// `styles/shared/toast.css`. Two rules bind this file: actions belong to
// `.moss-toast-actions` (grid row 2) and nothing else may join the message's
// row; anything that must sit ON the message's line goes INSIDE
// `.moss-toast-message` (rule 5 — that is `reconcileInlineCopy`).

import { t } from '../../../i18n';
import { isHttpUrl } from '../../url-scheme';
import { createCopyButton } from '../copy-button';
import { getToastRuntime } from './toast-runtime';
import type { ToastAction, ToastVariant } from './toast-types';

/** SVG icon markup by type (success/warning only; info/error have none). */
function iconHtml(type: ToastVariant): string {
  return type === "success"
    ? '<span class="moss-toast-icon"><svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M13.5 4.5L6 12L2.5 8.5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg></span>'
    : type === "warning"
    ? '<span class="moss-toast-icon"><svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M7.13 2.52L1.3 12.52c-.37.64.09 1.48.87 1.48h11.66c.78 0 1.24-.84.87-1.48L8.87 2.52c-.37-.64-1.37-.64-1.74 0z" stroke="currentColor" stroke-width="1.5" fill="none"/><path d="M8 6v3M8 11h.01" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg></span>'
    : '';
}

/** Small centered colored-dot icon markup — see `ToastOptions.dot`. */
function dotHtml(dot: "yellow" | "green"): string {
  return `<span class="moss-toast-icon moss-toast-icon--dot moss-toast-icon--dot-${dot}"></span>`;
}

/** The leading icon markup for a toast — `dot` wins over the type glyph. */
export function toastIconHtml(type: ToastVariant, dot?: "yellow" | "green"): string {
  return dot ? dotHtml(dot) : iconHtml(type);
}

/**
 * Reconcile the leading icon span to match `type`/`dot`. Removes any existing
 * icon and inserts a fresh one (as the first child) when there is one. `dot`,
 * when set, takes priority over the type-derived check/triangle icon. Used by
 * both create and in-place update so a type or dot change is faithful.
 */
export function setToastIcon(toast: HTMLElement, type: ToastVariant, dot?: "yellow" | "green"): void {
  toast.querySelector(".moss-toast-icon")?.remove();
  const html = toastIconHtml(type, dot);
  if (html) toast.insertAdjacentHTML("afterbegin", html);
  toast.classList.toggle("moss-toast--compact", dot === "green");
}

/**
 * Reconcile the inline copy affordance inside `.moss-toast-message` — see
 * `ToastOptions.inlineCopy`.
 *
 * Icon-only: the id it copies is already spelled out in the message, so a
 * "Copy" label would say it twice. `data-tooltip` (never `title=`) carries the
 * hover name; both surfaces that show these toasts — the shell and the
 * launcher — call `initTooltips()`.
 *
 * The live value is mirrored onto `data-inline-copy` so a later message-only
 * patch can restore the button after `textContent =` wipes the message's
 * children — `updateToast` promises untouched fields stay untouched.
 */
export function reconcileInlineCopy(toast: HTMLElement, value: string | undefined): void {
  const messageEl = toast.querySelector<HTMLElement>(".moss-toast-message");
  if (!messageEl) return;
  messageEl.querySelector(".moss-copy-btn")?.remove();
  if (!value) {
    delete toast.dataset.inlineCopy;
    return;
  }
  toast.dataset.inlineCopy = value;
  messageEl.appendChild(createCopyButton({ value, label: "", ariaLabel: t("publish.copy") }));
}

/**
 * Reconcile the inline link inside `.moss-toast-message` — see
 * {@link ToastOptions.inlineLink}. Same slot and same reasoning as
 * {@link reconcileInlineCopy}: it belongs to the sentence, so it goes on the
 * sentence's line rather than in the row-2 action grid.
 *
 * Wired through the same {@link wireActionHandlers} path as a real action, so
 * onion routing and the openUrl fallback behave identically. The live value is
 * mirrored onto the dataset so a later message-only patch can restore it after
 * `textContent =` wipes the message's children.
 */
export function reconcileInlineLink(
  toast: HTMLElement,
  link: { label: string; url: string } | undefined,
): void {
  const messageEl = toast.querySelector<HTMLElement>(".moss-toast-message");
  if (!messageEl) return;
  messageEl.querySelector(".moss-toast-inline-link")?.remove();
  if (!link) {
    delete toast.dataset.inlineLinkLabel;
    delete toast.dataset.inlineLinkUrl;
    return;
  }
  toast.dataset.inlineLinkLabel = link.label;
  toast.dataset.inlineLinkUrl = link.url;
  const el = document.createElement("a");
  el.className = "moss-toast-action moss-toast-inline-link";
  el.href = isHttpUrl(link.url) ? link.url : '#';
  el.dataset.url = link.url;
  el.dataset.actionIndex = "0";
  el.textContent = link.label;
  messageEl.appendChild(el);
  wireActionHandlers(messageEl, [{ label: link.label, url: link.url }]);
}

/** The inline link mirrored onto the toast's dataset, if it has one. */
function liveInlineLink(toast: HTMLElement): { label: string; url: string } | undefined {
  const { inlineLinkLabel, inlineLinkUrl } = toast.dataset;
  return inlineLinkLabel && inlineLinkUrl
    ? { label: inlineLinkLabel, url: inlineLinkUrl }
    : undefined;
}

/**
 * Write the message text, then restore the inline affordances that
 * `textContent =` just removed. `inlineCopy`/`inlineLink` omitted ⇒ carry the
 * live value forward; `""` / `null` ⇒ clear it.
 */
export function setToastMessage(
  toast: HTMLElement,
  text: string,
  inlineCopy?: string,
  inlineLink?: { label: string; url: string } | null,
): void {
  const messageEl = toast.querySelector(".moss-toast-message");
  if (!messageEl) return;
  messageEl.textContent = text;
  reconcileInlineCopy(toast, inlineCopy ?? toast.dataset.inlineCopy);
  reconcileInlineLink(toast, inlineLink === null ? undefined : inlineLink ?? liveInlineLink(toast));
}

/**
 * Create or update the progress bar in `toast`, in place.
 *
 * - `undefined` → remove the bar (a fresh toast with no `progress` has none).
 * - `< 0`       → indeterminate (animated stripe, full width).
 * - `0`–`100`   → determinate fill at that percent (clamped).
 *
 * The `.moss-progress-bar-fill` element is reused across calls so its CSS
 * `transition: width` animates between updates instead of resetting — this
 * is what keeps a slow download from flickering.
 */
export function applyProgressBar(toast: HTMLElement, progress: number | undefined): void {
  if (progress === undefined) {
    toast.querySelector(".moss-toast-progress")?.remove();
    return;
  }

  let progressContainer = toast.querySelector(".moss-toast-progress") as HTMLElement | null;
  if (!progressContainer) {
    progressContainer = document.createElement("div");
    progressContainer.className = "moss-toast-progress";
    const track = document.createElement("div");
    track.className = "moss-progress-bar-track";
    const fill = document.createElement("div");
    fill.className = "moss-progress-bar-fill";
    track.appendChild(fill);
    progressContainer.appendChild(track);
    toast.appendChild(progressContainer);
  }

  const fill = progressContainer.querySelector(".moss-progress-bar-fill") as HTMLElement | null;
  if (!fill) return;
  if (progress < 0) {
    fill.classList.add("indeterminate");
    fill.style.width = "100%";
  } else {
    fill.classList.remove("indeterminate");
    fill.style.width = `${Math.min(100, Math.max(0, Math.round(progress)))}%`;
  }
}

/**
 * Build one action's DOM element. A `copyValue` action renders as the
 * canonical copy button (see {@link ToastAction.copyValue}); everything else
 * renders as the existing `.moss-toast-action` anchor, wired by
 * {@link wireActionHandlers}.
 */
function buildActionElement(action: ToastAction, index: number): HTMLElement {
  if (action.copyValue !== undefined) {
    return createCopyButton({ value: action.copyValue, label: action.label });
  }
  // A command (has `onClick`) renders as a real <button> so keyboard Space
  // activates it and a screen reader announces "button"; a navigation (url
  // only) stays an <a>. wireActionHandlers runs onClick before url, so onClick
  // IS the command signal. The base `.moss-toast-action` resets <button> UA
  // chrome, so a command looks identical to a navigation link.
  const isCommand = action.onClick !== undefined;
  const el = isCommand
    ? document.createElement('button')
    : document.createElement('a');
  el.className = 'moss-toast-action';
  if (action.variant) el.classList.add(`moss-toast-action--${action.variant}`);
  el.dataset.actionIndex = String(index);
  if (isCommand) {
    (el as HTMLButtonElement).type = 'button'; // never submit a form
  } else {
    // The click handler preventDefaults, so this href is never followed by a
    // click — but a middle-click or the context menu's "Open link" reaches the
    // OS handler without passing openUrl, and a toast action's URL comes from
    // the plugin that raised the toast.
    (el as HTMLAnchorElement).href = action.url && isHttpUrl(action.url) ? action.url : '#';
    el.dataset.url = action.url || '';
  }
  // A primary action is a command (a filled button), not a "go to X"
  // navigation, so it drops the trailing "→" — same reasoning the copyValue
  // affordance already uses. Everything else keeps the arrow.
  el.textContent = action.variant === 'primary' ? action.label : `${action.label} →`;
  return el;
}

/** Wire click handlers for `.moss-toast-action` controls (a <button> command or
 * an <a> navigation): onClick callback wins, else open URL. */
function wireActionHandlers(root: ParentNode, actions: ToastAction[]): void {
  root.querySelectorAll<HTMLElement>(".moss-toast-action").forEach((link) => {
    link.addEventListener("click", async (e) => {
      e.preventDefault();
      e.stopPropagation();
      const idx = parseInt(link.dataset.actionIndex ?? "-1");
      const action = actions[idx];
      if (action?.onClick) {
        action.onClick();
        return;
      }
      const url = link.dataset.url;
      if (!url) return;
      // Resolved per click, never captured: an entry point may install the
      // host runtime after this handler was wired.
      const runtime = getToastRuntime();
      // A host-installed route (e.g. the desktop routes `.onion` hosts through
      // its Tor-aware backend opener, the same rule as the publish modal's
      // "View on onion" button) claims the
      // URL before the generic opener. On failure just log — the route's own
      // opener already holds the fallback chain.
      const route = runtime.urlRoutes?.find((r) => r.matches(url));
      if (route) {
        const res = await route.open(url);
        if (res.status === "error") runtime.logError(`Failed to open ${route.name} URL: ${res.error}`);
        return;
      }
      try {
        await runtime.openUrl(url);
      } catch (err) {
        runtime.logError(`Failed to open URL: ${err instanceof Error ? err.message : String(err)}`);
        window.open(url, "_blank");
      }
    });
  });
}

/**
 * Build a `.moss-toast-actions` span holding one element per action (a copy
 * button, an anchor, or a mix), with anchor click handlers wired.
 */
export function buildActionsElement(actions: ToastAction[]): HTMLElement {
  const span = document.createElement("span");
  span.className = "moss-toast-actions";
  actions.forEach((a, i) => span.appendChild(buildActionElement(a, i)));
  wireActionHandlers(span, actions);
  return span;
}

/**
 * Add or remove the trailing close (✕) button to match `dismissible`.
 *
 * `onClose` rather than a direct call into the registry: this module knows how
 * to build a close button, not what dismissing means. That is also what keeps
 * `toast-manager` → `toast-dom` a one-way edge.
 */
export function reconcileCloseButton(
  toast: HTMLElement,
  dismissible: boolean,
  onClose: () => void
): void {
  const existing = toast.querySelector(".moss-toast-close");
  if (dismissible && !existing) {
    const btn = document.createElement("button");
    btn.className = "moss-toast-close";
    btn.setAttribute("aria-label", t('editmisc.close_notification'));
    btn.textContent = "✕";
    btn.addEventListener("click", onClose);
    toast.appendChild(btn);
  } else if (!dismissible && existing) {
    existing.remove();
  }
}
