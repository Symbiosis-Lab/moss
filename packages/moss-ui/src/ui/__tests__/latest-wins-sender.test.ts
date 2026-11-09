/**
 * Unit tests for the latest-wins sender — the coalescing layer between
 * divider drag/morph producers and the `set_divider_position` IPC channel.
 *
 * Contract:
 *  1. Idle → request() sends SYNCHRONOUSLY in the same tick (existing tests
 *     pin a synchronous invoke on the non-animated open path).
 *  2. At most ONE send in flight, ever (ordering is trivially FIFO).
 *  3. Latest pending wins; intermediates are dropped.
 *  4. A pending value equal to the last sent value is skipped.
 *  5. request() promises resolve at the next drain point; they never reject.
 *  6. A rejecting send must not wedge the sender.
 *  7. cancelPending() drops only the stored pending value.
 *  8. After a drain the sender is idle again (next request sends synchronously).
 */

import { describe, test, expect, vi } from 'vitest';

import { createLatestWinsSender } from '../latest-wins-sender';

interface Deferred {
  promise: Promise<void>;
  resolve: () => void;
  reject: (err: unknown) => void;
}

function deferred(): Deferred {
  let resolve!: () => void;
  let reject!: (err: unknown) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Flush the microtask queue (macrotask hop drains all chained microtasks). */
async function flush(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

/** A send mock whose every call returns its own manually-settled deferred. */
function gatedSend() {
  const calls: number[] = [];
  const gates: Deferred[] = [];
  let active = 0;
  let maxActive = 0;
  const send = vi.fn((value: number) => {
    calls.push(value);
    active++;
    maxActive = Math.max(maxActive, active);
    const gate = deferred();
    gates.push(gate);
    return gate.promise.finally(() => {
      active--;
    });
  });
  return {
    send,
    calls,
    gates,
    maxConcurrent: () => maxActive,
  };
}

describe('createLatestWinsSender', () => {
  test('idle request() sends synchronously in the same tick', () => {
    const g = gatedSend();
    const sender = createLatestWinsSender<number>(g.send);
    void sender.request(5);
    expect(g.send).toHaveBeenCalledTimes(1);
    expect(g.send).toHaveBeenCalledWith(5);
  });

  test('never more than one send in flight under a burst', async () => {
    const g = gatedSend();
    const sender = createLatestWinsSender<number>(g.send);
    void sender.request(1);
    void sender.request(2);
    void sender.request(3);
    expect(g.send).toHaveBeenCalledTimes(1);
    g.gates[0].resolve();
    await flush();
    expect(g.send).toHaveBeenCalledTimes(2);
    g.gates[1].resolve();
    await flush();
    expect(g.maxConcurrent()).toBe(1);
  });

  test('latest wins: intermediates dropped, exactly [first, last] sent', async () => {
    const g = gatedSend();
    const sender = createLatestWinsSender<number>(g.send);
    void sender.request(1);
    void sender.request(2);
    void sender.request(3);
    void sender.request(4);
    g.gates[0].resolve();
    await flush();
    g.gates[1]?.resolve();
    await flush();
    expect(g.calls).toEqual([1, 4]);
  });

  test('a pending value equal to the last sent value is skipped', async () => {
    const g = gatedSend();
    const sender = createLatestWinsSender<number>(g.send);
    void sender.request(7);
    void sender.request(7);
    g.gates[0].resolve();
    await flush();
    expect(g.calls).toEqual([7]);
  });

  test('object payloads dedup by VALUE via the isEqual option (not identity)', async () => {
    // The divider channel carries {position, commit} objects. Default !==
    // is identity for objects, so two equal-valued literals would re-send;
    // the isEqual comparator restores contract #4 for structured payloads.
    const calls: Array<{ position: number; commit: boolean }> = [];
    const gates: Deferred[] = [];
    const send = (value: { position: number; commit: boolean }) => {
      calls.push(value);
      const gate = deferred();
      gates.push(gate);
      return gate.promise;
    };
    const sender = createLatestWinsSender(send, {
      isEqual: (a, b) => a.position === b.position && a.commit === b.commit,
    });
    void sender.request({ position: 500, commit: false });
    void sender.request({ position: 500, commit: false }); // equal value, distinct object
    gates[0].resolve();
    await flush();
    expect(calls).toEqual([{ position: 500, commit: false }]);

    // A pending payload differing ONLY in commit must still dispatch.
    void sender.request({ position: 500, commit: false });
    void sender.request({ position: 500, commit: true });
    gates[1].resolve();
    await flush();
    expect(calls.length).toBe(3);
    expect(calls[2]).toEqual({ position: 500, commit: true });
  });

  test('drain promise resolves only after the superseding send settles', async () => {
    const g = gatedSend();
    const sender = createLatestWinsSender<number>(g.send);
    void sender.request(1);
    const p = sender.request(2);
    let drained = false;
    void p.then(() => {
      drained = true;
    });
    g.gates[0].resolve();
    await flush();
    // send(2) dispatched but not yet settled — p must still be pending.
    expect(g.calls).toEqual([1, 2]);
    expect(drained).toBe(false);
    g.gates[1].resolve();
    await flush();
    expect(drained).toBe(true);
  });

  test('a rejecting send does not wedge the sender and request() never rejects', async () => {
    const g = gatedSend();
    const sender = createLatestWinsSender<number>(g.send);
    let resolved1 = false;
    const p1 = sender.request(1);
    void p1.then(() => {
      resolved1 = true;
    });
    void sender.request(2);
    g.gates[0].reject(new Error('ipc boom'));
    await flush();
    // The pending 2 is still dispatched despite the rejection.
    expect(g.calls).toEqual([1, 2]);
    g.gates[1].resolve();
    await flush();
    expect(resolved1).toBe(true);
  });

  test('cancelPending drops the stored value; promises still resolve', async () => {
    const g = gatedSend();
    const sender = createLatestWinsSender<number>(g.send);
    let r1 = false;
    let r2 = false;
    void sender.request(1).then(() => {
      r1 = true;
    });
    void sender.request(2).then(() => {
      r2 = true;
    });
    sender.cancelPending();
    g.gates[0].resolve();
    await flush();
    expect(g.calls).toEqual([1]);
    expect(r1).toBe(true);
    expect(r2).toBe(true);
  });

  test('returns to idle after a drain: next request sends synchronously again', async () => {
    const g = gatedSend();
    const sender = createLatestWinsSender<number>(g.send);
    void sender.request(1);
    g.gates[0].resolve();
    await flush();
    void sender.request(9);
    expect(g.calls).toEqual([1, 9]);
  });
});
