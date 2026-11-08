/**
 * BaseModal — abstract base class for centered modals.
 *
 * Provides backdrop + container DOM, open/close lifecycle, Escape key dismiss,
 * backdrop click dismiss, focus trapping, and focus restoration.
 *
 * Subclasses implement `renderContent()` to provide the modal's inner DOM.
 */

import { suppressTooltips } from "../../components/tooltip";

export abstract class BaseModal {
  /**
   * The live open instance of each modal class, so a second instance of the
   * SAME class cannot open on top of the first.
   *
   * Why this is here and not at the call sites: every modal builds its own
   * backdrop in its constructor and appends it to `document.body` on `open()`,
   * so two live instances mean two stacked `rgba(0,0,0,0.5)` backdrops
   * compounding to 0.75, then 0.875. A light-dismiss click then closes only
   * the topmost instance and the rest stay, which a user experiences as "the
   * screen keeps getting darker and clicking outside does nothing" — the modal
   * is effectively inescapable. That is exactly what a repeated Publish click
   * produced: `deploy-handler` clears its `isDeploying` re-entrancy flag
   * immediately before constructing the modal, so the flag guards nothing past
   * that point and every click built a fresh instance.
   *
   * Keyed by class rather than global on purpose: stacking two DIFFERENT
   * modals is legitimate (a delete-confirm over settings), while a second
   * instance of the same modal never is.
   */
  private static readonly openInstances = new Map<unknown, BaseModal>();

  /** Source of fresh ids for `wireAriaLabel()` — shared across every modal instance. */
  private static titleIdCounter = 0;

  protected backdrop: HTMLElement;
  protected container: HTMLElement;
  /** Held while open — see the call in `open()`. */
  private releaseTooltips: (() => void) | null = null;

  protected _isOpen = false;
  protected destroyed = false;
  protected previouslyFocusedElement: HTMLElement | null = null;

  protected escapeHandler: ((e: KeyboardEvent) => void) | null = null;
  protected focusTrapHandler: ((e: KeyboardEvent) => void) | null = null;
  protected returnHandler: ((e: KeyboardEvent) => void) | null = null;

  protected static readonly FOCUSABLE_SELECTOR =
    'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';

  /**
   * Return the DOM element to render inside the modal container.
   * Called once each time the modal is opened.
   */
  abstract renderContent(): HTMLElement;

  /**
   * May Escape / a backdrop click dismiss the modal right now? Subclasses
   * override to veto light-dismiss during states that must not be abandoned
   * (e.g. a publish in flight). Explicit affordances calling `close()`
   * directly are unaffected.
   */
  protected dismissable(): boolean {
    return true;
  }

  /**
   * An intentional initial focus target for a modal whose first message is
   * more important than its first control. Returning `null` preserves the
   * component family's normal first-focusable default.
   *
   * This hook is deliberately narrow: it is evaluated after content has been
   * rendered and must return an element inside this modal. That lets a report
   * alert focus its labelled heading without making a new focus policy for
   * every modal.
   */
  protected initialFocus(): HTMLElement | null {
    return null;
  }

  constructor() {
    // Create backdrop — not yet in DOM
    this.backdrop = document.createElement("div");
    this.backdrop.className = "moss-modal-backdrop";

    // Backdrop click closes modal (unless the subclass vetoes light-dismiss)
    this.backdrop.addEventListener("click", () => {
      if (this.dismissable()) this.close();
    });

    // Create container — not yet in DOM
    this.container = document.createElement("div");
    this.container.className = "moss-modal";
    this.container.setAttribute("role", "dialog");
    this.container.setAttribute("aria-modal", "true");

    // Prevent clicks on the container from bubbling to backdrop
    this.container.addEventListener("click", (e) => {
      e.stopPropagation();
    });

    // Set up Escape key handler (attached to document in open())
    this.escapeHandler = (e: KeyboardEvent) => {
      if (e.key === "Escape" && this._isOpen && this.dismissable()) {
        this.close();
      }
    };

    // Set up focus trap handler (attached to container in open())
    this.focusTrapHandler = (e: KeyboardEvent) => {
      this.trapFocus(e);
    };

    // Set up Return-activates-default handler (attached to container in open())
    this.returnHandler = (e: KeyboardEvent) => {
      this.activateDefault(e);
    };
  }

  /**
   * Open the modal: save focus, render content, append to DOM, attach handlers.
   */
  open(): void {
    if (this._isOpen || this.destroyed) return;

    // Another live instance of this same class owns the screen. Hand attention
    // back to it rather than stacking a second backdrop behind it.
    //
    // `isConnected` is the load-bearing half: an instance whose DOM was torn
    // out from under it (a host wiping its container, a test resetting
    // document.body) still reports `_isOpen`, and deferring to that zombie
    // would block the modal from ever opening again. Only a modal actually on
    // screen can claim the slot.
    const live = BaseModal.openInstances.get(this.constructor);
    if (live && live !== this && live._isOpen && live.backdrop.isConnected) {
      live.focusFirstElement();
      return;
    }
    BaseModal.openInstances.set(this.constructor, this);

    // Save focus before opening
    this.previouslyFocusedElement =
      (document.activeElement as HTMLElement) ?? null;

    this._isOpen = true;

    // Render content into container
    const content = this.renderContent();
    this.container.appendChild(content);

    // Give the container an accessible name from whatever `.moss-modal-title`
    // renderContent() just produced, unless the subclass already wired one
    // itself (SettingsModal's own visually-hidden h2).
    this.wireAriaLabel();

    // Append to DOM
    document.body.appendChild(this.backdrop);
    document.body.appendChild(this.container);

    // Show
    this.backdrop.classList.add("visible");
    this.container.classList.add("visible");

    this.suppressOutsideTooltips();

    // Attach handlers
    document.addEventListener("keydown", this.escapeHandler!);
    this.container.addEventListener("keydown", this.focusTrapHandler!);
    this.container.addEventListener("keydown", this.returnHandler!);

    // An opt-in report may first announce its labelled explanation; ordinary
    // modals retain the established first-control default.
    this.focusInitialElement();
  }

  /**
   * Close the modal: remove visible class, detach handlers, remove from DOM,
   * restore focus.
   */
  close(): void {
    if (!this._isOpen) return;

    this._isOpen = false;

    // Release the per-class slot only if this instance still holds it.
    if (BaseModal.openInstances.get(this.constructor) === this) {
      BaseModal.openInstances.delete(this.constructor);
    }

    this.restoreOutsideTooltips();

    // Remove visible class
    this.backdrop.classList.remove("visible");
    this.container.classList.remove("visible");

    // Detach handlers
    if (this.escapeHandler) {
      document.removeEventListener("keydown", this.escapeHandler);
    }
    if (this.focusTrapHandler) {
      this.container.removeEventListener("keydown", this.focusTrapHandler);
    }
    if (this.returnHandler) {
      this.container.removeEventListener("keydown", this.returnHandler);
    }

    // Remove from DOM
    this.backdrop.remove();
    this.container.remove();

    // Clear rendered content so next open() gets fresh content
    this.container.innerHTML = "";

    // Restore focus
    this.previouslyFocusedElement?.focus();
    this.previouslyFocusedElement = null;
  }

  /**
   * Full cleanup: close + set destroyed flag.
   */
  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;

    this.close();

    // Null out handler references
    this.escapeHandler = null;
    this.focusTrapHandler = null;
    this.returnHandler = null;
  }

  /**
   * Whether the modal is currently open.
   */
  isOpen(): boolean {
    return this._isOpen;
  }

  /**
   * Point the container's `aria-labelledby` at whatever `.moss-modal-title`
   * is currently rendered, minting a stable id for it if it doesn't have one.
   *
   * Called once from `open()`, which covers every subclass that builds its
   * content once in `renderContent()`. A subclass that replaces its own
   * title element later via an in-place re-render (not another `open()`)
   * calls this again after that re-render so the label follows it —
   * `ReverifyConfirmModal.rerender()` and
   * `OnionPressPublishModal.transitionTo()` both do. Idempotent either way:
   * an element that already carries an id (DeleteConfirmModal's own
   * per-instance title id, stable across its question → disclosure morph)
   * keeps it rather than being reassigned one.
   *
   * A no-op when there is no `.moss-modal-title` in the container yet — the
   * `wireAriaLabel()` call inside `open()` runs after `renderContent()` has
   * already appended its content, so this only happens for a subclass that
   * genuinely has no title (none today).
   */
  protected wireAriaLabel(): void {
    const title = this.container.querySelector<HTMLElement>(".moss-modal-title");
    if (!title) return;
    if (!title.id) title.id = `moss-modal-title-${BaseModal.titleIdCounter++}`;
    this.container.setAttribute("aria-labelledby", title.id);
  }

  /**
   * Return/Enter activates the modal's default button — the trailing
   * `.moss-btn-primary` or `.moss-btn-danger` in `.moss-modal-actions`, per
   * the footer convention (default trailing, Cancel leading) every modal
   * now follows. Two carve-outs, both about not stealing a keystroke that
   * means something else where the event actually landed:
   *
   *   - A `<textarea>` needs Enter to insert a newline.
   *   - A focused `<button>` already activates itself on Enter/Space; asking
   *     it to activate the DEFAULT button instead would fire the wrong one
   *     whenever a non-default button (Cancel, a secondary action) has
   *     focus, and double-fire when the default itself does.
   *
   * A busy or disabled default is not a valid target — `runAction`'s own
   * convention marks busy with `aria-disabled` rather than the `disabled`
   * property (see run-action.ts), so both are checked.
   */
  protected activateDefault(e: KeyboardEvent): void {
    // A control that already acted on this Enter (a switch toggling) claims it.
    if (e.key !== "Enter" || e.defaultPrevented) return;
    const target = e.target as HTMLElement | null;
    if (!target || target.tagName === "TEXTAREA" || target.tagName === "BUTTON") return;
    const primary = this.container.querySelector<HTMLButtonElement>(
      ".moss-modal-actions .moss-btn-primary, .moss-modal-actions .moss-btn-danger"
    );
    if (!primary) return;
    if (primary.disabled || primary.getAttribute("aria-disabled") === "true") return;
    e.preventDefault();
    primary.click();
  }

  /**
   * Hold off tooltips outside this modal for as long as it is on screen.
   *
   * A modal is the one tier the tooltip OUTRANKS (`--z-popup` 100 against
   * modal content's `--z-modal` + 1), so a tooltip still up when one opens
   * floats OVER the modal instead of being buried by it — the same absence as
   * the menu case, the other direction. Modals arm no outside-dismissal (the
   * backdrop click is their own), so `armOutsideDismiss` cannot speak for
   * them.
   *
   * No toast carve-out here: `suppressed()`'s rank check already spares any
   * trigger whose own ambient tier outranks this modal's, and a toast's
   * (`--z-toast`) always does — see tooltip.ts.
   *
   * Protected rather than inlined in `open()` because a subclass whose
   * elements live in the DOM permanently — `SettingsModal` — cannot call
   * `super.open()` at all: that appends `renderContent()` and re-appends the
   * backdrop on every open. It overrides the lifecycle and calls this pair
   * instead, so "a modal is on screen" still has one owner.
   */
  protected suppressOutsideTooltips(): void {
    this.releaseTooltips = suppressTooltips([this.container]);
  }

  /** Release what `suppressOutsideTooltips` held. Idempotent. */
  protected restoreOutsideTooltips(): void {
    this.releaseTooltips?.();
    this.releaseTooltips = null;
  }

  protected focusFirstElement(): void {
    const first = this.container.querySelector(
      BaseModal.FOCUSABLE_SELECTOR
    ) as HTMLElement | null;
    first?.focus();
  }

  private focusInitialElement(): void {
    const initial = this.initialFocus();
    if (initial && this.container.contains(initial)) {
      initial.focus();
      return;
    }
    this.focusFirstElement();
  }

  /**
   * Trap Tab key focus within the modal container.
   * When Tab is pressed on the last focusable element, wraps to the first.
   * When Shift+Tab is pressed on the first focusable element, wraps to the last.
   */
  private trapFocus(e: KeyboardEvent): void {
    if (e.key !== "Tab") return;

    const focusableElements = Array.from(
      this.container.querySelectorAll(BaseModal.FOCUSABLE_SELECTOR)
    ) as HTMLElement[];

    if (focusableElements.length === 0) return;

    const first = focusableElements[0];
    const last = focusableElements[focusableElements.length - 1];
    const initial = this.initialFocus();

    // An initial target may be programmatically focusable (`tabindex=-1`) and
    // therefore intentionally absent from FOCUSABLE_SELECTOR. Treat it as the
    // edge immediately before the first control so Shift+Tab cannot escape.
    if (initial && this.container.contains(initial) && document.activeElement === initial) {
      e.preventDefault();
      if (e.shiftKey) last.focus();
      else first.focus();
      return;
    }

    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }
}
