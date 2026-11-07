/**
 * moss-alert.ts — confirmations and failure messages drawn in the webview,
 * in place of native dialogs.
 *
 * `window.confirm`/`alert` are IPC calls in a webview, and the Tauri dialog
 * plugin that stands in for them cannot be styled, does not open at all on
 * the browser/HTTP carrier (where the fallback would answer "no" to a
 * question the user never saw), and stacks outside moss's own z-index scale.
 * A native OS dialog is a last resort, not the default. Every confirmation
 * and every failure message moss shows the user is a moss alert built on
 * BaseModal/OutcomeModal instead — the same promise-returning shape as a
 * native dialog call, so call sites stay small.
 *
 * Destructive styling follows the same HIG-Alerts adaptation as every other
 * confirm dialog in this contract: red marks a consequence the user didn't
 * choose or one with no undo, not one they clicked their way into. Callers
 * pass `destructive: true` when the action has neither an OS Trash nor a
 * moss "undo" behind it (see each call site for its own reasoning).
 */

import { OutcomeModal } from "./outcome-modal";
import { t } from "../../i18n";

export interface MossConfirmOptions {
  /**
   * Required — HIG Alerts: "Avoid using OK as the default button title
   * unless the alert is purely informational... A specific button title
   * like 'Erase,' 'Convert,' 'Clear,' or 'Delete' helps people understand
   * the action they're taking." A confirm dialog is never purely
   * informational, so there is no generic default to fall back to.
   */
  confirmLabel: string;
  /** Defaults to a generic "Cancel". */
  cancelLabel?: string;
  /** See the file header. */
  destructive?: boolean;
}

/**
 * One shared shape for both the two-button confirm and the one-button
 * informational alert `mossAlert()` needs — Apple's guidance for the latter
 * is the SAME dialog with `cancelLabel` omitted: "if you must display an
 * alert with a single button that's also the default, use a Done button,
 * not a Cancel button" (HIG Alerts).
 */
class AlertModal extends OutcomeModal {
  constructor(
    private readonly message: string,
    private readonly confirmLabel: string,
    private readonly cancelLabel: string | null,
    private readonly destructive: boolean,
    onDone: (result: boolean) => void,
  ) {
    super(onDone);
    this.container.classList.add("moss-modal--confirm");
  }

  renderContent(): HTMLElement {
    const root = document.createElement("div");
    root.className = "moss-alert-modal moss-stack";

    // Today's callers pass one sentence, not a title+body pair — this slot
    // carries it as the title (no separate body), the faithful port of what
    // the native dialogs showed rather than a copy rewrite.
    const title = document.createElement("h2");
    title.className = "moss-modal-title";
    title.textContent = this.message;
    root.appendChild(title);

    const actions = document.createElement("div");
    actions.className = "moss-modal-actions";

    if (this.cancelLabel !== null) {
      const cancel = document.createElement("button");
      cancel.type = "button";
      cancel.className = "moss-btn moss-btn-secondary";
      cancel.setAttribute("data-action", "cancel");
      cancel.textContent = this.cancelLabel;
      cancel.addEventListener("click", () => this.finish(false));
      actions.appendChild(cancel);
    }

    const confirm = document.createElement("button");
    confirm.type = "button";
    confirm.className = this.destructive ? "moss-btn moss-btn-danger" : "moss-btn moss-btn-primary";
    confirm.setAttribute("data-action", "confirm");
    confirm.textContent = this.confirmLabel;
    confirm.addEventListener("click", () => this.finish(true));
    actions.appendChild(confirm);

    root.appendChild(actions);
    return root;
  }
}

/**
 * Ask the user to confirm, returning their answer — the moss-alert
 * replacement for a native confirm dialog.
 *
 * Never throws. `BaseModal`'s per-class singleton guard refuses a second
 * live instance of the SAME modal class rather than stacking one on top of
 * the other; if that happens here (two confirms requested back to back
 * before the first is answered), this resolves `false` rather than leaving
 * the caller's `await` hanging forever — fail to no, as when no dialog can
 * be shown at all.
 */
export function mossConfirm(message: string, opts: MossConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    const modal = new AlertModal(
      message,
      opts.confirmLabel,
      opts.cancelLabel ?? t("dialog.cancel"),
      opts.destructive ?? false,
      resolve,
    );
    modal.open();
    if (!modal.isOpen()) resolve(false);
  });
}

/**
 * Tell the user something went wrong — the moss-alert replacement for a
 * native message dialog. One Done button, never styled as
 * destructive (nothing here is a choice).
 */
export function mossAlert(message: string, opts: { okLabel?: string } = {}): Promise<void> {
  return new Promise((resolve) => {
    const modal = new AlertModal(message, opts.okLabel ?? t("dialog.ok"), null, false, () => resolve());
    modal.open();
    if (!modal.isOpen()) resolve();
  });
}
