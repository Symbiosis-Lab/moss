/**
 * `style:stream` draws each post whole as an <article>, inside the page's own
 * <article>. The page article's measure and end margin must not reach the
 * posts: the space between two posts is the hairline's padding on each side,
 * and the listing ends on the last post's date.
 */
import { test, expect, type Page } from "../../support/browser-test";

const PHONE = { width: 390, height: 844 };
const WIDE = { width: 1440, height: 900 };

for (const [name, viewport] of [["phone", PHONE], ["wide", WIDE]] as const) {
  test.describe(`stream listing at ${name} width`, () => {
    test.beforeEach(async ({ page }) => {
      await page.setViewportSize(viewport);
    });

    test("the More link under a stream sits at its own margin below the last date", async ({ page }) => {
      await page.goto("/");
      const gap = await page.evaluate(() => {
        const date = document.querySelector(".moss-stream-item:last-child .moss-stream-date")!.getBoundingClientRect();
        const more = document.querySelector(".moss-stream + .moss-embed-more")!;
        return { space: more.getBoundingClientRect().top - date.bottom, margin: parseFloat(getComputedStyle(more).marginTop) };
      });
      expect(Math.abs(gap.space - gap.margin)).toBeLessThanOrEqual(1);
    });

    test("the hairline between two posts has the same space on each side", async ({ page }) => {
      await page.goto("/notes/");
      const sides = await page.evaluate(() => {
        const [first, second] = document.querySelectorAll(".moss-stream-item");
        const hairline = second.getBoundingClientRect().top;
        const above = hairline - first.querySelector(".moss-stream-date")!.getBoundingClientRect().bottom;
        const below = second.querySelector(".moss-stream-body")!.getBoundingClientRect().top - hairline - 1;
        return { above, below };
      });
      expect(sides.below).toBeGreaterThan(0);
      expect(Math.abs(sides.above - sides.below)).toBeLessThanOrEqual(1);
    });

    test("a post is as wide as the stream it sits in", async ({ page }) => {
      await page.goto("/notes/");
      const widths = await page.evaluate(() => ({
        stream: document.querySelector(".moss-stream")!.getBoundingClientRect().width,
        item: document.querySelector(".moss-stream-item")!.getBoundingClientRect().width,
      }));
      expect(Math.abs(widths.stream - widths.item)).toBeLessThanOrEqual(1);
    });
  });
}

/**
 * A thread: on a page the earlier parts sit above the title and the later
 * ones after the body; every part's dot sits on one straight line, level with
 * its first line of text, and the line stops at the last part. In a listing
 * the fold's dashed line runs on the same rail.
 */
const railOf = (page: Page, selector: string) =>
  page.evaluate((sel) =>
    [...document.querySelectorAll<HTMLElement>(sel)].map((item) => {
      const box = item.getBoundingClientRect();
      // Dot and line hang off the part's first child; the part is their containing block.
      const dot = getComputedStyle(item.firstElementChild!, "::before");
      const line = getComputedStyle(item.firstElementChild!, "::after");
      const text = document.createTreeWalker(item, NodeFilter.SHOW_TEXT, {
        acceptNode: (n) => (n.textContent!.trim() ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP),
      }).nextNode()!;
      const range = document.createRange();
      range.selectNodeContents(text);
      const first = range.getClientRects()[0];
      // A pseudo-element has no box of its own: its painted edge is the offset plus its margin.
      const x = (s: CSSStyleDeclaration) => parseFloat(s.left) + parseFloat(s.marginLeft);
      const y = (s: CSSStyleDeclaration) => parseFloat(s.top) + parseFloat(s.marginTop);
      const dotLeft = box.left + x(dot);
      const dotTop = box.top + y(dot);
      const drawn = line.content !== "none" && line.display !== "none";
      return {
        dotX: dotLeft + parseFloat(dot.width) / 2,
        dotY: dotTop + parseFloat(dot.height) / 2,
        dotRight: dotLeft + parseFloat(dot.width),
        dotBottom: dotTop + parseFloat(dot.height),
        dotColor: dot.backgroundColor,
        lineX: drawn ? box.left + x(line) + parseFloat(line.width) / 2 : null,
        lineTop: drawn ? box.top + y(line) : null,
        lineBottom: drawn ? box.bottom - parseFloat(line.bottom) - parseFloat(line.marginBottom) : null,
        lineColor: drawn ? line.backgroundColor : null,
        first: { left: first.left, top: first.top, bottom: first.bottom },
      };
    }), selector);

const pageBackground = (page: Page) =>
  page.evaluate(() => {
    for (const el of [document.body, document.documentElement]) {
      const bg = getComputedStyle(el).backgroundColor;
      if (bg !== "rgba(0, 0, 0, 0)" && bg !== "transparent") return bg;
    }
    return "rgb(255, 255, 255)";
  });

type Rail = Awaited<ReturnType<typeof railOf>>[number];

/** WCAG contrast ratio of two `rgb()`/`rgba()` colours (alpha ignored: the rail is opaque). */
function contrast(a: string, b: string): number {
  const lum = (c: string) => {
    const [r, g, bl] = c.match(/[\d.]+/g)!.slice(0, 3).map((v) => {
      const s = Number(v) / 255;
      return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** One unbroken rail: dots on one vertical, each centred on its first line, and the parts' lines together running from the first dot to the last without a gap, starting at neither earlier nor ending later. */
function expectOnOneRail(rail: Rail[], background: string) {
  expect(rail.length).toBeGreaterThan(0);
  rail.forEach((part) => {
    expect(Math.abs(part.dotX - rail[0].dotX), "every dot on one vertical").toBeLessThanOrEqual(0.5);
    expect(Math.abs(part.dotY - (part.first.top + part.first.bottom) / 2), "the dot is centred on its first line").toBeLessThanOrEqual(1);
    expect(part.first.left - part.dotRight, "the text clears the dot").toBeGreaterThan(4);
    expect(contrast(part.dotColor, background), "the dot stands out from the page").toBeGreaterThanOrEqual(3);
    if (part.lineX === null || part.lineBottom! - part.lineTop! <= 0.5) return;
    expect(Math.abs(part.lineX - part.dotX), "the line runs through the dots").toBeLessThanOrEqual(0.5);
    expect(contrast(part.lineColor!, background), "the line stands out from the page").toBeGreaterThanOrEqual(3);
  });
  const first = rail[0].dotY;
  const last = rail[rail.length - 1].dotY;
  const runs = rail
    .filter((p) => p.lineX !== null && p.lineBottom! - p.lineTop! > 0.5)
    .map((p) => [p.lineTop!, p.lineBottom!] as const)
    .sort((x, y) => x[0] - y[0]);
  if (rail.length === 1) {
    expect(runs, "a lone part draws no line").toHaveLength(0);
    return;
  }
  expect(runs.length, "a line joins the parts").toBeGreaterThan(0);
  expect(runs[0][0], "the line starts at the first dot, not above it").toBeGreaterThanOrEqual(first - 0.5);
  expect(runs[0][0], "the line leaves from the first dot").toBeLessThanOrEqual(first + 0.5);
  let reach = runs[0][1];
  for (const [top, bottom] of runs.slice(1)) {
    expect(top, "the line is unbroken from dot to dot").toBeLessThanOrEqual(reach + 0.5);
    reach = Math.max(reach, bottom);
  }
  expect(reach, "the line reaches the last dot").toBeGreaterThanOrEqual(last - 0.5);
  expect(reach, "the line stops at the last part").toBeLessThanOrEqual(last + 0.5);
}

for (const scheme of ["light", "dark"] as const) {
  for (const [name, viewport] of [["phone", PHONE], ["wide", { width: 1280, height: 900 }]] as const) {
    test.describe(`a thread at ${name} width, ${scheme}`, () => {
      test.use({ colorScheme: scheme });
      test.beforeEach(async ({ page }) => {
        await page.setViewportSize(viewport);
      });

      test("a part's page draws the earlier parts above its title and the later ones after its body", async ({ page }) => {
        await page.goto("/threads/2026-02-03-0800/");
        const layout = await page.evaluate(() => {
          const rect = (sel: string) => document.querySelector(sel)!.getBoundingClientRect();
          const before = document.querySelector('.moss-thread[data-part="before"]')!;
          const after = document.querySelector('.moss-thread[data-part="after"]')!;
          const body = [...document.querySelectorAll("p")].find((p) => p.textContent!.includes("The third crossing ran"))!;
          return {
            before: before.getBoundingClientRect(),
            after: after.getBoundingClientRect(),
            title: rect("h1"),
            body: body.getBoundingClientRect(),
            beforeMargin: parseFloat(getComputedStyle(before).marginBlockEnd),
            afterMargin: parseFloat(getComputedStyle(after).marginBlockStart),
            overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
          };
        });
        expect(layout.before.bottom).toBeLessThanOrEqual(layout.title.top);
        expect(layout.title.top - layout.before.bottom).toBeGreaterThanOrEqual(layout.beforeMargin - 1);
        expect(layout.title.top - layout.before.bottom, "space between the earlier parts and the title").toBeGreaterThan(8);
        expect(layout.after.top - layout.body.bottom).toBeGreaterThanOrEqual(layout.afterMargin - 1);
        expect(layout.after.top - layout.body.bottom, "space between the body and the later parts").toBeGreaterThan(8);
        expect(layout.overflow).toBeLessThanOrEqual(0);
      });

      test("each part above and below the page sits on one rail, and the line stops at the last part", async ({ page }) => {
        await page.goto("/threads/2026-02-03-0800/");
        const background = await pageBackground(page);
        const before = await railOf(page, '.moss-thread[data-part="before"] .moss-stream-item');
        const after = await railOf(page, '.moss-thread[data-part="after"] .moss-stream-item');
        expect(before.length).toBe(2);
        expect(after.length).toBe(2);
        expectOnOneRail(before, background);
        expectOnOneRail(after, background);
        expect(Math.abs(before[0].dotX - after[0].dotX), "one rail above and below").toBeLessThanOrEqual(0.5);
      });

      test("a titled part drawn in a thread keeps its dot on its first line", async ({ page }) => {
        await page.goto("/threads/2026-02-01-0800/");
        const after = await railOf(page, '.moss-thread[data-part="after"] .moss-stream-item');
        expect(after.length).toBe(4);
        expectOnOneRail(after, await pageBackground(page));
      });

      test("a listing folds the thread's middle onto the same rail", async ({ page }) => {
        await page.goto("/threads/");
        const background = await pageBackground(page);
        expect(await page.locator(".moss-stream-thread").count()).toBe(1);
        const parts = await railOf(page, ".moss-stream-thread > .moss-stream-item");
        expect(parts.length).toBe(3);
        expectOnOneRail(parts.slice(1), background);
        const fold = await page.evaluate(() => {
          const more = document.querySelector<HTMLElement>(".moss-stream-thread-more")!;
          const box = more.getBoundingClientRect();
          const line = getComputedStyle(more, "::before");
          return {
            x: box.left + parseFloat(line.left) + parseFloat(line.marginLeft),
            top: box.top + parseFloat(line.top) + parseFloat(line.marginTop),
            bottom: box.bottom - parseFloat(line.bottom) - parseFloat(line.marginBottom),
            style: line.borderInlineStartStyle,
            color: line.borderInlineStartColor,
          };
        });
        expect(fold.style).toBe("dashed");
        expect(Math.abs(fold.x + 0.5 - parts[0].dotX), "the fold's line runs on the rail").toBeLessThanOrEqual(0.5);
        expect(contrast(fold.color, background), "the fold's line stands out from the page").toBeGreaterThanOrEqual(3);
        // The first part's solid line meets the fold's dashed one, which carries on to the next dot.
        expect(parts[0].lineX, "a line leaves the first part").not.toBeNull();
        expect(parts[0].lineBottom!, "the first part's line meets the fold").toBeGreaterThanOrEqual(fold.top - 0.5);
        expect(parts[0].lineBottom!, "solid and dashed do not overlap").toBeLessThanOrEqual(fold.top + 0.5);
        expect(fold.bottom, "the fold's line reaches the next dot").toBeGreaterThanOrEqual(parts[1].dotY - 0.5);
        expect(Math.abs(parts[0].dotY - (parts[0].first.top + parts[0].first.bottom) / 2), "the first dot is centred on its first line").toBeLessThanOrEqual(1);
        expect(contrast(parts[0].dotColor, background)).toBeGreaterThanOrEqual(3);
        expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBeLessThanOrEqual(0);
      });
    });
  }
}
