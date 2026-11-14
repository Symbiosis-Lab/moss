/**
 * A preview morph reconciles the page with the served HTML, which does not
 * carry what page scripts built at load. After the morph settles, the page must
 * hold every script-built part a fresh load of the same HTML holds.
 *
 * Each entry in `SCRIPTS` is one load-time script: how it runs on a page, and
 * the elements it builds. A new load-only script is covered by being listed.
 *
 * Not listed: the viewer lightbox and the link-preview card are made on first
 * use, so a fresh load has none either; the contents ruler is rebuilt by the
 * nav island on every morph (nav-submenu / contents-ruler tests).
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { applyMorph } from "../../bridge/__tests__/apply-morph";
import { initSidenotes } from "../sidenotes";
import { initSelectionActions } from "../selection-actions";

/** The served page: `n` footnotes, the selection tools' marker, an article. */
function served(n: number): string {
  const refs = Array.from(
    { length: n },
    (_, i) =>
      `<sup class="moss-footnote-ref" id="fnref-${i + 1}"><a href="#fn-${i + 1}">${i + 1}</a></sup>`,
  ).join(" ");
  const notes = Array.from(
    { length: n },
    (_, i) =>
      `<li id="fn-${i + 1}" tabindex="-1"><p>Note ${i + 1}. <a class="moss-footnote-backref" href="#fnref-${i + 1}">&#8617;</a></p></li>`,
  ).join("");
  return `<html><head></head><body><article class="container"><h1>Title</h1><p>Body ${refs}.</p><section class="moss-footnotes" role="doc-endnotes"><ol>${notes}</ol></section></article><script data-share-card="/share.js"></script></body></html>`;
}

interface Script {
  name: string;
  /** What the script does when the page loads. */
  run: () => void;
  /** The elements it builds, as selectors. */
  built: string[];
}

const SCRIPTS: Script[] = [
  {
    name: "sidenotes",
    run: initSidenotes,
    built: [
      "aside.moss-sidenote",
      "span.moss-sidenote-number",
      "[aria-describedby^='moss-sidenote-']",
      "h2.moss-footnotes-title",
      "div.moss-footnote-scroller",
      "button.moss-footnote-grab",
    ],
  },
  {
    name: "selection actions",
    run: initSelectionActions,
    built: [
      "div.sel-popover",
      ".sel-popover button.sel-share",
      ".sel-popover span.sel-sep",
      ".sel-popover button.sel-copy",
      "div.mobile-bar",
      ".mobile-bar div.pill-bar",
      ".mobile-bar button.pill-bar-btn.sel-share",
    ],
  },
];

function groups(selectors: string[]): Record<string, number> {
  return Object.fromEntries(
    selectors.map((s) => [s, document.querySelectorAll(s).length]),
  );
}

/** Load `html` into an empty document and run the script, as a page load does. */
function load(html: string, script: Script): void {
  document.head.innerHTML = "";
  document.body.innerHTML = new DOMParser().parseFromString(html, "text/html").body.innerHTML;
  script.run();
}

/** What a fresh load of `html` builds. */
function fresh(html: string, script: Script): Record<string, number> {
  load(html, script);
  return groups(script.built);
}

/** Morph to `html` and dispatch what the bridge dispatches. */
function morph(html: string): void {
  applyMorph(html);
  document.dispatchEvent(new CustomEvent("moss-morph-patched"));
}

beforeEach(() => {
  window.matchMedia ??= (() => ({ matches: false })) as unknown as typeof window.matchMedia;
});

describe.each(SCRIPTS)("$name after a morph", (script) => {
  it.each([
    ["the same page", 3, 3],
    ["a page with one more footnote", 3, 4],
  ])("builds what a fresh load builds: %s", (_label, before, after) => {
    const expected = fresh(served(after), script);
    expect(Object.values(expected).every((n) => n > 0)).toBe(true);

    load(served(before), script);
    morph(served(after));
    expect(groups(script.built)).toEqual(expected);

    // A second morph must not stack a second copy of anything.
    morph(served(after));
    expect(groups(script.built)).toEqual(expected);
  });
});

describe("sidenotes after a morph", () => {
  it("describes each marker by its margin note again", () => {
    load(served(2), SCRIPTS[0]);
    morph(served(2));
    const link = document.querySelector("#fnref-1 a")!;
    expect(link.getAttribute("aria-describedby")).toBe("moss-sidenote-1");
    expect(document.getElementById("moss-sidenote-1")?.textContent).toContain("Note 1.");
  });

  it("leaves one live set of listeners after two morphs", () => {
    const add = vi.spyOn(window, "addEventListener");
    load(served(2), SCRIPTS[0]);
    morph(served(2));
    morph(served(2));
    const live = add.mock.calls.filter(
      ([type, , opts]) =>
        type === "hashchange" && !(opts as AddEventListenerOptions).signal?.aborted,
    );
    add.mockRestore();
    expect(live).toHaveLength(1);
  });

  it.each([
    ["margin notes", ""],
    ["the sheet", "<style>.moss-sidenote { display: none; }</style>"],
  ])("does not replay a #fn-1 arrival: %s", (_label, style) => {
    vi.stubGlobal("matchMedia", () => ({ matches: true }));
    const scroll = vi.fn();
    Element.prototype.scrollIntoView = scroll;
    const page = served(2).replace("<article", `${style}<article`);
    load(page, SCRIPTS[0]);
    history.replaceState(null, "", "#fn-1");
    scroll.mockClear();
    morph(page);
    history.replaceState(null, "", "#");
    vi.unstubAllGlobals();
    expect(scroll).not.toHaveBeenCalled();
    expect(document.querySelector(".moss-footnote-landed")).toBeNull();
    expect(document.querySelector(".moss-footnotes-lifted")).toBeNull();
  });

  describe.each([
    ["margin notes", ""],
    ["the sheet", "<style>.moss-sidenote { display: none; }</style>"],
  ])("a re-run over built notes: %s", (_label, style) => {
    const page = () => served(3).replace("<article", `${style}<article`);
    const built = () => ({
      ...groups(SCRIPTS[0].built),
      elements: document.body.querySelectorAll("*").length,
    });

    it("yields one set however often the init runs", () => {
      const expected = fresh(page(), SCRIPTS[0]);
      expect(Object.values(expected).every((n) => n > 0)).toBe(true);
      const expectedAll = built();
      initSidenotes(false);
      initSidenotes(false);
      expect(built()).toEqual(expectedAll);
    });

    it("yields one set when the morph left the built notes in place", () => {
      fresh(page(), SCRIPTS[0]);
      const expectedAll = built();
      morph_skipped();
      expect(built()).toEqual(expectedAll);
    });
  });
});

/** The event alone: a morph that removed nothing of what the script built. */
function morph_skipped(): void {
  document.dispatchEvent(new CustomEvent("moss-morph-patched"));
}
