/**
 * Tests for the overlay-toast-gate: the single queue-while-covered rule
 * shared by Rust-sent toasts and direct toastManager calls.
 */

import { describe, test, expect, beforeEach } from "vitest";
import { setOverlayUp, isOverlayUp, queueOrRun, __test__reset } from "../overlay-toast-gate";

beforeEach(() => {
  __test__reset();
});

describe("overlay-toast-gate: setOverlayUp / isOverlayUp", () => {
  test("starts down", () => {
    expect(isOverlayUp()).toBe(false);
  });

  test("reflects the last call", () => {
    setOverlayUp(true);
    expect(isOverlayUp()).toBe(true);
    setOverlayUp(false);
    expect(isOverlayUp()).toBe(false);
  });
});

describe("overlay-toast-gate: queueOrRun", () => {
  test("runs immediately when the overlay is down", () => {
    let ran = false;
    queueOrRun("k", () => { ran = true; });
    expect(ran).toBe(true);
  });

  test("queues while the overlay is up, then flushes on drop", () => {
    setOverlayUp(true);
    let ran = false;
    queueOrRun("k", () => { ran = true; });
    expect(ran).toBe(false); // not rendered while covered

    setOverlayUp(false);
    expect(ran).toBe(true); // rendered exactly once on dismiss
  });

  test("flushes queued entries in the order they were queued", () => {
    setOverlayUp(true);
    const order: string[] = [];
    queueOrRun("a", () => order.push("a"));
    queueOrRun("b", () => order.push("b"));
    queueOrRun("c", () => order.push("c"));
    setOverlayUp(false);
    expect(order).toEqual(["a", "b", "c"]);
  });

  test("a repeated key while queued collapses to the latest call — not duplicated", () => {
    setOverlayUp(true);
    const calls: string[] = [];
    queueOrRun("dup", () => calls.push("first"));
    queueOrRun("dup", () => calls.push("second"));
    setOverlayUp(false);
    expect(calls).toEqual(["second"]);
  });

  test("distinct keys never collapse into each other", () => {
    setOverlayUp(true);
    const calls: string[] = [];
    queueOrRun("content:one", () => calls.push("one"));
    queueOrRun("content:two", () => calls.push("two"));
    setOverlayUp(false);
    expect(calls).toEqual(["one", "two"]);
  });

  test("a 6th distinct entry evicts the oldest queued one (cap 5)", () => {
    setOverlayUp(true);
    const calls: string[] = [];
    for (let i = 1; i <= 6; i++) {
      queueOrRun(`k${i}`, () => calls.push(`k${i}`));
    }
    setOverlayUp(false);
    // k1 was the oldest and is evicted to make room for k6; the rest survive.
    expect(calls).toEqual(["k2", "k3", "k4", "k5", "k6"]);
  });

  test("the cap counts distinct entries, not calls — a dedupe replace never evicts", () => {
    setOverlayUp(true);
    const calls: string[] = [];
    for (let i = 1; i <= 5; i++) {
      queueOrRun(`k${i}`, () => calls.push(`k${i}`));
    }
    // Re-queuing an EXISTING key replaces it in place — still 5 entries, no eviction.
    queueOrRun("k3", () => calls.push("k3-updated"));
    setOverlayUp(false);
    expect(calls).toEqual(["k1", "k2", "k3-updated", "k4", "k5"]);
  });

  test("setOverlayUp(true) called again while already up does not re-flush or clear the queue", () => {
    setOverlayUp(true);
    let ran = false;
    queueOrRun("k", () => { ran = true; });
    setOverlayUp(true); // no-op edge (already up)
    expect(ran).toBe(false);
    setOverlayUp(false);
    expect(ran).toBe(true);
  });

  test("a toast queued, then the overlay drops and rises again before flush logic re-runs, still flushes only once", () => {
    setOverlayUp(true);
    let calls = 0;
    queueOrRun("k", () => { calls += 1; });
    setOverlayUp(false); // flushes
    setOverlayUp(true);
    setOverlayUp(false); // nothing queued this time — no re-run
    expect(calls).toBe(1);
  });
});
