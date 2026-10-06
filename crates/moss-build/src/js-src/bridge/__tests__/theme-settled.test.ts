import { describe, it, expect, vi, afterEach } from "vitest";
import { onThemeSettled, THEME_SETTLE_MS } from "../theme-settled";

afterEach(() => {
  document.documentElement.removeAttribute("data-theme");
  vi.useRealTimers();
});

describe("onThemeSettled", () => {
  it("fires on a data-theme flip, now and again after the transition settles", async () => {
    vi.useFakeTimers();
    const cb = vi.fn();
    const settled = onThemeSettled(window, cb);

    document.documentElement.setAttribute("data-theme", "dark");
    await Promise.resolve();
    expect(cb).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(THEME_SETTLE_MS + 40);
    expect(cb).toHaveBeenCalledTimes(2);
    settled.stop();
  });

  it("stop() detaches the observer and cancels a pending settle", async () => {
    vi.useFakeTimers();
    const cb = vi.fn();
    const settled = onThemeSettled(window, cb);

    settled.fire();
    settled.stop();
    document.documentElement.setAttribute("data-theme", "dark");
    await Promise.resolve();
    vi.advanceTimersByTime(1000);
    expect(cb).toHaveBeenCalledTimes(1);
  });
});
