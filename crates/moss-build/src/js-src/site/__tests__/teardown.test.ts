import { describe, expect, test, vi } from "vitest";

import { teardownStack } from "../nav/teardown";

describe("teardownStack", () => {
  test("release removes every listener it added, with the options it added them with", () => {
    const stack = teardownStack();
    const target = document.createElement("div");
    const handler = vi.fn();
    stack.on(target, "click", handler, { passive: true });
    target.click();
    expect(handler).toHaveBeenCalledTimes(1);
    stack.release();
    target.click();
    expect(handler).toHaveBeenCalledTimes(1);
  });

  test("release runs added cleanups and listener removals in the order recorded", () => {
    const stack = teardownStack();
    const order: string[] = [];
    const target = document.createElement("div");
    const spy = vi.spyOn(target, "removeEventListener").mockImplementation(() => void order.push("listener"));
    stack.add(() => order.push("first"));
    stack.on(target, "click", () => {});
    stack.add(() => order.push("last"));
    stack.release();
    expect(order).toEqual(["first", "listener", "last"]);
    spy.mockRestore();
  });

  test("releasing twice runs each cleanup once, and the stack is reusable", () => {
    const stack = teardownStack();
    const cleanup = vi.fn();
    stack.add(cleanup);
    stack.release();
    stack.release();
    expect(cleanup).toHaveBeenCalledTimes(1);
    stack.add(cleanup);
    stack.release();
    expect(cleanup).toHaveBeenCalledTimes(2);
  });
});
