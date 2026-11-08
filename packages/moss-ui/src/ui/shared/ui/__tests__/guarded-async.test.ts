/**
 * runGuarded — the shared "an async UI action always gives the surface back"
 * primitive. These are the properties its callers rely on; break one and the
 * dead-end class it was written to end comes straight back.
 */

import { describe, it, expect, vi } from 'vitest';
import { runGuarded, errorMessage } from '../guarded-async';

describe('errorMessage', () => {
  it('reads an Error message, passes a string through, and never returns [object Object]', () => {
    expect(errorMessage(new Error('boom'))).toBe('boom');
    expect(errorMessage('plain')).toBe('plain');
    expect(errorMessage({ code: 7 })).toBe('{"code":7}');
    expect(errorMessage(undefined)).toBe('undefined');
  });
});

describe('runGuarded', () => {
  it('restores after a resolve, and only AFTER onOk has written its state', async () => {
    const order: string[] = [];
    const value = await runGuarded({
      action: async () => 'v',
      onOk: () => order.push('onOk'),
      onError: () => order.push('onError'),
      restore: () => order.push('restore'),
    });

    expect(value).toBe('v');
    // Order matters: the success handler writes state, THEN the surface redraws.
    expect(order).toEqual(['onOk', 'restore']);
  });

  it('restores after a REJECT and reports the message — the whole point', async () => {
    const restore = vi.fn();
    const onError = vi.fn();

    const value = await runGuarded({
      action: async () => {
        throw new Error('invoke rethrew');
      },
      onError,
      restore,
    });

    expect(value).toBeUndefined();
    expect(onError).toHaveBeenCalledWith('invoke rethrew', expect.any(Error));
    expect(restore).toHaveBeenCalledOnce();
  });

  it('restores even when onOk itself throws', async () => {
    const restore = vi.fn();
    await runGuarded({
      action: async () => 'v',
      onOk: () => {
        throw new Error('bad success handler');
      },
      onError: () => {},
      restore,
    });
    expect(restore).toHaveBeenCalledOnce();
  });

  it('does nothing at all when the surface went stale', async () => {
    const onOk = vi.fn();
    const onError = vi.fn();
    const restore = vi.fn();

    await runGuarded({ action: async () => 'v', onOk, onError, restore, stale: () => true });
    await runGuarded({
      action: async () => {
        throw new Error('x');
      },
      onError,
      restore,
      stale: () => true,
    });

    expect(onOk).not.toHaveBeenCalled();
    expect(onError).not.toHaveBeenCalled();
    expect(restore).not.toHaveBeenCalled();
  });

  it('never rejects, so `void runGuarded(...)` cannot raise an unhandled rejection', async () => {
    await expect(
      runGuarded({
        action: async () => {
          throw new Error('x');
        },
        onError: () => {},
      }),
    ).resolves.toBeUndefined();
  });
});
