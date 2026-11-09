/**
 * mount.ts reads the site's mount from `<meta name="moss:base">` once, at load,
 * and link previews use it both ways: the fetch of `previews.json` is mounted,
 * and a mounted link's path is stripped before it is looked up there.
 */

import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";

async function loadWithBase(base: string | null): Promise<typeof import("../mount")> {
  vi.resetModules();
  document.head.querySelectorAll('meta[name="moss:base"]').forEach((m) => m.remove());
  if (base !== null) {
    const meta = document.createElement("meta");
    meta.name = "moss:base";
    meta.content = base;
    document.head.prepend(meta);
  }
  return import("../mount");
}

describe("mount", () => {
  test("a site at the root changes nothing", async () => {
    const { BASE, siteUrl, sitePath } = await loadWithBase(null);
    expect(BASE).toBe("/");
    expect(siteUrl("/notes/")).toBe("/notes/");
    expect(sitePath("/notes/")).toBe("/notes/");
  });

  test("a site under a path mounts site paths and strips them back", async () => {
    const { BASE, siteUrl, sitePath } = await loadWithBase("/~alice/");
    expect(BASE).toBe("/~alice/");
    expect(siteUrl("/_moss/previews.json")).toBe("/~alice/_moss/previews.json");
    expect(siteUrl("//cdn.example/x")).toBe("//cdn.example/x");
    expect(siteUrl("https://example.com/x")).toBe("https://example.com/x");
    expect(siteUrl("rel/")).toBe("rel/");
    expect(sitePath("/~alice/notes/")).toBe("/notes/");
    expect(sitePath("/~alice")).toBe("/");
    expect(sitePath("/~bob/x")).toBe("/~bob/x");
  });
});

describe("link previews under a mount", () => {
  const fetched: string[] = [];

  beforeEach(() => {
    vi.useFakeTimers();
    fetched.length = 0;
    vi.stubGlobal("fetch", vi.fn((url: string) => {
      fetched.push(url);
      return Promise.resolve({ json: () => Promise.resolve({ "/notes/": { title: "Notes" } }) } as Response);
    }));
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.querySelectorAll("main").forEach((m) => m.remove());
  });

  test("fetches the mounted previews.json and finds a mounted link's site path", async () => {
    await loadWithBase("/~alice/");
    await import("../link-preview");
    const main = document.createElement("main");
    main.innerHTML = `<p><a class="wikilink" href="${location.origin}/~alice/notes/">Notes</a></p>`;
    document.body.appendChild(main);
    main.querySelector("a")!.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await vi.advanceTimersByTimeAsync(300);
    expect(fetched).toEqual(["/~alice/_moss/previews.json"]);
    expect(document.querySelector(".moss-preview-popup.visible")?.textContent).toContain("Notes");
  });
});
