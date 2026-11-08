// ThresholdPass token discipline and decision error paths: the pass adopts
// the newest guard generation, drops stale events and stale resolves, and on
// a failed decision send distinguishes "the question is dead" (screen comes
// down) from a transient failure (the screen re-arms — the question is still
// parked backend-side).

import { describe, test, expect, vi, beforeEach } from 'vitest';

type Handler = (msg: { kind: string; payload?: unknown }) => void;
const handlers = new Map<string, Handler>();
vi.mock("../../../utils/moss-events", () => ({
  onMossEvent: vi.fn(async (kind: string, handler: Handler) => {
    handlers.set(kind, handler);
    return () => handlers.delete(kind);
  }),
}));

import { ThresholdPass, type ThresholdPassDeps } from '../threshold-pass';
import type { ThresholdPrompt } from '../seam';

const prompt = (token: number): ThresholdPrompt => ({
  token,
  kind: 'unowned',
  report: {
    root: '/u/Drive',
    root_class: { kind: 'normal' },
    nested: [
      {
        rel_path: 'a/blog',
        abs_path: '/u/Drive/a/blog',
        name: 'blog',
        published: false,
        site_id: null,
      },
    ],
    truncated: false,
    dirs_visited: 7,
  },
  suggestion: null,
});

const firePrompt = (p: ThresholdPrompt) =>
  handlers.get('ThresholdPrompt')!({ kind: 'ThresholdPrompt', payload: p });
const fireResolved = (token: number, cancelled: boolean) =>
  handlers.get('ThresholdResolved')!({ kind: 'ThresholdResolved', payload: { token, cancelled } });

const flush = () => new Promise((r) => setTimeout(r, 0));

describe('ThresholdPass', () => {
  let overlay: HTMLElement;
  let submit: ReturnType<typeof vi.fn>;
  let onError: ReturnType<typeof vi.fn>;
  let showChecking: ReturnType<typeof vi.fn>;
  let narrate: ReturnType<typeof vi.fn>;
  let pass: ThresholdPass;

  beforeEach(async () => {
    handlers.clear();
    overlay = document.createElement('div');
    document.body.innerHTML = '';
    document.body.appendChild(overlay);
    submit = vi.fn().mockResolvedValue({ status: 'ok', data: null });
    onError = vi.fn();
    showChecking = vi.fn();
    narrate = vi.fn();
    const deps: ThresholdPassDeps = {
      raiseOverlay: () => overlay,
      clearOverlay: () => {
        overlay.classList.remove('visible');
        overlay.replaceChildren();
      },
      getStage: () => 'ready',
      setStage: vi.fn(),
      dropWaitingScreen: vi.fn(),
      showChecking,
      narrate,
      onError,
      submitDecision: submit,
      acknowledgePrompt: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    };
    pass = new ThresholdPass(deps);
    await pass.start();
  });

  const clickPrimary = () => {
    overlay.querySelector<HTMLButtonElement>('[data-threshold-primary]')!.click();
  };

  const fireChecking = (token: number, dirs: number) =>
    handlers.get('ThresholdChecking')!({
      kind: 'ThresholdChecking',
      payload: { token, dirs_visited: dirs },
    });

  // The walk is a wait, so it is drawn by the one surface that owns waiting
  // (screen/surface.ts) — this pass owns only the question.
  test('the walk paints on the shared screen and narrates to the hairline', () => {
    fireChecking(8, 47);

    expect(showChecking).toHaveBeenCalledWith(47);
    expect(narrate).toHaveBeenLastCalledWith(expect.stringContaining('47'));
    // The question has not been asked yet, so nothing of this pass's own is up.
    expect(overlay.querySelector('[data-threshold-cards]')).toBeNull();
  });

  test('the question ends the narration and takes the slot back', () => {
    fireChecking(8, 47);
    firePrompt(prompt(8));

    expect(narrate).toHaveBeenLastCalledWith(null);
    expect(overlay.querySelector('[data-threshold-cards]')).not.toBeNull();
  });

  test('a late ThresholdResolved for an old token does not tear down the current screen', () => {
    firePrompt(prompt(8));
    expect(overlay.querySelector('[data-threshold-cards]')).not.toBeNull();

    // Pass 7's teardown lands after pass 8's question is up: ignored.
    fireResolved(7, true);
    expect(overlay.querySelector('[data-threshold-cards]')).not.toBeNull();
    expect(overlay.classList.contains('visible')).toBe(true);

    // The current token's resolve is the one that ends the pass.
    fireResolved(8, true);
    expect(overlay.innerHTML).toBe('');
  });

  test("an older pass's events cannot displace the newer question", () => {
    firePrompt(prompt(8));
    fireChecking(5, 3);
    // Still the prompt: the older pass never reaches a screen at all.
    expect(overlay.querySelector('[data-threshold-cards]')).not.toBeNull();
    expect(showChecking).not.toHaveBeenCalled();
  });

  test('a decision quotes the adopted token', async () => {
    firePrompt(prompt(8));
    clickPrimary();
    await flush();
    expect(submit).toHaveBeenCalledWith({ kind: 'open_nested', abs_path: '/u/Drive/a/blog' }, 8);
  });

  test("a refusal ('question is dead') closes the screen", async () => {
    submit.mockResolvedValue({
      status: 'error',
      error: 'stale threshold decision: a newer question replaced this one',
    });
    firePrompt(prompt(8));
    clickPrimary();
    await flush();
    expect(overlay.innerHTML).toBe('');
    expect(onError).not.toHaveBeenCalled();
  });

  test('a transient failure re-arms the screen instead of stranding the question', async () => {
    submit.mockResolvedValueOnce({ status: 'error', error: 'ipc timed out' });
    firePrompt(prompt(8));
    clickPrimary();
    await flush();

    // The screen is still up, the failure was surfaced, and the controls
    // work again — the question is still parked backend-side.
    expect(overlay.querySelector('[data-threshold-cards]')).not.toBeNull();
    expect(onError).toHaveBeenCalledWith(expect.stringContaining('ipc timed out'));
    const primary = overlay.querySelector<HTMLButtonElement>('[data-threshold-primary]')!;
    expect(primary.disabled).toBe(false);

    clickPrimary();
    await flush();
    expect(submit).toHaveBeenCalledTimes(2);
    expect(submit).toHaveBeenLastCalledWith(
      { kind: 'open_nested', abs_path: '/u/Drive/a/blog' },
      8,
    );
  });
});
