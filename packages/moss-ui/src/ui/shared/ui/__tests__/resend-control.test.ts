/**
 * Unit tests for the shared Resend button (ui/resend-control.ts).
 *
 * The behaviours pinned here are the two that are easy to get wrong: a silent
 * outcome, and a button that resizes while it counts.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createResendControl, RESEND_COOLDOWN_MS, type ResendControl } from '../resend-control';

function make(overrides: Partial<Parameters<typeof createResendControl>[0]> = {}) {
  const opts = {
    label: () => 'Resend',
    countdownLabel: (s: number) => `Resend in ${s}s`,
    onResend: vi.fn().mockResolvedValue({ ok: true }),
    onStatus: vi.fn(),
    ...overrides,
  };
  const control = createResendControl(opts);
  document.body.appendChild(control.el);
  return { control, onResend: opts.onResend as ReturnType<typeof vi.fn>, onStatus: opts.onStatus as ReturnType<typeof vi.fn> };
}

let live: ResendControl | null = null;

beforeEach(() => {
  vi.useFakeTimers();
  document.body.innerHTML = '';
});

afterEach(() => {
  live?.destroy();
  live = null;
  vi.useRealTimers();
  document.body.innerHTML = '';
});

describe('createResendControl', () => {
  it('arm() marks the button aria-disabled and shows the full cooldown', () => {
    const { control } = make();
    live = control;
    control.arm();
    // aria-disabled, never `disabled` — see the focus note in resend-control.ts.
    expect(control.el.getAttribute('aria-disabled')).toBe('true');
    expect(control.el.disabled).toBe(false);
    expect(control.el.textContent).toBe(`Resend in ${RESEND_COOLDOWN_MS / 1000}s`);
  });

  it('ticks down every second without letting the button resize', async () => {
    const { control } = make();
    live = control;
    control.arm();
    // The no-reflow guarantee is tabular figures plus a *measured* min-width
    // (see the 'width reservation' suite below) — not a fixed 'Nch' guess.
    expect(control.el.style.fontVariantNumeric).toBe('tabular-nums');

    await vi.advanceTimersByTimeAsync(3000);
    expect(control.el.textContent).toBe('Resend in 27s');
  });

  it('re-enables and reverts the label at zero', async () => {
    const { control } = make();
    live = control;
    control.arm();
    await vi.advanceTimersByTimeAsync(RESEND_COOLDOWN_MS);
    expect(control.el.getAttribute('aria-disabled')).toBeNull();
    expect(control.el.textContent).toBe('Resend');
  });

  it('ignores a second click while a resend is in flight', async () => {
    let resolve!: (v: { ok: true }) => void;
    const pending = new Promise<{ ok: true }>((r) => (resolve = r));
    const { control, onResend } = make({ onResend: vi.fn().mockReturnValue(pending) });
    live = control;

    control.el.click();
    control.el.click();
    expect(onResend).toHaveBeenCalledTimes(1);

    resolve({ ok: true });
    await vi.advanceTimersByTimeAsync(10);
  });

  it('reports success in the live region and re-arms', async () => {
    const { control, onStatus } = make();
    live = control;

    control.el.click();
    await vi.advanceTimersByTimeAsync(10);

    expect(onStatus).toHaveBeenCalledWith({ tone: 'calm', text: expect.stringContaining('Sent again') });
    expect(control.el.getAttribute('aria-disabled')).toBe('true'); // re-armed

    // The success notice reverts so the waiting status is not buried.
    await vi.advanceTimersByTimeAsync(4000);
    expect(onStatus).toHaveBeenLastCalledWith(null);
  });

  it('reports failure with the server message verbatim and re-arms anyway', async () => {
    const { control, onStatus } = make({
      onResend: vi
        .fn()
        .mockResolvedValue({ ok: false, error: 'A verification email was just sent — check your inbox.' }),
    });
    live = control;

    control.el.click();
    await vi.advanceTimersByTimeAsync(10);

    // A 429 is the server saying it is STILL cooling: re-arming is the honest
    // state, and the server's own user-facing prose is not re-worded here.
    expect(onStatus).toHaveBeenCalledWith({
      tone: 'error',
      text: expect.stringContaining('A verification email was just sent'),
    });
    expect(control.el.getAttribute('aria-disabled')).toBe('true');
  });

  it('stays focusable — and focused — after its own click arms the cooldown', async () => {
    const { control } = make();
    live = control;
    control.el.focus();
    expect(document.activeElement).toBe(control.el);

    control.el.click();
    await vi.advanceTimersByTimeAsync(10);

    // The click is what arms the cooldown, so a real `disabled` would unfocus
    // the element the user just pressed (HTML spec: an element that becomes
    // non-focusable while focused is unfocused), dropping activeElement to
    // <body> — outside the modal and outside its focus trap.
    //
    // `el.disabled === false` is the assertion that actually falsifies the old
    // implementation: jsdom does NOT implement the unfocus rule, so the
    // activeElement check below passes either way and only documents the
    // browser behaviour this is protecting.
    expect(control.el.disabled).toBe(false);
    expect(control.el.getAttribute('aria-disabled')).toBe('true');
    expect(document.activeElement).toBe(control.el);
  });

  it('ignores clicks while cooling down', async () => {
    const { control, onResend } = make();
    live = control;
    control.arm();
    control.el.click();
    await vi.advanceTimersByTimeAsync(10);
    expect(onResend).not.toHaveBeenCalled();

    // ...and accepts one the moment the cooldown ends.
    await vi.advanceTimersByTimeAsync(RESEND_COOLDOWN_MS);
    control.el.click();
    await vi.advanceTimersByTimeAsync(10);
    expect(onResend).toHaveBeenCalledTimes(1);
  });

  it('destroy() stops the countdown writing to a dead button', async () => {
    const { control } = make();
    control.arm();
    const label = control.el.textContent;
    control.destroy();
    await vi.advanceTimersByTimeAsync(5000);
    expect(control.el.textContent).toBe(label);
  });
});

// jsdom implements no layout, so getBoundingClientRect() is always 0×0 —
// exactly the "detached node" case the reservation must not crash on. To
// exercise the real measuring path we stub requestAnimationFrame (capture
// and flush manually) and stub the element's own getBoundingClientRect to
// report a width keyed to whatever text was set when it was called.
describe('width reservation (measured, not guessed)', () => {
  let rafCallbacks: Array<() => void>;

  beforeEach(() => {
    rafCallbacks = [];
    vi.stubGlobal('requestAnimationFrame', (cb: () => void) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  function flushRaf(): void {
    const due = rafCallbacks;
    rafCallbacks = [];
    due.forEach((cb) => cb());
  }

  /** Widths keyed by exact textContent; anything else reports 0. */
  function stubWidthsByText(el: HTMLElement, widths: Record<string, number>): void {
    el.getBoundingClientRect = () =>
      ({
        width: widths[el.textContent ?? ''] ?? 0,
        right: 0,
        left: 0,
        top: 0,
        bottom: 0,
        height: 0,
      }) as DOMRect;
  }

  it('reserves the widest of the idle label and the max-cooldown countdown label', () => {
    // Default cooldown is RESEND_COOLDOWN_MS (30s), so the widest countdown
    // form it will ever show is "Resend in 30s".
    const { control } = make();
    live = control;
    stubWidthsByText(control.el, { Resend: 60.2, 'Resend in 30s': 95.4 });

    flushRaf();

    expect(control.el.style.minWidth).toBe('96px'); // ceil(95.4)
    // The idle label is restored after measuring — no visible flash of the
    // countdown text on a button that was never armed.
    expect(control.el.textContent).toBe('Resend');
  });

  it('picks the countdown label when it is the wider of the two', () => {
    const { control } = make();
    live = control;
    stubWidthsByText(control.el, { Resend: 200, 'Resend in 30s': 90 });

    flushRaf();

    expect(control.el.style.minWidth).toBe('200px');
  });

  it('does not reserve a width for a button that is not yet connected to the document', () => {
    const opts = {
      label: () => 'Resend',
      countdownLabel: (s: number) => `Resend in ${s}s`,
      onResend: vi.fn().mockResolvedValue({ ok: true }),
      onStatus: vi.fn(),
    };
    const control = createResendControl(opts);
    live = control;
    // Not appended to document.body — getBoundingClientRect would report 0
    // width in a real browser too (a detached node has no box).
    control.el.getBoundingClientRect = () =>
      ({ width: 999, right: 0, left: 0, top: 0, bottom: 0, height: 0 }) as DOMRect;

    flushRaf();

    expect(control.el.style.minWidth).toBe('');
  });
});
