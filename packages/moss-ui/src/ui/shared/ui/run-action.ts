/**
 * runAction — a click changes the screen in the same frame it is received.
 *
 * THE RULE: when a person
 * clicks something, something must change before anything else happens. Not
 * "once the network answers" — immediately. And when the work behind the click
 * fails, they must be told, on screen, in words. A click that produces no pixel
 * change is indistinguishable from a broken button, so the user clicks again,
 * and again, and concludes moss is broken. That belief is expensive and we earn
 * it: a user on 0.9.0 reported the updater as dead while a 38 MB download was
 * running the whole time, because the busy paint sat behind an `await`.
 *
 * The rule is three obligations, and this helper's whole reason to exist is that
 * remembering three obligations at twenty call sites is not a plan:
 *
 *   1. Claim the re-entrancy guard FIRST — before any I/O, so the second click
 *      cannot start a second copy of the work.
 *   2. Paint the busy state BEFORE the first `await` — synchronously, in the
 *      click's own frame.
 *   3. Reach a terminal state on EVERY path — success, error-shaped result,
 *      thrown exception, early return — and a terminal *message* on every
 *      failing one. `console.error` is not a terminal message.
 *
 * Here all three are structural. The caller does not paint, so the caller cannot
 * paint late; the helper owns the try/catch/finally, so no path can skip the
 * restore; and a failure always produces a toast, so no failure can be silent.
 * The caller supplies only the work and, optionally, the words.
 *
 * ## Relationship to {@link runGuarded}
 *
 * `runGuarded` (guarded-async.ts) is the DOM-free half of this: it guarantees
 * obligation 3 for callers that already own their own painting and messaging.
 * `runAction` is built on it and adds obligations 1 and 2 plus the default
 * affordance and the default toast. New click handlers should reach for
 * `runAction`; `runGuarded` remains right for non-click async regions (an
 * event-driven refresh, a poll) where there is no trigger and no guard to claim.
 *
 * ## Decisions a caller could get wrong
 *
 * **Busy is `aria-disabled` + `aria-busy` + a class, NEVER the `disabled`
 * property.** Per the HTML spec an element that becomes non-focusable while
 * focused is unfocused, and activeElement falls to `<body>`. Inside a modal that
 * drops focus out of the focus trap for the entire round trip. `aria-disabled`
 * doesn't block the click — the guard does — and it works identically on a
 * `<button>`, a card, a tile, a menu item or an icon, which is the point: one
 * affordance for every trigger shape. `.moss-is-busy` in
 * `styles/shared/primitives.css` supplies the visible half (dimmed + wait
 * cursor), so the busy state is visible even when there is no label to swap.
 *
 * **Nothing here makes the trigger unclickable, and that is deliberate.** The
 * guard is the only thing refusing the second invocation. `.moss-is-busy` must
 * never also set `pointer-events: none`: that takes the cursor down with it —
 * an element that receives no pointer events shows no cursor — and makes
 * `onRepeat` unreachable for element triggers, because the second click never
 * arrives. A surface that genuinely must swallow clicks while busy says so in
 * its own stylesheet; it is not the shared default.
 *
 * **Failure feedback is mandatory; success feedback is opt-in.** Most actions
 * are self-evidencing — the panel opens, the file appears, the row updates — and
 * a toast per click would be noise. Nothing is self-evidencing about a failure,
 * so `okMessage` is optional and the failure toast is not suppressible. If an
 * action must fail quietly, catch it inside `run` and return normally; making
 * that explicit is deliberate.
 *
 * **A repeat click while busy is not a no-op to the person pressing it.** It
 * re-asserts the busy paint rather than returning silently (the update
 * install does the same in Rust: a repeat press re-shows its progress toast at
 * the percentage last painted). Pass `onRepeat` when the surface needs more than
 * re-asserting the trigger's own busy state.
 *
 * **`stale()` means "the surface is gone", not "the user cancelled".** When it
 * returns true the success handler and the restore are skipped — there is
 * nothing left to restore — but a FAILURE IS STILL REPORTED, because a toast
 * outlives the surface and obligation 3 is about the user, not the widget.
 */

import { t } from '../../i18n';
import { uiEvent } from './toast/toast-runtime';
import { runGuarded, errorMessage } from './guarded-async';
import { showAck, showToast } from './toast/toast-manager';

/** Class carrying the visible half of the busy state (primitives.css). */
export const BUSY_CLASS = 'moss-is-busy';

/**
 * Marks the descendant whose text `busyLabel` replaces. Put it on the text span
 * of a trigger that also contains an icon; without it a trigger with element
 * children keeps its label (see {@link ActionSpec.busyLabel}).
 */
export const BUSY_LABEL_ATTR = 'data-action-label';

/**
 * The value a caller actually cares about, with the generated `Result` envelope
 * peeled off. `Promise<Result<Foo, string>>` yields `Foo`; anything else is
 * itself.
 */
export type Unwrapped<T> = T extends { status: 'ok'; data: infer D }
  ? D
  : T extends { status: 'error'; error: unknown }
    ? never
    : T;

export interface ActionSpec<T> {
  /**
   * The async work — usually one `commands.*` call. Invoked only AFTER the busy
   * state has been painted. Both of its failure shapes are handled identically:
   * a rejected promise and a resolved `{ status: 'error', error }` are the same
   * event to the user, and the generated bindings produce either one depending
   * on whether the Rust error was a string or an `Error`.
   */
  run(): Promise<T>;

  /**
   * The clicked element. Supplies the re-entrancy identity and receives the
   * default busy affordance. Omit only when the trigger is not an element, and
   * then pass {@link key} — with neither, re-entrancy CANNOT be detected and a
   * double click runs the work twice.
   */
  trigger?: Element | null;

  /** Re-entrancy identity when there is no element (a keyboard shortcut, a
   *  native menu item). Two calls sharing a key cannot overlap. */
  key?: string;

  /**
   * Replaces the trigger's label while busy, e.g. `t('status.working')`. The
   * text swapped is: the `[data-action-label]` descendant if there is one, else
   * the trigger's own text if it has no element children, else nothing — a
   * blind `textContent =` on a card or an icon+text button would delete its
   * children. Optional: `.moss-is-busy` already makes the busy state visible.
   */
  busyLabel?: string;

  /**
   * A custom in-progress surface — an inline status line, a progress toast, a
   * skeleton row. Called synchronously, in the click's frame, IN ADDITION to
   * the trigger affordance (so the trigger still goes busy). Pair it with
   * {@link restore}.
   */
  busy?(): void;

  /** Undoes {@link busy}. Runs in the helper's `finally`, on every path. */
  restore?(): void;

  /** Confirmation toast on success. Omit for a self-evidencing action. */
  okMessage?: string;

  /** Runs with the unwrapped success value, before the restore. */
  onOk?(value: Unwrapped<T>): void;

  /**
   * Wording for the failure toast, given the underlying detail. Default:
   * `t('action.failed', { error })`. Supply a specific sentence whenever the
   * surface knows what was being attempted — "Couldn't send the test email"
   * beats the generic line.
   */
  failMessage?(detail: string): string;

  /** Extra effect when a click arrives while this action is already running.
   *  The trigger's busy paint is re-asserted regardless. */
  onRepeat?(): void;

  /** True when the surface has been destroyed/closed/superseded. See the module
   *  docblock: it suppresses the restore and the ack, never the failure. */
  stale?(): boolean;
}

/** A stable-ish name for a failed action's trigger, for the `command.failed`
 *  breadcrumb — never its text content (could be a name, an email, a URL):
 *  its `id`, else `tag.class`, else undefined. */
function triggerName(trigger?: Element | null): string | undefined {
  if (!trigger) return undefined;
  if (trigger.id) return `#${trigger.id}`;
  const cls = trigger.classList.item(0);
  return cls ? `${trigger.tagName.toLowerCase()}.${cls}` : trigger.tagName.toLowerCase();
}

/** Elements currently mid-action. Weak so a removed trigger is collectable. */
const busyElements = new WeakSet<object>();
/** String-keyed actions currently running. Bounded by concurrent actions. */
const busyKeys = new Set<string>();

function claim(spec: { trigger?: Element | null; key?: string }): (() => void) | null {
  const { trigger, key } = spec;
  if (key !== undefined) {
    if (busyKeys.has(key)) return null;
    busyKeys.add(key);
  }
  if (trigger) {
    if (busyElements.has(trigger)) {
      if (key !== undefined) busyKeys.delete(key);
      return null;
    }
    busyElements.add(trigger);
  }
  return () => {
    if (key !== undefined) busyKeys.delete(key);
    if (trigger) busyElements.delete(trigger);
  };
}

/** The element whose text `busyLabel` replaces, or null when there is none. */
function labelTarget(trigger: Element): Element | null {
  const marked = trigger.querySelector(`[${BUSY_LABEL_ATTR}]`);
  if (marked) return marked;
  return trigger.childElementCount === 0 ? trigger : null;
}

/** Paint the busy affordance. Returns the function that takes it back. */
function paintBusy(trigger: Element | null | undefined, busyLabel?: string): () => void {
  if (!trigger) return () => {};

  trigger.setAttribute('aria-disabled', 'true');
  trigger.setAttribute('aria-busy', 'true');
  trigger.classList.add(BUSY_CLASS);

  let restoreLabel = (): void => {};
  if (busyLabel !== undefined) {
    const target = labelTarget(trigger);
    if (target) {
      const previous = target.textContent ?? '';
      target.textContent = busyLabel;
      restoreLabel = () => {
        target.textContent = previous;
      };
    }
  }

  return () => {
    trigger.removeAttribute('aria-disabled');
    trigger.removeAttribute('aria-busy');
    trigger.classList.remove(BUSY_CLASS);
    restoreLabel();
  };
}

/** True for a resolved value carrying the generated `Result` error arm. */
function resultError(value: unknown): string | null {
  if (typeof value !== 'object' || value === null) return null;
  const v = value as { status?: unknown; error?: unknown };
  if (v.status !== 'error') return null;
  return typeof v.error === 'string' ? v.error : errorMessage(v.error);
}

/** Peel a resolved `{ status: 'ok', data }` envelope; pass anything else on. */
function unwrapOk<T>(value: T): Unwrapped<T> {
  if (typeof value === 'object' && value !== null) {
    const v = value as { status?: unknown; data?: unknown };
    if (v.status === 'ok' && 'data' in v) return v.data as Unwrapped<T>;
  }
  return value as Unwrapped<T>;
}

/**
 * Run a user-triggered async action under the three obligations.
 *
 * Resolves with the unwrapped success value, or `undefined` when the action was
 * refused (already running), failed, or the surface went stale — so
 * `if (!value) return;` is the caller's early-out.
 *
 * ```ts
 * btn.addEventListener('click', () => {
 *   void runAction({
 *     trigger: btn,
 *     busyLabel: t('status.working'),
 *     run: () => commands.emailSendTest(args),
 *     okMessage: t('email.test_sent'),
 *     failMessage: (e) => t('email.test_failed', { error: e }),
 *   });
 * });
 * ```
 */
export async function runAction<T>(spec: ActionSpec<T>): Promise<Unwrapped<T> | undefined> {
  // Obligation 1: the guard is claimed before anything else can yield.
  const release = claim(spec);
  if (!release) {
    // A repeat click still has to register. Re-assert rather than return blind.
    paintBusy(spec.trigger, spec.busyLabel);
    spec.onRepeat?.();
    return undefined;
  }
  // One breadcrumb per accepted click on a named control — the counterpart
  // to `command.failed` below, so a Send Logs bundle can tell "the user
  // clicked X" from "X failed" from "X never got clicked at all".
  uiEvent('control.clicked', { key: spec.key, trigger: triggerName(spec.trigger) });

  // Obligation 2: painted synchronously — every statement above and below this
  // point runs in the click's own frame, before `run()` is ever invoked.
  const unpaint = paintBusy(spec.trigger, spec.busyLabel);
  spec.busy?.();

  const fail = (detail: string): void => {
    // One choke point for every runAction-driven command failure surfaced to
    // the user. `detail` is
    // `errorMessage(err)` — whatever message a rejected promise or thrown
    // Error carried, not guaranteed to be a Rust-side string only; a caller's
    // `run()` can embed arbitrary text (e.g. a recipient address in an email
    // send-test failure). Truncated, not logged verbatim.
    uiEvent('command.failed', {
      key: spec.key,
      trigger: triggerName(spec.trigger),
      error: detail.length > 120 ? `${detail.slice(0, 120)}…` : detail,
    });
    showToast({
      what: spec.failMessage ? spec.failMessage(detail) : t('action.failed', { error: detail }),
      severity: 'NeedsAction',
    });
  };

  // Obligation 3: runGuarded owns the try/catch/finally, so `restore` runs on
  // resolve, reject, and a throw out of the success handler alike.
  return runGuarded<Unwrapped<T> | undefined>({
    action: async () => {
      const value = await spec.run();
      const error = resultError(value);
      // An error-shaped RESULT is a failure, identical to a rejection. Throwing
      // it here is what makes the two paths literally the same code below.
      if (error !== null) throw new Error(error);
      return unwrapOk(value);
    },
    onOk: (value) => {
      if (spec.stale?.() === true) return;
      if (spec.okMessage) showAck(spec.okMessage);
      spec.onOk?.(value as Unwrapped<T>);
    },
    // NOTE: `stale` is checked inside `onOk`/`restore` rather than handed to
    // runGuarded, which would suppress this callback too. Staleness means the
    // widget is gone, not that the user stopped caring — a toast outlives the
    // widget, and this call is the only thing standing between them and a
    // silent failure.
    onError: (message) => fail(message),
    restore: () => {
      if (spec.stale?.() === true) return;
      unpaint();
      spec.restore?.();
    },
  }).finally(() => {
    // The guard is released last and unconditionally — including when the
    // surface is stale, which skips `restore` but must never wedge the key.
    release();
  });
}

/**
 * Attach a click handler that runs `spec` under {@link runAction}, with
 * `trigger` filled in. The one-line adoption path for a bare
 * `addEventListener('click', () => void commands.foo())`.
 *
 * Returns a disposer that removes the listener.
 */
export function bindAction<T>(
  trigger: Element,
  spec: Omit<ActionSpec<T>, 'trigger'>,
): () => void {
  const onClick = (): void => {
    void runAction({ ...spec, trigger });
  };
  trigger.addEventListener('click', onClick);
  return () => trigger.removeEventListener('click', onClick);
}
