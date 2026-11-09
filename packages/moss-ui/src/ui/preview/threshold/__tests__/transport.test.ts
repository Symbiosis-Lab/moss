// The threshold transport's round trip: the guard's events reach the screen
// callbacks (prompt receipt is acked first), and the screen's one decision —
// quoting the pass token — reaches the backend command.

import { describe, test, expect, vi, beforeEach } from 'vitest';

// Capture the per-kind handlers the transport registers, so the test can
// play the guard's side of the wire.
type Handler = (msg: { kind: string; payload?: unknown }) => void;
const handlers = new Map<string, Handler>();
vi.mock("../../../utils/moss-events", () => ({
  onMossEvent: vi.fn(async (kind: string, handler: Handler) => {
    handlers.set(kind, handler);
    return () => handlers.delete(kind);
  }),
}));

import {
  createThresholdTransport,
  isThresholdRefusal,
  type ThresholdPrompt,
  type ThresholdUi,
} from '../seam';

const prompt: ThresholdPrompt = {
  token: 7,
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
};

describe('createThresholdTransport', () => {
  let ui: { [K in keyof ThresholdUi]: ReturnType<typeof vi.fn> };
  let submit: ReturnType<typeof vi.fn>;
  let acknowledge: ReturnType<typeof vi.fn>;

  const make = () => createThresholdTransport(ui, submit, acknowledge);

  beforeEach(() => {
    handlers.clear();
    ui = { showChecking: vi.fn(), showPrompt: vi.fn(), close: vi.fn() };
    submit = vi.fn().mockResolvedValue({ status: 'ok', data: null });
    acknowledge = vi.fn().mockResolvedValue({ status: 'ok', data: null });
  });

  test("the guard's events drive the screen callbacks, tokens attached", async () => {
    const transport = make();
    await transport.start();

    handlers.get('ThresholdChecking')!({
      kind: 'ThresholdChecking',
      payload: { token: 7, dirs_visited: 42 },
    });
    expect(ui.showChecking).toHaveBeenCalledWith(42, 7);

    handlers.get('ThresholdPrompt')!({ kind: 'ThresholdPrompt', payload: prompt });
    expect(ui.showPrompt).toHaveBeenCalledWith(prompt);

    handlers.get('ThresholdResolved')!({
      kind: 'ThresholdResolved',
      payload: { token: 7, cancelled: true },
    });
    expect(ui.close).toHaveBeenCalledWith(true, 7);
    handlers.get('ThresholdResolved')!({
      kind: 'ThresholdResolved',
      payload: { token: 8, cancelled: false },
    });
    expect(ui.close).toHaveBeenLastCalledWith(false, 8);
  });

  test('a prompt is acked with its token before the screen shows it', async () => {
    const order: string[] = [];
    acknowledge.mockImplementation(async (token: number) => {
      order.push(`ack:${token}`);
      return { status: 'ok', data: null };
    });
    ui.showPrompt.mockImplementation(() => order.push('show'));
    const transport = make();
    await transport.start();

    handlers.get('ThresholdPrompt')!({ kind: 'ThresholdPrompt', payload: prompt });
    expect(order).toEqual(['ack:7', 'show']);
  });

  test('a failed ack still shows the prompt (the guard falls back on its own clock)', async () => {
    acknowledge.mockRejectedValue(new Error('gone'));
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const transport = make();
    await transport.start();

    handlers.get('ThresholdPrompt')!({ kind: 'ThresholdPrompt', payload: prompt });
    expect(ui.showPrompt).toHaveBeenCalledWith(prompt);
    await vi.waitFor(() => expect(warn).toHaveBeenCalled());
    warn.mockRestore();
  });

  test('a decision goes through the injected command port, quoting the token', async () => {
    const transport = make();

    await transport.decide({ kind: 'open_nested', abs_path: '/u/Drive/a/blog' }, 7);
    expect(submit).toHaveBeenCalledWith(
      { kind: 'open_nested', abs_path: '/u/Drive/a/blog' },
      7,
    );
  });

  test('a refused decision surfaces as a thrown error, not a silent ok', async () => {
    submit.mockResolvedValue({
      status: 'error',
      error: 'no threshold question is awaiting a decision',
    });
    const transport = make();

    await expect(transport.decide({ kind: 'cancel' }, 7)).rejects.toThrow(
      'no threshold question is awaiting a decision',
    );
  });

  test('the combined unlisten detaches every subscription', async () => {
    const transport = make();
    const unlisten = await transport.start();
    expect(handlers.size).toBe(3);
    unlisten();
    expect(handlers.size).toBe(0);
  });
});

describe('isThresholdRefusal', () => {
  test("recognizes the backend's two dead-question texts and nothing else", () => {
    expect(isThresholdRefusal('no threshold question is awaiting a decision')).toBe(true);
    expect(
      isThresholdRefusal('stale threshold decision: a newer question replaced this one'),
    ).toBe(true);
    expect(isThresholdRefusal('connection reset')).toBe(false);
    expect(isThresholdRefusal('')).toBe(false);
  });
});
