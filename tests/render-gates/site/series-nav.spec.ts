/**
 * The series nav paints only an arrow and the neighbour's title: arrow and
 * placement say which way a link goes, so the words "Previous" / "Next" are
 * never painted. They stay in the link's name for screen readers. The arrow
 * sits on the title's first line, a word's gap from it.
 */
import { test, expect, type Page } from "../../support/browser-test";

/** The arrow's gap needs to read as a space between words at least. */
const MIN_GAP = 8;

async function sides(page: Page) {
  return page.evaluate(() => {
    const ink = (el: Element) => {
      const r = document.createRange();
      r.selectNodeContents(el);
      return r.getClientRects();
    };
    return [...document.querySelectorAll("a.moss-series-nav-link")].map((a) => {
      const arrow = a.querySelector(".moss-series-nav-arrow")!;
      const title = a.querySelector(".moss-series-nav-title")!;
      // Text is painted unless its element is clipped to a 1px box (visually hidden).
      const painted: string[] = [];
      const walker = document.createTreeWalker(a, NodeFilter.SHOW_TEXT);
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        const box = n.parentElement!.getBoundingClientRect();
        if (n.textContent!.trim() && (box.width > 1 || box.height > 1)) painted.push(n.textContent!.trim());
      }
      const arrowLine = ink(arrow)[0];
      const titleLines = ink(title);
      const firstLine = titleLines[0];
      const prev = a.classList.contains("moss-series-nav-prev");
      const titleRight = Math.max(...[...titleLines].map((r) => r.right));
      return {
        side: prev ? "prev" : "next",
        painted,
        expected: prev ? [arrow.textContent!.trim(), title.textContent!.trim()] : [title.textContent!.trim(), arrow.textContent!.trim()],
        gap: prev ? firstLine.left - arrowLine.right : arrowLine.left - titleRight,
        centreOff: Math.abs((arrowLine.top + arrowLine.bottom) / 2 - (firstLine.top + firstLine.bottom) / 2),
        // The title's own box, not its glyph range, which sits half the leading lower.
        textBelowLinkTop: title.getBoundingClientRect().top - a.getBoundingClientRect().top,
      };
    });
  });
}

const PAGES = [
  { path: "/course/one/", prev: "Previous", next: "Next" },
  { path: "/course/two/", prev: "Previous", next: "Next" },
  { path: "/course/three/", prev: "Previous", next: "Next" },
  { path: "/ke/one/", prev: "上一个", next: "下一个" },
  { path: "/ke/two/", prev: "上一个", next: "下一个" },
  { path: "/ke/three/", prev: "上一个", next: "下一个" },
];

for (const { path, prev, next } of PAGES) {
  for (const width of [390, 1280]) {
    test(`${path} at ${width}px: each link paints its arrow and title only`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(path);
      const found = await sides(page);
      expect(found.length).toBeGreaterThan(0);
      for (const s of found) {
        expect(s.painted, `${s.side}: painted text`).toEqual(s.expected);
        expect(s.gap, `${s.side}: gap between the arrow and the title`).toBeGreaterThanOrEqual(MIN_GAP);
        expect(s.centreOff, `${s.side}: arrow off the title's first line`).toBeLessThanOrEqual(1);
        expect(s.textBelowLinkTop, `${s.side}: link box above its text`).toBeLessThanOrEqual(1);
      }
      for (const [cls, word] of [["prev", prev], ["next", next]] as const) {
        const link = page.locator(`a.moss-series-nav-${cls}`);
        if ((await link.count()) === 0) continue;
        const title = (await link.locator(".moss-series-nav-title").textContent())!.trim();
        await expect(link, `${cls}: rel`).toHaveAttribute("rel", cls);
        await expect(link, `${cls}: name a screen reader hears`).toHaveAccessibleName(`${word} ${title}`);
      }
    });
  }
}
