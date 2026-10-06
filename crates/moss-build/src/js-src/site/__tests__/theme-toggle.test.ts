/**
 * The site's moon: a flip of the showing theme, remembered under
 * localStorage["moss-theme"], the key shell.html's pre-paint script reads.
 */

import { describe, test, expect, beforeEach } from "vitest";

describe("toggleTheme", () => {
  beforeEach(async () => {
    localStorage.clear();
    await import("../theme");
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
});
