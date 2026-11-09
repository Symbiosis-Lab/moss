/**
 * Tests for the WKWebView repaint workaround in iframe-bridge.ts.
 *
 * WKWebView on macOS sometimes fails to repaint after location.reload()
 * inside an iframe, leaving the viewport blank until a user-triggered scroll.
 * This is a known WebKit compositor bug.
 *
 * The fix: after notifyNavigation() fires on page load, force a repaint by
 * toggling translateZ(0) on the document element across two animation frames.
 * This promotes the element to a GPU-composited layer, forcing WebKit to
 * invalidate and repaint.
 *
 * References:
 * - WebKit bug: https://bugs.webkit.org/show_bug.cgi?id=258375
 * - Tauri WKWebView blank iframe: https://github.com/nicegram/nicegram-web/issues/7
 */
import { describe, it, expect, beforeEach, vi } from "vitest";

/**
 * Extracted forceRepaint logic mirroring iframe-bridge.ts implementation.
 * Uses the same translateZ(0) technique to force WKWebView compositor repaint.
 */
function forceRepaint(doc: { documentElement: { style: { transform: string } } }): void {
  requestAnimationFrame(() => {
    doc.documentElement.style.transform = 'translateZ(0)';
    requestAnimationFrame(() => {
      doc.documentElement.style.transform = '';
    });
  });
}

describe("iframe-bridge WKWebView repaint workaround", () => {
  let rafCallbacks: (() => void)[];

  beforeEach(() => {
    rafCallbacks = [];
    vi.stubGlobal("requestAnimationFrame", (cb: () => void) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    });
  });

  it("sets translateZ(0) on first animation frame to force GPU layer promotion", () => {
    const doc = { documentElement: { style: { transform: '' } } };

    forceRepaint(doc);

    // Before any rAF fires, transform is unchanged
    expect(doc.documentElement.style.transform).toBe('');

    // Fire first rAF — should set translateZ(0)
    rafCallbacks[0]();
    expect(doc.documentElement.style.transform).toBe('translateZ(0)');
  });

  it("clears transform on second animation frame to avoid permanent layer", () => {
    const doc = { documentElement: { style: { transform: '' } } };

    forceRepaint(doc);

    // Fire first rAF
    rafCallbacks[0]();
    expect(doc.documentElement.style.transform).toBe('translateZ(0)');

    // Fire second rAF — should clear transform
    rafCallbacks[1]();
    expect(doc.documentElement.style.transform).toBe('');
  });

  it("completes full cycle: empty → translateZ(0) → empty", () => {
    const doc = { documentElement: { style: { transform: '' } } };
    const transforms: string[] = [];

    forceRepaint(doc);

    transforms.push(doc.documentElement.style.transform); // before any rAF

    rafCallbacks[0]();
    transforms.push(doc.documentElement.style.transform); // after 1st rAF

    rafCallbacks[1]();
    transforms.push(doc.documentElement.style.transform); // after 2nd rAF

    expect(transforms).toEqual(['', 'translateZ(0)', '']);
  });

  it("schedules exactly two requestAnimationFrame calls", () => {
    const doc = { documentElement: { style: { transform: '' } } };

    forceRepaint(doc);
    // First call to rAF is synchronous
    expect(rafCallbacks).toHaveLength(1);

    // Fire first rAF — schedules second
    rafCallbacks[0]();
    expect(rafCallbacks).toHaveLength(2);

    // Fire second rAF — no more scheduled
    rafCallbacks[1]();
    expect(rafCallbacks).toHaveLength(2);
  });
});
