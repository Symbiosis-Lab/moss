/**
 * Tests for `foldPlan` — which ancestors the island's trail drops at a given
 * width.
 *
 * jsdom does no layout, so it cannot measure a crumb; that is the engine's job
 * and it is asserted in tests/render-gates/site/nav-island.spec.ts. What lives
 * here is the *decision* made from a measurement, stated as numbers: given
 * these widths and this much room, which crumbs go?
 *
 * The rule being pinned, and the reason it is not a media query: the first
 * crumb and the current page always survive (they are the two ends of "where
 * am I"), and the middle is sacrificed left to right, because the ancestor
 * nearest the reader locates them better than the one nearest the site root.
 */

import { describe, test, expect, beforeEach } from "vitest";

import { foldPlan, outerWidth } from "../nav/breadcrumb-fold";
import { initNavIsland } from "../nav/nav-island";

// A five-level trail: 潮汐週報 / 獎項 / 寫作獎 / 第一季 / 末代女礦工…
const TRAIL = [100, 60, 70, 60, 220];
const SEP = 10;
const MORE = 20;

/** Width the row needs with nothing folded. */
const FULL = TRAIL.reduce((a, b) => a + b, 0) + SEP * (TRAIL.length - 1); // 550

describe("foldPlan", () => {
  test("a trail that fits folds nothing", () => {
    expect(foldPlan(TRAIL, SEP, MORE, FULL)).toEqual([]);
    expect(foldPlan(TRAIL, SEP, MORE, 900)).toEqual([]);
  });

  test("sub-pixel overflow is slack, not a fold", () => {
    // Fractional layout widths make an exact compare flicker between folded and
    // unfolded on a window nobody resized.
    expect(foldPlan(TRAIL, SEP, MORE, FULL - 1)).toEqual([]);
  });

  test("ancestors go from the middle outward, leftmost first", () => {
    // Room for one fold (that row measures 510): 獎項 (index 1) is the least
    // useful ancestor, so it is the first to go.
    expect(foldPlan(TRAIL, SEP, MORE, 520)).toEqual([1]);
    // Tighter: 寫作獎 follows. 第一季 — nearest the reader — is still there.
    expect(foldPlan(TRAIL, SEP, MORE, 430)).toEqual([1, 2]);
    // Tighter still: every ancestor is gone.
    expect(foldPlan(TRAIL, SEP, MORE, 300)).toEqual([1, 2, 3]);
  });

  test("the first crumb and the current page always survive", () => {
    // Absurdly narrow — a phone with a long title. The two ends of "where am
    // I" are never sacrificed, so the answer stops at every middle index.
    expect(foldPlan(TRAIL, SEP, MORE, 10)).toEqual([1, 2, 3]);
    expect(foldPlan(TRAIL, SEP, MORE, 0)).toEqual([1, 2, 3]);
  });

  test("folding is charged for the button it adds", () => {
    // Hiding one 60px ancestor removes 60 + one 10px separator = 70, but adds
    // the 20px `…` and its own separator = 30. Net saving is 40, not 70, and
    // the plan has to account for that or it under-folds and the row overflows.
    const widths = [100, 60, 220];
    const full = 100 + 60 + 220 + SEP * 2; // 400
    expect(foldPlan(widths, SEP, MORE, full)).toEqual([]);
    // 361..390 is the band where the naive arithmetic ("just drop 70") would
    // wrongly say one fold is enough.
    expect(foldPlan(widths, SEP, MORE, 370)).toEqual([1]);
    // …and one fold genuinely is enough at 370: the folded row is
    // 100 + 220 + 20 + 2×10 = 360. At 350 it is not, and there is nothing
    // further to drop, so the plan returns the maximal fold anyway.
    expect(foldPlan(widths, SEP, MORE, 350)).toEqual([1]);
  });

  test("the current page truncates before an ancestor is dropped", () => {
    // The current page is the ONE crumb allowed to ellipsise, so
    // the room it can give back has to be spent before any ancestor leaves the
    // screen: a cut title still says which page you are on, a dropped ancestor
    // says nothing at all. Charging it full width instead — which is what the
    // first implementation did — inverts that and over-folds.
    const FLOOR = 60;

    // The headline case. At 430 the old plan dropped TWO ancestors; the title
    // had 160px of give sitting unused right next to them. Spend that first
    // and the row fits whole: charged row = 100+60+70+60+60 + 4×10 = 390.
    expect(foldPlan(TRAIL, SEP, MORE, 430)).toEqual([1, 2]);
    expect(foldPlan(TRAIL, SEP, MORE, 430, FLOOR)).toEqual([]);

    // The give is finite, and once it is spent the folds resume — still
    // leftmost first, so 第一季 outlives 獎項.
    expect(foldPlan(TRAIL, SEP, MORE, 360, FLOOR)).toEqual([1]);
    expect(foldPlan(TRAIL, SEP, MORE, 300, FLOOR)).toEqual([1, 2]);
    expect(foldPlan(TRAIL, SEP, MORE, 250, FLOOR)).toEqual([1, 2, 3]);

    // And the floor is a floor: it never squeezes past it to save an ancestor.
    expect(foldPlan(TRAIL, SEP, MORE, 10, FLOOR)).toEqual([1, 2, 3]);
  });

  test("a floor wider than the title cannot invent room", () => {
    // A short current page with a generous `min-width`: the crumb is already
    // narrower than the floor, so there is nothing to give back and the plan
    // must match the no-floor answer rather than pretending it shrank.
    const short = [100, 60, 70, 60, 40];
    expect(foldPlan(short, SEP, MORE, 260, 200)).toEqual(foldPlan(short, SEP, MORE, 260));
  });

  test("a separator's margins count as room the row has to pay for", () => {
    // The bug this pins: `offsetWidth` stops at the border, so the fold plan
    // charged a "/" 4px when it takes 11 — and a row it measured at 203px
    // rendered at 224px inside a 210px trail. jsdom does no layout, so
    // `offsetWidth` is 0 here and only the margins move; that is enough to
    // prove they are counted at all, which is the part that was missing.
    const sep = document.createElement("span");
    sep.style.marginLeft = "3.5px";
    sep.style.marginRight = "3.5px";
    document.body.appendChild(sep);
    expect(outerWidth(sep)).toBe(7);

    // No margins declared: read as 0, not NaN. A NaN here would poison the
    // whole row total and the trail would never fold.
    const bare = document.createElement("span");
    document.body.appendChild(bare);
    expect(outerWidth(bare)).toBe(0);
  });

  test("a trail with no middle cannot fold", () => {
    // A top-level page: site name + the page, nothing droppable between them.
    expect(foldPlan([100, 220], SEP, MORE, 10)).toEqual([]);
    expect(foldPlan([100], SEP, MORE, 10)).toEqual([]);
    expect(foldPlan([], SEP, MORE, 10)).toEqual([]);
  });
});

/**
 * The sections panel across a morph, and who gets a focus ring.
 *
 * A preview rebuild morphs the page in place, so `initNavIsland()` runs again
 * over an island that survived from the previous page. Everything below is a
 * question about that second run — which is why jsdom can answer it: it is
 * about which nodes and attributes exist, not about where anything sits.
 */
describe("initNavIsland — the contents gate and the sections panel", () => {
  /** The island as `island.rs` emits it: both panels present, empty, hidden. */
  const ISLAND = `
    <div class="moss-nav-island">
      <div class="moss-nav-island-bar">
        <nav class="moss-nav-island-trail">
          <a href="/" class="site-name" data-island-crumb>潮汐週報</a>
        </nav>
        <span class="moss-nav-island-actions">
          <button type="button" class="moss-nav-island-sections"
                  aria-expanded="false" aria-label="sections"></button>
        </span>
      </div>
      <div class="moss-nav-island-menu" data-island-menu="levels" hidden></div>
      <div class="moss-nav-island-menu" data-island-menu="sections" hidden></div>
    </div>`;

  const sectionsBtn = () =>
    document.querySelector<HTMLButtonElement>(".moss-nav-island-sections")!;
  const sectionsMenu = () =>
    document.querySelector<HTMLElement>('[data-island-menu="sections"]')!;

  /** Replace what the island is describing, as a morph does. */
  function setPage(headings: string[]): void {
    const main = document.querySelector("main")!;
    main.innerHTML = headings
      .map((h, i) => `<h2 id="sec-${i}">${h}</h2><p>…</p>`)
      .join("");
  }

  beforeEach(() => {
    document.body.innerHTML = `${ISLAND}<main></main>`;
  });

  const island = () => document.querySelector<HTMLElement>(".moss-nav-island")!;

  test("a page with no headings gets no island at all", () => {
    setPage([]);
    initNavIsland();
    expect(island().getAttribute("data-shown")).toBeNull();
    expect(sectionsMenu().children).toHaveLength(0);
  });

  test("one heading is not a contents table — the island stays dormant", () => {
    // As amended 2026-08-30: a single row names the page's only
    // section, which the title directly above it already said; the island
    // would be a bar summoned to repeat the heading the reader just read.
    setPage(["一"]);
    initNavIsland();
    expect(island().getAttribute("data-shown")).toBeNull();
    expect(sectionsMenu().children).toHaveLength(0);
  });

  test("two headings earn it, and each becomes a row that links to its section", () => {
    // The boundary AND the panel's contents: one test, because a third
    // heading would only re-assert what the second already proves.
    setPage(["一", "二"]);
    initNavIsland();
    expect(island().getAttribute("data-shown")).toBe("false");
    expect([...sectionsMenu().children].map((r) => r.textContent)).toEqual(["一", "二"]);
    expect([...sectionsMenu().children].map((r) => r.getAttribute("href"))).toEqual([
      "#sec-0",
      "#sec-1",
    ]);
  });

  test("a morph to a page with no headings puts the island away again", () => {
    // The bug this guards: the old code only ever UNhid the sections button, so
    // a page with no headings kept whatever button its predecessor had turned
    // on — opening a panel that listed the previous page's sections. The
    // contents gate now governs the same case one level up, and the island
    // itself has to go dormant rather than stand there empty.
    setPage(["一", "二"]);
    initNavIsland();
    expect(island().getAttribute("data-shown")).toBe("false");

    setPage([]);
    initNavIsland();
    expect(island().getAttribute("data-shown")).toBeNull();
    expect(sectionsMenu().children).toHaveLength(0);
  });

  test("a morph rebuilds the list instead of stacking onto it", () => {
    // Rows were appended without clearing, so after a morph the panel held the
    // new page's sections underneath the old page's — and the stale ones linked
    // to ids no longer in the document, so clicking them went nowhere. That is
    // the whole of "clicking the table of contents doesn't jump".
    setPage(["一", "二", "三"]);
    initNavIsland();

    setPage(["甲", "乙"]);
    initNavIsland();

    expect([...sectionsMenu().children].map((r) => r.textContent)).toEqual(["甲", "乙"]);
  });

  test("a morph leaves the panel closed rather than mid-open", () => {
    setPage(["一", "二"]);
    initNavIsland();
    sectionsBtn().dispatchEvent(new MouseEvent("click", { detail: 1, bubbles: true }));
    expect(sectionsMenu().hidden).toBe(false);

    setPage(["甲", "乙"]);
    initNavIsland();
    expect(sectionsMenu().hidden).toBe(true);
    expect(sectionsBtn().getAttribute("aria-expanded")).toBe("false");
  });

  test("a pointer open does not put a focus ring on the first row", () => {
    // Focusing the first row draws `:focus-visible` around it — a box the
    // reader never asked for and cannot dismiss. With a pointer they are
    // already where they need to be, so nothing should take focus.
    setPage(["一", "二"]);
    initNavIsland();
    sectionsBtn().dispatchEvent(new MouseEvent("click", { detail: 1, bubbles: true }));

    expect(sectionsMenu().hidden).toBe(false);
    expect(document.activeElement).not.toBe(sectionsMenu().firstElementChild);
  });

  test("sections come from the shallowest heading level the page uses", () => {
    // An author who started at `####` has sections all the same. Hard-coding
    // `h2` reported none, so the panel was empty and — once the button follows
    // the heading count — there was no button either.
    const main = document.querySelector("main")!;

    main.innerHTML = `<h4 id="a">01</h4><h4 id="b">02</h4>`;
    initNavIsland();
    expect(island().getAttribute("data-shown")).toBe("false");
    expect([...sectionsMenu().children].map((r) => r.textContent)).toEqual(["01", "02"]);

    // And an ordinary page is untouched: `h2` still wins whenever it exists,
    // so the list never mixes two levels.
    main.innerHTML = `<h2 id="c">一</h2><h3 id="d">1.1</h3><h2 id="e">二</h2>`;
    initNavIsland();
    expect([...sectionsMenu().children].map((r) => r.textContent)).toEqual(["一", "二"]);
  });

  test("a keyboard open still lands on the first row", () => {
    // Enter/Space on a button fires `click` with `detail === 0`, and that is
    // the case that genuinely needs focus moved — it is the only way in.
    setPage(["一", "二"]);
    initNavIsland();
    sectionsBtn().dispatchEvent(new MouseEvent("click", { detail: 0, bubbles: true }));

    expect(document.activeElement).toBe(sectionsMenu().firstElementChild);
  });
});
