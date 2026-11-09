// Proves the JS wiring: a moss-page-color message from the preview iframe sets
// <html data-chrome-theme> on the shell. The render gate (chrome-backdrop.spec)
// proves the CSS cascade once the attribute is set; this proves it gets set.

import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";

// Heavy deps the NavigationManager constructor/handlers touch.
vi.mock("../iframe-rpc", () => ({
  IframeRPC: class { call = vi.fn(); destroy = vi.fn(); },
}));
vi.mock('../../engine', () => ({
  engine: {
    resolvePageSource: vi.fn().mockResolvedValue({ status: "ok", data: {} }),
    listPages: vi.fn().mockResolvedValue({ status: "ok", data: [] }),
  },
}));
vi.mock("../../context/event-bus", () => ({
  eventBus: { emit: vi.fn(), on: vi.fn(), off: vi.fn() },
  Events: { PREVIEW_URL_CHANGED: "preview-url-changed" },
}));

import { NavigationManager } from "../navigation-manager";
import { host } from "../../host";

// No native swipe stream: the indicator falls back to the bridge's wheel heuristic.
host.swipePhases = async () => false;

/** Post as the preview iframe would: with its contentWindow as the source.
 *  The handler source-checks, so tests must send from the real sender. */
function pageColor(iframe: HTMLIFrameElement, color: string | null) {
  window.dispatchEvent(
    new MessageEvent("message", {
      data: { type: "moss-page-color", color },
      source: iframe.contentWindow,
    }),
  );
}

describe("NavigationManager: moss-page-color -> data-chrome-theme", () => {
  // Same leak guard as site-accent-wiring.test.ts: an undisposed manager's
  // theme observer survives the test and re-applies stored state as a
  // microtask after a later test's cleanup ran.
  const live: NavigationManager[] = [];

  beforeEach(() => {
    document.documentElement.setAttribute("data-theme", "dark");
    document.documentElement.removeAttribute("data-chrome-theme");
  });

  afterEach(() => {
    for (const m of live.splice(0)) m.destroy();
    document.querySelectorAll("iframe").forEach((f) => f.remove());
  });

  function mgr() {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    live.push(new NavigationManager(iframe));
    return iframe;
  }

  test("a light backdrop sets data-chrome-theme=light (the bug fix: OS dark + light site)", () => {
    const iframe = mgr();
    pageColor(iframe, "#F4EDE3");
    expect(document.documentElement.getAttribute("data-chrome-theme")).toBe("light");
  });

  test("a dark backdrop sets data-chrome-theme=dark", () => {
    const iframe = mgr();
    pageColor(iframe, "#1a1816");
    expect(document.documentElement.getAttribute("data-chrome-theme")).toBe("dark");
  });

  test("a null/invalid backdrop mirrors the intended (data-theme) theme", () => {
    const iframe = mgr();
    pageColor(iframe, null);
    // data-theme is "dark" -> chrome mirrors dark when no backdrop is known.
    expect(document.documentElement.getAttribute("data-chrome-theme")).toBe("dark");
  });

  test("constructor mirrors data-theme up front (no stale cold-start chrome)", () => {
    document.documentElement.setAttribute("data-theme", "light");
    mgr();
    expect(document.documentElement.getAttribute("data-chrome-theme")).toBe("light");
  });

  test("flipping the shell theme drops the stale backdrop (the moon-toggle wedge)", async () => {
    const iframe = mgr();
    pageColor(iframe, "#1a1816");
    expect(document.documentElement.getAttribute("data-chrome-theme")).toBe("dark");

    // The site's own moon toggle reaches the shell (moon SSOT), so data-theme
    // flips — but the only backdrop the shell holds was measured on the dark
    // page. Until the preview re-reports, the chrome must not go on asserting
    // dark over a page the user just asked to be light.
    document.documentElement.setAttribute("data-theme", "light");
    await Promise.resolve();
    expect(document.documentElement.getAttribute("data-chrome-theme")).toBe("light");
  });

  test("a backdrop reported after the flip is authoritative again", async () => {
    const iframe = mgr();
    pageColor(iframe, "#1a1816");
    document.documentElement.setAttribute("data-theme", "light");
    await Promise.resolve();

    // Dropping the stale color is not a permanent demotion: real evidence
    // still outranks the intended theme, which is the whole point of the
    // chrome tracking the page rather than the OS.
    pageColor(iframe, "#1a1816");
    expect(document.documentElement.getAttribute("data-chrome-theme")).toBe("dark");
  });

  test("a page-color message from a non-iframe source is ignored", () => {
    // A nested embed can post to window.top regardless of shell-mount gating
    // on the sender — the same reason the ambient and swipe handlers check
    // event.source. Without the check, any embedded page can restyle the
    // shell's chrome (and, downstream, everything --moss-page-color feeds).
    mgr();
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { type: "moss-page-color", color: "#F4EDE3" },
        // no source: not the preview iframe
      }),
    );
    // data-theme is "dark"; the spoofed light backdrop must not flip the chrome.
    expect(document.documentElement.getAttribute("data-chrome-theme")).toBe("dark");
  });
});
