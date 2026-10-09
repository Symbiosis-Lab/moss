/**
 * The contents ruler and the section name on the island's button, as wired by
 * `initNavIsland()`.
 *
 * jsdom does no layout, so what is asserted here is wiring: who exists, who is
 * told what, and what is undone on a morph. Where the ruler sits, whether its
 * labels clear the text and whether it hides the button is geometry, asserted
 * in tests/render-gates/site/contents-ruler.spec.ts.
 */

import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

import { SECTION_SYNC_EVENT } from "../nav/section-event";
import { initNavIsland } from "../nav/nav-island";

const ISLAND = `
  <div class="moss-nav-island">
    <div class="moss-nav-island-bar">
      <nav class="moss-nav-island-trail">
        <a href="/" class="site-name" data-island-crumb>Home</a>
      </nav>
      <span class="moss-nav-island-actions">
        <button type="button" class="moss-nav-island-sections"
                aria-expanded="false" aria-label="Sections on this page"><svg></svg></button>
      </span>
    </div>
    <div class="moss-nav-island-menu" data-island-menu="levels" hidden></div>
    <div class="moss-nav-island-menu" data-island-menu="sections" hidden></div>
  </div>`;

const island = () => document.querySelector<HTMLElement>(".moss-nav-island")!;
const button = () => document.querySelector<HTMLButtonElement>(".moss-nav-island-sections")!;
const rulers = () => [...document.querySelectorAll<HTMLElement>(".moss-contents-ruler")];
const rulerTitles = () =>
  [...document.querySelectorAll<HTMLElement>(".moss-contents-ruler a")].map((a) =>
    a.getAttribute("aria-label"),
  );

function setPage(titles: string[]): void {
  document.querySelector("main")!.innerHTML = titles
    .map((t, i) => `<h2 id="s${i}">${t}</h2><p>…</p>`)
    .join("");
}

const nextFrame = (): Promise<void> => new Promise((resolve) => requestAnimationFrame(() => resolve()));

/** Put heading `i` at the scrollspy line and the rest above or below it, and
 *  let the frame that scroll schedules run. */
async function scrollTo(current: number): Promise<void> {
  placeHeadings(current);
  window.dispatchEvent(new Event("scroll"));
  await nextFrame();
}

function placeHeadings(current: number): void {
  document.querySelectorAll<HTMLElement>("main h2").forEach((h, i) => {
    h.getBoundingClientRect = () => ({ top: i <= current ? 0 : 900 }) as DOMRect;
  });
}

beforeEach(() => {
  document.body.innerHTML = `${ISLAND}<main></main><footer></footer>`;
  // No canvas in jsdom: titles are then shown whole and CSS does the clipping.
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

describe("the contents ruler", () => {
  test("lists the page's sections by their full titles, beside main", () => {
    setPage(["一", "二", "三"]);
    initNavIsland();
    expect(rulers()).toHaveLength(1);
    expect(rulerTitles()).toEqual(["一", "二", "三"]);
    // Inside `main` it would be positioned against the article, not the window.
    expect(rulers()[0].parentElement).toBe(document.body);
    expect(rulers()[0].previousElementSibling).toBe(document.querySelector("main"));
    expect(rulers()[0].getAttribute("aria-label")).toBe("Sections on this page");
  });

  test("a morph leaves exactly one ruler, and none on a page without a contents table", () => {
    setPage(["一", "二", "三"]);
    initNavIsland();
    setPage(["甲", "乙"]);
    initNavIsland();
    expect(rulers()).toHaveLength(1);
    expect(rulerTitles()).toEqual(["甲", "乙"]);

    setPage(["only"]);
    initNavIsland();
    expect(rulers()).toHaveLength(0);
  });

  test("it follows the island's scrollspy, with the sections panel closed", async () => {
    setPage(["一", "二", "三"]);
    initNavIsland();
    await scrollTo(1);
    const links = [...document.querySelectorAll(".moss-contents-ruler a")];
    expect(links.map((a) => a.getAttribute("aria-current"))).toEqual([null, "true", null]);
    expect(links.map((a) => a.hasAttribute("data-read"))).toEqual([true, false, false]);
    // …and the panel's rows carry the same index once it opens.
    button().click();
    const rows = [...document.querySelectorAll('[data-island-menu="sections"] a')];
    expect(rows.map((a) => a.getAttribute("aria-current"))).toEqual([null, "true", null]);
    expect(rows.map((a) => a.hasAttribute("data-read"))).toEqual([true, false, false]);
  });

  test("it asks nothing of the page itself: the event is its only input", () => {
    setPage(["一", "二", "三"]);
    initNavIsland();
    island().dispatchEvent(new CustomEvent(SECTION_SYNC_EVENT, { detail: { index: 2 } }));
    const links = [...document.querySelectorAll(".moss-contents-ruler a")];
    expect(links.map((a) => a.getAttribute("aria-current"))).toEqual([null, null, "true"]);
  });
});

describe("the section name on the sections button", () => {
  test("is empty before the first section and names the current one after", async () => {
    setPage(["一", "二"]);
    placeHeadings(-1);
    initNavIsland();
    const name = () => button().querySelector(".moss-nav-island-section-name")!.textContent;
    expect(name()).toBe("");
    await scrollTo(1);
    expect(name()).toBe("二");
  });

  test("the accessible name is the visible text behind a hidden prefix, never an aria-label", async () => {
    setPage(["一", "二"]);
    initNavIsland();
    await scrollTo(0);
    expect(button().hasAttribute("aria-label")).toBe(false);
    expect(button().textContent!.replace(/\s+/g, " ").trim()).toBe("Sections on this page 一");
  });

  test("a morph restores the emitted button, so the next page reads its own label", () => {
    setPage(["一", "二"]);
    initNavIsland();
    setPage(["only"]);
    initNavIsland();
    expect(button().getAttribute("aria-label")).toBe("Sections on this page");
    expect(button().querySelector(".moss-nav-island-section-name")).toBeNull();
  });
});

/**
 * jsdom has no layout, so every box is empty and the ruler never finds room.
 * Give it some: headings far to the right of the labels, an article that ends
 * well below the middle of the window. After this the ruler is on screen.
 */
function giveItRoom(): void {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (
    this: Element,
  ) {
    const isHeading = this.tagName === "H2";
    const isBar = this.classList.contains("moss-nav-island-bar");
    return {
      left: isHeading ? 600 : 0,
      top: 0,
      bottom: isBar ? 0 : 5000,
      height: 0,
      width: 0,
    } as DOMRect;
  });
}

describe("the island's sections button while the ruler is up", () => {
  afterEach(() => vi.restoreAllMocks());

  test("is hidden exactly while the ruler is on screen", () => {
    giveItRoom();
    setPage(["一", "二"]);
    initNavIsland();
    expect(island().dataset.ruler).toBe("on");
    expect(rulers()[0].hidden).toBe(false);

    // Scrolled past the article: the ruler fades, the button is its own again.
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(
      () => ({ left: 600, top: -4000, bottom: 0, height: 0, width: 0 }) as DOMRect,
    );
    window.dispatchEvent(new Event("scroll"));
    return new Promise<void>((resolve) =>
      requestAnimationFrame(() => {
        expect(island().dataset.ruler).toBe("off");
        expect(rulers()[0].hasAttribute("data-away")).toBe(true);
        resolve();
      }),
    );
  });

  test("keeps its button when a site switches the ruler off with CSS", () => {
    giveItRoom();
    const style = document.createElement("style");
    style.textContent = ".moss-contents-ruler { display: none; }";
    document.head.append(style);
    setPage(["一", "二"]);
    initNavIsland();
    expect(island().dataset.ruler).toBe("off");
    style.remove();
  });

  test("a morph to a page without a contents table gives the button back", () => {
    giveItRoom();
    setPage(["一", "二"]);
    initNavIsland();
    setPage(["only"]);
    initNavIsland();
    expect(island().dataset.ruler).toBeUndefined();
  });
});

describe("one pass per frame", () => {
  afterEach(() => vi.restoreAllMocks());

  test("a burst of scroll and resize events is one pass, not one each", async () => {
    setPage(["一", "二", "三"]);
    initNavIsland();
    let passes = 0;
    island().addEventListener(SECTION_SYNC_EVENT, () => passes++);
    for (let i = 0; i < 5; i++) {
      window.dispatchEvent(new Event("scroll"));
      window.dispatchEvent(new Event("resize"));
    }
    expect(passes, "nothing runs inside the event").toBe(0);
    await nextFrame();
    expect(passes).toBe(1);
  });

  test("the button comes back when a site hides the ruler later, on the next pass", async () => {
    giveItRoom();
    setPage(["一", "二"]);
    initNavIsland();
    expect(island().dataset.ruler).toBe("on");
    const style = document.createElement("style");
    style.textContent = ".moss-contents-ruler { visibility: hidden; display: none; }";
    document.head.append(style);
    window.dispatchEvent(new Event("scroll"));
    await nextFrame();
    expect(island().dataset.ruler).toBe("off");
    style.remove();
  });
});

describe("the sections panel's long titles", () => {
  afterEach(() => vi.restoreAllMocks());

  test("are cut in the middle, as on the ruler and the button, and keep their whole name", () => {
    const long = "第一章的開頭很長很長很長很長很長很長很長很長很長很長很長的結尾";
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
      measureText: (text: string) => ({ width: [...text].length * 10 }),
      font: "",
    } as unknown as CanvasRenderingContext2D);
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(150);
    setPage([long, "短"]);
    initNavIsland();
    button().dispatchEvent(new MouseEvent("click", { detail: 1, bubbles: true }));
    const [first, second] = [...document.querySelectorAll('[data-island-menu="sections"] a')];
    expect([...first.textContent!].length).toBeLessThanOrEqual(15);
    const [head, tail] = first.textContent!.split("…");
    expect(long.startsWith(head)).toBe(true);
    expect(long.endsWith(tail)).toBe(true);
    expect(head.length).toBeGreaterThan(0);
    expect(tail.length).toBeGreaterThan(0);
    expect(first.getAttribute("aria-label")).toBe(long);
    expect(second.textContent).toBe("短");
    expect(second.hasAttribute("aria-label")).toBe(false);
  });
});
