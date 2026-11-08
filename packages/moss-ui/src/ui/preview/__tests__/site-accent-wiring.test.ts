// Proves the JS wiring: a moss-site-accent message from the preview iframe
// lands as --moss-site-veil on the shell root, re-derives on app theme flips,
// and clears on reset() (the folder-switch boundary). The derivation itself
// is covered in site-tint.test.ts; the CSS consumers in site-tint-css.test.ts
// and the visual pass.

import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";

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

// A stand-in "customized" accent — deliberately NOT moss's own default, so
// these wiring tests aren't short-circuited by the default-accent guard.
const CUSTOM_ACCENT = "rgb(80, 40, 120)";

function siteAccent(source: Window | null, color: string | null) {
  window.dispatchEvent(
    new MessageEvent("message", {
      data: { type: "moss-site-accent", color },
      source,
    }),
  );
}

const root = () => document.documentElement;

describe("NavigationManager: moss-site-accent -> identity tint", () => {
  // Undisposed managers keep their theme observers alive, and beforeEach's
  // data-theme write would make a PREVIOUS test's manager re-apply its stored
  // accent as a microtask after the property cleanup ran — the leak reads as
  // a failed source check. Destroy every manager at test end.
  const live: NavigationManager[] = [];

  beforeEach(() => {
    root().setAttribute("data-theme", "light");
    root().style.removeProperty("--moss-site-veil");
    root().style.removeProperty("--moss-ambient-band");
  });

  afterEach(() => {
    for (const m of live.splice(0)) m.destroy();
    document.querySelectorAll("iframe").forEach((f) => f.remove());
  });

  function mgr() {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);
    live.push(manager);
    return { iframe, manager };
  }

  test("an accent from the iframe sets the veil", () => {
    const { iframe } = mgr();
    siteAccent(iframe.contentWindow, CUSTOM_ACCENT);
    expect(root().style.getPropertyValue("--moss-site-veil")).toMatch(/^rgba\(/);
  });

  test("a spoofed source is ignored", () => {
    mgr();
    siteAccent(null, CUSTOM_ACCENT);
    expect(root().style.getPropertyValue("--moss-site-veil")).toBe("");
  });

  test("an iframe without a window is not spoofable by a null-source message", () => {
    // When the iframe has no browsing context its contentWindow is null,
    // and a null event.source (closed sender, MessagePort) would satisfy
    // `event.source === contentWindow` by null === null. The guard must
    // require a real source, not merely a matching one. (A disconnected
    // iframe stands in for the torn-down shell here — jsdom can't model
    // detach-after-load, but the null === null hazard is identical.)
    const iframe = document.createElement("iframe");
    expect(iframe.contentWindow).toBeNull();
    live.push(new NavigationManager(iframe));
    siteAccent(null, CUSTOM_ACCENT);
    expect(root().style.getPropertyValue("--moss-site-veil")).toBe("");
  });

  test("a null accent clears a previous page's sticky tint", () => {
    const { iframe } = mgr();
    siteAccent(iframe.contentWindow, CUSTOM_ACCENT);
    siteAccent(iframe.contentWindow, null);
    expect(root().style.getPropertyValue("--moss-site-veil")).toBe("");
  });

  test("the default accent (no custom branding) is treated as no site identity", () => {
    // Any site without a `.moss/theme/style.css` override inherits moss's
    // own baked-in default — the reported bug ("always green, matching the
    // default of moss").
    const { iframe } = mgr();
    siteAccent(iframe.contentWindow, CUSTOM_ACCENT);
    siteAccent(iframe.contentWindow, "rgb(45, 90, 45)");
    expect(root().style.getPropertyValue("--moss-site-veil")).toBe("");
  });

  test("an app theme flip re-derives the tint from the stored accent", () => {
    // Lightness comes from the app surface (transfer colour, never
    // luminance), so the same accent must yield different tints per theme.
    const { iframe } = mgr();
    siteAccent(iframe.contentWindow, CUSTOM_ACCENT);
    const light = root().style.getPropertyValue("--moss-site-veil");

    root().setAttribute("data-theme", "dark");
    return Promise.resolve().then(() => {
      const dark = root().style.getPropertyValue("--moss-site-veil");
      expect(dark).not.toBe("");
      expect(dark).not.toBe(light);
      root().setAttribute("data-theme", "light");
    });
  });

  test("reset() clears the tint — the folder-switch boundary", () => {
    // The accent is a per-SITE fact: it survives same-site navigation (no
    // clear-on-load) and dies with the site, exactly where lastPageColor does.
    const { iframe, manager } = mgr();
    siteAccent(iframe.contentWindow, CUSTOM_ACCENT);
    manager.reset();
    expect(root().style.getPropertyValue("--moss-site-veil")).toBe("");
  });
});

describe("NavigationManager: moss-chrome-ambient -> ambient band", () => {
  const live: NavigationManager[] = [];

  beforeEach(() => {
    root().setAttribute("data-theme", "light");
    root().style.removeProperty("--moss-site-veil");
    root().style.removeProperty("--moss-ambient-band");
  });

  afterEach(() => {
    for (const m of live.splice(0)) m.destroy();
    document.querySelectorAll("iframe").forEach((f) => f.remove());
  });

  function mgr() {
    const iframe = document.createElement("iframe");
    document.body.appendChild(iframe);
    const manager = new NavigationManager(iframe);
    live.push(manager);
    return { iframe, manager };
  }

  function ambient(source: Window | null, colors: unknown) {
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { type: "moss-chrome-ambient", colors },
        source,
      }),
    );
  }

  test("a chrome-ambient report writes the band", () => {
    const { iframe } = mgr();
    ambient(iframe.contentWindow, ["rgb(10, 20, 30)", "rgb(30, 40, 50)"]);
    expect(root().style.getPropertyValue("--moss-ambient-band")).not.toBe("");
  });

  test("a spoofed source writes nothing", () => {
    mgr();
    ambient(null, ["rgb(10, 20, 30)"]);
    expect(root().style.getPropertyValue("--moss-ambient-band")).toBe("");
  });

  test("iframe load clears the band but keeps the identity tint", () => {
    // Band is a per-PAGE content fact, the accent a per-SITE identity fact;
    // this asymmetry is the user-visible design: identity holds across
    // navigation while content-light follows content. The bridge re-samples
    // within a frame or two of load (and re-arms its dedup), so the
    // clear is a boundary, not a blackout.
    const { iframe } = mgr();
    siteAccent(iframe.contentWindow, CUSTOM_ACCENT);
    ambient(iframe.contentWindow, ["rgb(10, 20, 30)"]);
    iframe.dispatchEvent(new Event("load"));
    expect(root().style.getPropertyValue("--moss-ambient-band")).toBe("");
    expect(root().style.getPropertyValue("--moss-site-veil")).not.toBe("");
  });
});
