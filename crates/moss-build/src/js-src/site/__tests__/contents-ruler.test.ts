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

  test("a section is current once its heading passes 35% of the window's height", async () => {
    setPage(["一", "二", "三"]);
    initNavIsland();
    const at = async (top: number): Promise<number> => {
      document.querySelectorAll<HTMLElement>("main h2").forEach((h, i) => {
        h.getBoundingClientRect = () => ({ top: i < 1 ? -400 : i === 1 ? top : top + 900 }) as DOMRect;
      });
      window.dispatchEvent(new Event("scroll"));
      await nextFrame();
      return [...document.querySelectorAll(".moss-contents-ruler a")].findIndex((a) => a.getAttribute("aria-current") === "true");
    };
    expect(await at(window.innerHeight * 0.3)).toBe(1);
    expect(await at(window.innerHeight * 0.4)).toBe(0);
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
 * Give it some: the article's text far to the right of the labels, an article
 * that ends well below the middle of the window. After this the ruler is on
 * screen.
 */
function giveItRoom(): void {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (
    this: Element,
  ) {
    const isBar = this.classList.contains("moss-nav-island-bar");
    return {
      left: this.closest("main") ? 600 : 0,
      top: 0,
      bottom: isBar ? 0 : 5000,
      height: 0,
      width: 0,
    } as DOMRect;
  });
}

describe("the island's sections button while the ruler is up", () => {
  afterEach(() => vi.restoreAllMocks());

  test("is hidden wherever the ruler is the contents list, whatever the scroll", async () => {
    giveItRoom();
    setPage(["一", "二"]);
    initNavIsland();
    expect(island().dataset.ruler).toBe("on");
    const place = (rect: (el: Element) => Partial<DOMRect>): Promise<void> => {
      vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
        return { left: 600, top: 0, bottom: 0, height: 0, width: 0, ...rect(this) } as DOMRect;
      });
      window.dispatchEvent(new Event("scroll"));
      return nextFrame();
    };

    // Scrolled past the article, to the foot of the page: still the ruler's.
    await place(() => ({ top: -4000, bottom: -3000 }));
    expect([island().dataset.ruler, rulers()[0].hasAttribute("data-away")]).toEqual(["on", false]);

    // Over the opening the ruler fades, and the island still has no sections.
    await place((el) => (el.matches("main h2") ? { top: 900 } : { top: 300 }));
    expect([island().dataset.ruler, rulers()[0].hasAttribute("data-away")]).toEqual(["away", true]);
  });

  test("the bar still comes on a scroll up, and an open sections panel closes", async () => {
    giveItRoom();
    setPage(["一", "二"]);
    initNavIsland();
    expect(island().dataset.ruler).toBe("on");
    const scrollToY = async (y: number): Promise<void> => {
      Object.defineProperty(window, "scrollY", { value: y, configurable: true });
      window.dispatchEvent(new Event("scroll"));
      await nextFrame();
    };
    await scrollToY(2000);
    await scrollToY(1800);
    expect([island().dataset.shown, island().inert]).toEqual(["true", false]);

    const panel = document.querySelector<HTMLElement>('[data-island-menu="sections"]')!;
    panel.hidden = false;
    button().setAttribute("aria-expanded", "true");
    await scrollToY(1700);
    expect([panel.hidden, button().getAttribute("aria-expanded")]).toEqual([true, "false"]);
    await scrollToY(0);
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

/**
 * A block a theme pushes into the margin, as rects: the text column at 600,
 * the labels from 0 (so 576px of room), a `data-at` block at its own left
 * edge and 800 wide (as is a `.wide` one at 300), a `data-zero` node with no
 * box. Everything shares the
 * ruler's height, so every band sits beside it.
 */
function widenedPage(blocks: string): void {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const at = this.getAttribute("data-at") ?? (this.classList.contains("wide") ? "300" : null);
    const left = at !== null ? Number(at) : this.closest("main") && !this.hasAttribute("data-zero") ? 600 : 0;
    const width = at !== null ? 800 : this.closest("main") && !this.hasAttribute("data-zero") ? 400 : 0;
    const isBar = this.classList.contains("moss-nav-island-bar");
    return { left, right: left + width, width, top: 0, bottom: isBar ? 0 : 5000, height: 0 } as DOMRect;
  });
  document.querySelector("main")!.innerHTML = `<h2 id="s0">一</h2><p>…</p>${blocks}<h2 id="s1">二</h2><p>…</p>`;
}

const labelWidth = () => document.querySelector<HTMLElement>(".moss-contents-ruler-label")!.style.maxWidth;

describe("the labels beside a block the theme widens", () => {
  afterEach(() => vi.restoreAllMocks());

  test("stop one gutter short of a widened block's text, not of a link inside it", () => {
    widenedPage(`<div data-at="300">Figures <a href="#">in full</a></div>`);
    initNavIsland();
    expect(labelWidth()).toBe("276px");
  });

  test("stop short of a widened block with no content of its own, such as a rule", () => {
    widenedPage(`<hr data-at="300">`);
    initNavIsland();
    expect(labelWidth()).toBe("276px");
  });

  test("keep the text's gutter beside a band whose content stays in the column", () => {
    widenedPage(`<div data-at="0"><p>in the column</p></div>`);
    initNavIsland();
    expect(labelWidth()).toBe("576px");
  });

  test("ignore a node that has no box", () => {
    widenedPage(`<script data-zero>void 0</script>`);
    initNavIsland();
    expect([labelWidth(), rulers()[0].hasAttribute("data-away")]).toEqual(["576px", false]);
  });

  test("stop short of a widened block script adds after load, on the next pass", async () => {
    widenedPage("");
    initNavIsland();
    expect(labelWidth()).toBe("576px");
    document.querySelector("main h2")!.insertAdjacentHTML("afterend", `<div data-at="300">Figures</div>`);
    await Promise.resolve();
    window.dispatchEvent(new Event("scroll"));
    await nextFrame();
    expect(labelWidth()).toBe("276px");
  });

  test("stop short of a block a class change widens after load, on the next pass", async () => {
    widenedPage(`<div id="figures">Figures</div>`);
    initNavIsland();
    expect(labelWidth()).toBe("576px");
    document.getElementById("figures")!.classList.add("wide");
    await Promise.resolve();
    window.dispatchEvent(new Event("scroll"));
    await nextFrame();
    expect(labelWidth()).toBe("276px");
  });
});

test("a pictures ruler on a vertical page is not sent away by where its text begins", () => {
  // The lane is reserved and the dashes end at 40. Every block starts at 100
  // except the first heading, whose left edge a vertical page puts left of the
  // dashes, on a line above the ruler's span.
  document.body.dataset.contents = "pictures";
  document.body.dataset.typesetting = "vertical";
  document.body.style.setProperty("--moss-contents-lane", "8rem");
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const inMain = this.closest("main") !== null;
    const heading = this.matches("main h2#s0");
    const left = heading ? 0 : inMain ? 100 : 0;
    const width = inMain ? 400 : this.tagName === "OL" ? 40 : 0;
    const [top, bottom] = heading ? [-600, -500] : [0, this.classList.contains("moss-nav-island-bar") ? 0 : 5000];
    return { left, right: left + width, width, top, bottom, height: 0 } as DOMRect;
  });
  setPage(["一", "二"]);
  initNavIsland();
  expect([island().dataset.ruler, rulers()[0].hasAttribute("data-away")]).toEqual(["on", false]);
  vi.restoreAllMocks();
});

describe("a section's short name", () => {
  test("names it on the ruler, the button and the panel; the heading keeps its text", async () => {
    setPage(["Opening hours of the reading room", "Two", "Three"]);
    const first = document.querySelector<HTMLElement>("main h2")!;
    first.dataset.contentsLabel = "Opening hours";
    first.insertAdjacentHTML("beforeend", '<a class="moss-heading-anchor" href="#s0"></a>');
    initNavIsland();
    expect(rulerTitles()).toEqual(["Opening hours", "Two", "Three"]);
    expect(document.querySelector(".moss-contents-ruler-label")!.textContent).toBe("Opening hours");
    await scrollTo(0);
    expect(document.querySelector(".moss-nav-island-section-name")!.textContent).toBe("Opening hours");
    button().dispatchEvent(new MouseEvent("click", { detail: 1, bubbles: true }));
    const rows = [...document.querySelectorAll('[data-island-menu="sections"] a')].map((a) => a.textContent);
    expect(rows).toEqual(["Opening hours", "Two", "Three"]);
    expect(first.textContent).toBe("Opening hours of the reading room");
  });
});
