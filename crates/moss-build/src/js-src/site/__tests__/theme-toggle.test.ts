/**
 * The site's moon: a flip of the showing theme, remembered under
 * localStorage["moss-theme"], the key shell.html's pre-paint script reads.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";
import { runInNewContext } from "node:vm";
import shell from "../../../assets/templates/shell.html?raw";

describe("shell.html pre-paint", () => {
  const script = shell.match(/<script>([\s\S]*?)<\/script>/)![1];

  test.each([
    ["light", true, "light"],
    ["dark", false, "dark"],
    [null, true, "dark"],
    [null, false, "light"],
  ] as const)("auto: saved=%s systemDark=%s resolves to %s", (saved, systemDark, expected) => {
    const root = document.createElement("html");
    runInNewContext(script, {
      document: { documentElement: root },
      localStorage: { getItem: () => saved },
      window: { matchMedia: () => ({ matches: systemDark }) },
    });
    expect(root.getAttribute("data-theme")).toBe(expected);
  });

  test.each([true, false])("auto: blocked storage falls back to systemDark=%s", (systemDark) => {
    const root = document.createElement("html");
    runInNewContext(script, {
      document: { documentElement: root },
      get localStorage() { throw new DOMException("blocked", "SecurityError"); },
      window: { matchMedia: () => ({ matches: systemDark }) },
    });
    expect(root.getAttribute("data-theme")).toBe(systemDark ? "dark" : "light");
  });

  test("auto: missing matchMedia falls back to light", () => {
    const root = document.createElement("html");
    runInNewContext(script, {
      document: { documentElement: root },
      localStorage: { getItem: () => null },
      window: {},
    });
    expect(root.getAttribute("data-theme")).toBe("light");
  });
});

describe("toggleTheme", () => {
  beforeEach(async () => {
    document.documentElement.removeAttribute("data-moss-color-scheme");
    localStorage.clear();
    await import("../theme");
  });

  afterEach(() => document.documentElement.removeAttribute("data-moss-color-scheme"));

  test.each(["light", "dark"])("does nothing under a fixed %s scheme", (scheme) => {
    const root = document.documentElement;
    root.setAttribute("data-moss-color-scheme", scheme);
    root.setAttribute("data-theme", scheme);
    const saved = scheme === "light" ? "dark" : "light";
    localStorage.setItem("moss-theme", saved);
    const changed = vi.fn();
    root.addEventListener("moss-theme-change", changed);
    try {
      window.toggleTheme();
      expect(root.getAttribute("data-theme")).toBe(scheme);
      expect(localStorage.getItem("moss-theme")).toBe(saved);
      expect(changed).not.toHaveBeenCalled();
    } finally {
      root.removeEventListener("moss-theme-change", changed);
    }
  });

  test("light → dark", () => {
    document.documentElement.setAttribute("data-theme", "light");
    window.toggleTheme();
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(localStorage.getItem("moss-theme")).toBe("dark");
  });

  test("dark → light", () => {
    document.documentElement.setAttribute("data-theme", "dark");
    window.toggleTheme();
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(localStorage.getItem("moss-theme")).toBe("light");
  });

  test("still flips when storage access throws", () => {
    const blocked = vi.spyOn(window, "localStorage", "get").mockImplementation(() => {
      throw new DOMException("blocked", "SecurityError");
    });
    try {
      document.documentElement.setAttribute("data-theme", "light");
      expect(() => window.toggleTheme()).not.toThrow();
      expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    } finally {
      blocked.mockRestore();
    }
  });
});
