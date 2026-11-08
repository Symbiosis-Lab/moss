/**
 * runAction — the three obligations every action owes the person who started it, pinned.
 *
 * Each `describe` below is one obligation. If one of these goes red, the class
 * of bug it was written against (a dead-looking button, a silent failure, a
 * control stuck busy forever) is back in the product, not merely in the test.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

const { showToastMock, showAckMock, uiEventMock } = vi.hoisted(() => ({
  showToastMock: vi.fn(),
  showAckMock: vi.fn(),
  uiEventMock: vi.fn(),
}));
vi.mock('../toast/toast-manager', async () => {
  const actual = await vi.importActual<typeof import('../toast/toast-manager')>('../toast/toast-manager');
  return { ...actual, showToast: showToastMock, showAck: showAckMock };
});

import { runAction, bindAction, BUSY_CLASS, BUSY_LABEL_ATTR } from '../run-action';
// run-action.ts is `shared/ui` and so does not import the log directly
// (shared/ is pure); its [ui] breadcrumb goes through the same injected
// host seam toasts use (toast-runtime.ts).
import { configureToastRuntime, _resetToastRuntime } from '../toast/toast-runtime';

/** A promise plus the handles to settle it from the test body. */
function deferred<T>(): {
  promise: Promise<T>;
  resolve: (v: T) => void;
  reject: (e: unknown) => void;
} {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Let every queued microtask (and the helper's own chain) settle. */
const flush = (): Promise<void> => new Promise((r) => setTimeout(r, 0));

let btn: HTMLButtonElement;

beforeEach(() => {
  document.body.innerHTML = '';
  btn = document.createElement('button');
  btn.textContent = 'Send test';
  document.body.appendChild(btn);
  showToastMock.mockClear();
  showAckMock.mockClear();
  uiEventMock.mockClear();
  configureToastRuntime({ logUiEvent: (line: string) => uiEventMock(line) });
});

afterEach(() => {
  vi.restoreAllMocks();
  _resetToastRuntime();
});

// ---------------------------------------------------------------------------
// Obligation 2 — paint before the awaited work, in the click's own frame.
// ---------------------------------------------------------------------------

describe('paints before the work resolves', () => {
  it('has already marked the trigger busy by the time `run` is invoked', () => {
    let busyAtRunTime: boolean | undefined;
    void runAction({
      trigger: btn,
      run: async () => {
        // This body runs synchronously up to its first await — i.e. as early as
        // any I/O could possibly start. The paint must already be on screen.
        busyAtRunTime = btn.classList.contains(BUSY_CLASS);
        return null;
      },
    });
    expect(busyAtRunTime).toBe(true);
  });

  it('paints synchronously — before control returns to the caller', () => {
    const gate = deferred<null>();
    void runAction({ trigger: btn, run: () => gate.promise, busyLabel: 'Working…' });

    // No await between the click and this assertion: the same frame.
    expect(btn.getAttribute('aria-busy')).toBe('true');
    expect(btn.getAttribute('aria-disabled')).toBe('true');
    expect(btn.classList.contains(BUSY_CLASS)).toBe(true);
    expect(btn.textContent).toBe('Working…');

    gate.resolve(null);
  });

  it('runs a custom in-progress surface synchronously too', () => {
    const busy = vi.fn();
    const gate = deferred<null>();
    void runAction({ trigger: btn, busy, run: () => gate.promise });
    expect(busy).toHaveBeenCalledOnce();
    gate.resolve(null);
  });

  it('swaps the [data-action-label] span, not a trigger that has children', async () => {
    const card = document.createElement('div');
    card.innerHTML = `<span class="icon">✎</span><span ${BUSY_LABEL_ATTR}>Start writing</span>`;
    document.body.appendChild(card);

    const gate = deferred<null>();
    void runAction({ trigger: card, busyLabel: 'Creating…', run: () => gate.promise });

    expect(card.querySelector('.icon')).not.toBeNull(); // children survive
    expect(card.querySelector(`[${BUSY_LABEL_ATTR}]`)?.textContent).toBe('Creating…');

    gate.resolve(null);
    await flush();
    expect(card.querySelector(`[${BUSY_LABEL_ATTR}]`)?.textContent).toBe('Start writing');
  });
});

// ---------------------------------------------------------------------------
// Obligation 3a — a terminal MESSAGE on every failing path.
// ---------------------------------------------------------------------------

describe('always tells the user when the action fails', () => {
  it('reports a REJECTED promise', async () => {
    await runAction({ trigger: btn, run: async () => Promise.reject(new Error('network down')) });
    expect(showToastMock).toHaveBeenCalledOnce();
    expect(showToastMock.mock.calls[0][0].what).toContain('network down');
    expect(showToastMock.mock.calls[0][0].severity).toBe('NeedsAction');
  });

  it('reports an ERROR-SHAPED RESULT identically — same toast, same severity', async () => {
    await runAction({
      trigger: btn,
      run: async () => ({ status: 'error' as const, error: 'network down' }),
    });
    expect(showToastMock).toHaveBeenCalledOnce();
    expect(showToastMock.mock.calls[0][0].what).toContain('network down');
    expect(showToastMock.mock.calls[0][0].severity).toBe('NeedsAction');
  });

  it('uses the caller\'s wording when given', async () => {
    await runAction({
      trigger: btn,
      run: async () => ({ status: 'error' as const, error: 'no route' }),
      failMessage: (e) => `Couldn't send the test email: ${e}`,
    });
    expect(showToastMock.mock.calls[0][0].what).toBe("Couldn't send the test email: no route");
  });

  it('reports even when the surface went stale — a toast outlives the widget', async () => {
    await runAction({
      trigger: btn,
      stale: () => true,
      run: async () => Promise.reject(new Error('gone')),
    });
    expect(showToastMock).toHaveBeenCalledOnce();
  });

  it('is silent on success unless okMessage is given, and unwraps the ok result', async () => {
    const onOk = vi.fn();
    const value = await runAction({
      trigger: btn,
      run: async () => ({ status: 'ok' as const, data: { id: 7 } }),
      onOk,
    });
    expect(value).toEqual({ id: 7 });
    expect(onOk).toHaveBeenCalledWith({ id: 7 });
    expect(showAckMock).not.toHaveBeenCalled();
    expect(showToastMock).not.toHaveBeenCalled();

    await runAction({ trigger: btn, run: async () => null, okMessage: 'Test email sent' });
    expect(showAckMock).toHaveBeenCalledWith('Test email sent');
  });
});

// ---------------------------------------------------------------------------
// Obligation 3b — a terminal UI STATE on every path, so a retry works.
// ---------------------------------------------------------------------------

describe('gives the surface back on every path', () => {
  it('restores after success', async () => {
    const restore = vi.fn();
    await runAction({ trigger: btn, busyLabel: 'Working…', restore, run: async () => null });
    expect(btn.getAttribute('aria-busy')).toBeNull();
    expect(btn.getAttribute('aria-disabled')).toBeNull();
    expect(btn.classList.contains(BUSY_CLASS)).toBe(false);
    expect(btn.textContent).toBe('Send test');
    expect(restore).toHaveBeenCalledOnce();
  });

  it('restores after a rejection', async () => {
    const restore = vi.fn();
    await runAction({
      trigger: btn,
      busyLabel: 'Working…',
      restore,
      run: async () => Promise.reject(new Error('x')),
    });
    expect(btn.classList.contains(BUSY_CLASS)).toBe(false);
    expect(btn.textContent).toBe('Send test');
    expect(restore).toHaveBeenCalledOnce();
  });

  it('restores after an error-shaped result', async () => {
    await runAction({
      trigger: btn,
      busyLabel: 'Working…',
      run: async () => ({ status: 'error' as const, error: 'x' }),
    });
    expect(btn.classList.contains(BUSY_CLASS)).toBe(false);
    expect(btn.textContent).toBe('Send test');
  });

  it('restores even when the success handler itself throws', async () => {
    await runAction({
      trigger: btn,
      busyLabel: 'Working…',
      run: async () => null,
      onOk: () => {
        throw new Error('bad success handler');
      },
    });
    expect(btn.classList.contains(BUSY_CLASS)).toBe(false);
    expect(showToastMock).toHaveBeenCalledOnce(); // and it is reported
  });

  it('releases the guard on every path, so a retry actually runs', async () => {
    const run = vi.fn().mockRejectedValueOnce(new Error('flaky')).mockResolvedValueOnce(null);
    await runAction({ trigger: btn, run });
    await runAction({ trigger: btn, run });
    expect(run).toHaveBeenCalledTimes(2);
  });

  it('releases a string key even when the surface is stale', async () => {
    const run = vi.fn().mockResolvedValue(null);
    await runAction({ key: 'sync', stale: () => true, run });
    await runAction({ key: 'sync', run });
    expect(run).toHaveBeenCalledTimes(2);
  });
});

// ---------------------------------------------------------------------------
// Obligation 1 — the guard is claimed first, so a second click cannot re-enter.
// ---------------------------------------------------------------------------

describe('re-entrancy', () => {
  it('does not run the work twice when clicked again while busy', async () => {
    const gate = deferred<null>();
    const run = vi.fn(() => gate.promise);

    void runAction({ trigger: btn, run });
    const second = await runAction({ trigger: btn, run });

    expect(run).toHaveBeenCalledOnce();
    expect(second).toBeUndefined();

    gate.resolve(null);
    await flush();
  });

  it('re-asserts the busy paint on a repeat click instead of returning blind', async () => {
    const gate = deferred<null>();
    const onRepeat = vi.fn();
    void runAction({ trigger: btn, busyLabel: 'Working…', run: () => gate.promise, onRepeat });

    // Something else stomped the affordance (a re-render, say).
    btn.classList.remove(BUSY_CLASS);
    btn.removeAttribute('aria-busy');

    await runAction({ trigger: btn, busyLabel: 'Working…', run: () => gate.promise, onRepeat });
    expect(btn.classList.contains(BUSY_CLASS)).toBe(true);
    expect(btn.getAttribute('aria-busy')).toBe('true');
    expect(onRepeat).toHaveBeenCalledOnce();

    gate.resolve(null);
    await flush();
  });

  it('guards by `key` when there is no element trigger', async () => {
    const gate = deferred<null>();
    const run = vi.fn(() => gate.promise);
    void runAction({ key: 'comment-sync', run });
    await runAction({ key: 'comment-sync', run });
    expect(run).toHaveBeenCalledOnce();
    gate.resolve(null);
    await flush();
  });

  it('keeps guards independent across different triggers', async () => {
    const other = document.createElement('button');
    const gate = deferred<null>();
    const run = vi.fn(() => gate.promise);
    void runAction({ trigger: btn, run });
    void runAction({ trigger: other, run });
    expect(run).toHaveBeenCalledTimes(2);
    gate.resolve(null);
    await flush();
  });
});

// ---------------------------------------------------------------------------
// bindAction — the one-line adoption path.
// ---------------------------------------------------------------------------

describe('bindAction', () => {
  it('paints on click and carries the same guarantees', async () => {
    const gate = deferred<null>();
    bindAction(btn, { busyLabel: 'Working…', run: () => gate.promise });

    btn.click();
    expect(btn.classList.contains(BUSY_CLASS)).toBe(true);
    expect(btn.textContent).toBe('Working…');

    gate.reject(new Error('boom'));
    await flush();
    expect(btn.textContent).toBe('Send test');
    expect(showToastMock).toHaveBeenCalledOnce();
  });

  it('double click runs the work once', async () => {
    const gate = deferred<null>();
    const run = vi.fn(() => gate.promise);
    bindAction(btn, { run });
    btn.click();
    btn.click();
    expect(run).toHaveBeenCalledOnce();
    gate.resolve(null);
    await flush();
  });

  it('the disposer removes the listener', () => {
    const run = vi.fn().mockResolvedValue(null);
    const dispose = bindAction(btn, { run });
    dispose();
    btn.click();
    expect(run).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// Send Logs UI trail — every accepted click and every surfaced
// failure on this single choke point leaves a breadcrumb, so a bundle can
// tell "clicked" from "failed" from "never reached the button at all".
// ---------------------------------------------------------------------------

describe('control.clicked / command.failed breadcrumbs', () => {
  it('logs control.clicked once the guard is claimed, naming the trigger', async () => {
    btn.id = 'send-test';
    await runAction({ trigger: btn, run: async () => null });

    expect(uiEventMock).toHaveBeenCalledWith('[ui] control.clicked trigger=#send-test');
  });

  it('does not log control.clicked on a repeat click while busy', () => {
    const gate = deferred<null>();
    void runAction({ trigger: btn, run: () => gate.promise });
    uiEventMock.mockClear();

    void runAction({ trigger: btn, run: () => gate.promise });

    expect(uiEventMock).not.toHaveBeenCalledWith(expect.stringContaining('control.clicked'));
  });

  it('logs command.failed with the trigger and the error detail on a rejection', async () => {
    btn.classList.add('danger');
    await runAction({ trigger: btn, run: async () => { throw new Error('disk is full'); } });

    expect(uiEventMock).toHaveBeenCalledWith('[ui] command.failed trigger=button.danger error=disk is full');
  });

  it('logs command.failed on an error-shaped Result, not just a thrown rejection', async () => {
    await runAction({ trigger: btn, run: async () => ({ status: 'error', error: 'nope' }) });

    expect(uiEventMock).toHaveBeenCalledWith(expect.stringContaining('error=nope'));
  });
});
