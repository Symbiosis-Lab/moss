/**
 * Render gate: the split masthead keeps the toggles beside the site name.
 *
 * When the nav wraps (`data-nav-split`), row 1 is the site name or the
 * breadcrumb trail with the toggle cluster at its end edge, and row 2 holds
 * the links alone; a trail folds into the room row 1 leaves it, and must
 * settle on one layout. In the hamburger range the links are an overlay, and
 * the name, the menu button and the toggles share one line, the name wrapping
 * to at most two lines before the controls go below it. Row placement depends
 * on measured widths, so it is asserted in a real engine at several widths.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-split-order.config.ts
 */

import { test, expect, type Page } from "@playwright/test";

interface Band {
  top: number;
  bottom: number;
}

interface Row {
  split: boolean;
  name: Band;
  links: Band;
  icons: Band;
}

const overlaps = (a: Band, b: Band) => a.top < b.bottom && b.top < a.bottom;

async function measure(page: Page): Promise<Row> {
  return page.evaluate(() => {
    const content = document.querySelector(".nav-content") as HTMLElement;
    const name = document.querySelector(".nav-left") as HTMLElement;
    const links = document.querySelector(".nav-links") as HTMLElement;
    const icons = document.querySelector(".nav-icons") as HTMLElement;
    const r = (el: Element) => {
      const b = el.getBoundingClientRect();
      return { top: b.top, bottom: b.bottom };
    };
    return {
      split: content.hasAttribute("data-nav-split"),
      name: r(name),
      links: r(links),
      icons: r(icons),
    };
  });
}

for (const width of [360, 500, 768, 1280]) {
  test(`toggles stay on the site name's row at ${width}px`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/", { waitUntil: "load" });
    await page.evaluate(() => document.fonts.ready);
    const row = await measure(page);
    await page.screenshot({
      path: testInfo.outputPath(`nav-${width}.png`),
      clip: { x: 0, y: 0, width, height: 160 },
    });

    const where =
      `split=${row.split} name=[${row.name.top},${row.name.bottom}] ` +
      `links=[${row.links.top},${row.links.bottom}] icons=[${row.icons.top},${row.icons.bottom}]`;
    expect(overlaps(row.icons, row.name), where).toBe(true);
    if (row.split) {
      // Split: the links take a row of their own, wholly below the toggles.
      expect(row.icons.bottom, where).toBeLessThanOrEqual(row.links.top);
    } else {
      expect(overlaps(row.icons, row.links), where).toBe(true);
    }
  });
}

async function setName(page: Page, text: string): Promise<void> {
  await page.evaluate((t) => {
    document.querySelector(".site-name")!.textContent = t;
    window.dispatchEvent(new Event("resize"));
  }, text);
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
}

test("at 320px a name of up to two lines keeps the controls beside it", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 900 });
  await page.goto("/", { waitUntil: "load" });
  await page.evaluate(() => document.fonts.ready);
  expect(await page.locator(".mobile-menu-button").isVisible(), "320px must be in the hamburger range").toBe(true);
  await setName(page, "Harbour Field Notes");
  const row = await measure(page);
  const stack = await page.evaluate(() => document.querySelector(".nav-content")!.hasAttribute("data-nav-stack"));
  expect(stack).toBe(false);
  expect(overlaps(row.icons, row.name), `name=[${row.name.top},${row.name.bottom}] icons=[${row.icons.top},${row.icons.bottom}]`).toBe(true);
});

test("at 320px a name that would take three lines beside the controls sends them below", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 900 });
  await page.goto("/", { waitUntil: "load" });
  await page.evaluate(() => document.fonts.ready);
  await setName(page, "The Harbour Society Field Notes and Recordings Archive");
  const row = await measure(page);
  const state = await page.evaluate(() => {
    const left = document.querySelector(".nav-left") as HTMLElement;
    return {
      stack: document.querySelector(".nav-content")!.hasAttribute("data-nav-stack"),
      clipped: left.scrollWidth > left.clientWidth,
      overflowX: document.documentElement.scrollWidth > window.innerWidth,
    };
  });
  expect(state.stack).toBe(true);
  expect(row.icons.top, `name=[${row.name.top},${row.name.bottom}] icons=[${row.icons.top},${row.icons.bottom}]`).toBeGreaterThanOrEqual(row.name.bottom - 1);
  expect(state.clipped, "the name is not clipped").toBe(false);
  expect(state.overflowX, "no sideways scroll").toBe(false);
});

test("a logo too wide to sit beside the controls is not clipped at 320px", async ({ page }) => {
  // An image cannot wrap: the controls must go below it rather than cut it.
  await page.setViewportSize({ width: 320, height: 900 });
  await page.goto("/", { waitUntil: "load" });
  const clip = await page.evaluate(() => {
    const name = document.querySelector(".site-name") as HTMLElement;
    name.innerHTML =
      '<img class="site-logo" alt="" style="inline-size: 220px; block-size: 40px" ' +
      'src="data:image/svg+xml,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22/%3E">';
    const left = document.querySelector(".nav-left") as HTMLElement;
    return { scroll: left.scrollWidth, client: left.clientWidth };
  });
  expect(clip.scroll, JSON.stringify(clip)).toBeLessThanOrEqual(clip.client);
});

test("a name built from words never breaks inside a word at 320px", async ({ page }) => {
  // A theme wordmark: the name's words as flex items in a `.nav-left` that
  // does not grow, so the name is exactly as wide as its min-content.
  await page.setViewportSize({ width: 320, height: 900 });
  await page.goto("/", { waitUntil: "load" });
  await page.addStyleTag({ content: ".nav-left { flex: 0 0 auto; } .site-name { display: inline-flex; }" });
  const words = await page.evaluate(() => {
    const name = document.querySelector(".site-name") as HTMLElement;
    name.innerHTML = "<span>Brightwater</span><span>Almanac</span>";
    const line = parseFloat(getComputedStyle(name).lineHeight) || parseFloat(getComputedStyle(name).fontSize) * 1.3;
    return [...name.children].map((w) => ({ height: w.getBoundingClientRect().height, line }));
  });
  for (const w of words) expect(w.height, JSON.stringify(words)).toBeLessThan(w.line * 1.6);
});

test("with no site name the hamburger row still ends at the toggles at 320px", async ({ page }) => {
  // `[site].header = "nav"` emits `.nav-content` with `.nav-right` alone;
  // removing `.nav-left` here gives the same markup on this fixture.
  await page.setViewportSize({ width: 320, height: 900 });
  await page.goto("/", { waitUntil: "load" });
  const edges = await page.evaluate(() => {
    document.querySelector(".nav-left")?.remove();
    const content = document.querySelector(".nav-content") as HTMLElement;
    const style = getComputedStyle(content);
    return {
      contentEnd: content.getBoundingClientRect().right - parseFloat(style.paddingRight),
      iconsEnd: (document.querySelector(".nav-icons") as HTMLElement).getBoundingClientRect().right,
    };
  });
  expect(Math.abs(edges.contentEnd - edges.iconsEnd), JSON.stringify(edges)).toBeLessThanOrEqual(4);
});

test("a theme that keeps .nav-right a box keeps the links before the toggles", async ({ page }) => {
  // An unlayered rule outranks every moss layer, as a theme's does.
  await page.setViewportSize({ width: 768, height: 900 });
  await page.goto("/", { waitUntil: "load" });
  await page.addStyleTag({ content: ".nav-right { display: flex; }" });
  await page.setViewportSize({ width: 767, height: 900 });
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  const order = await page.evaluate(() => {
    const box = (s: string) => document.querySelector(s)!.getBoundingClientRect();
    return { linksLeft: box(".nav-links").left, iconsLeft: box(".nav-icons").left };
  });
  expect(order.iconsLeft, JSON.stringify(order)).toBeGreaterThan(order.linksLeft);
});

const TRAIL_PAGE = "/archive/autumn/rain/";

test("a breadcrumb page splits like a plain one: trail and toggles, then the links", async ({ page }) => {
  await page.setViewportSize({ width: 500, height: 900 });
  await page.goto(TRAIL_PAGE, { waitUntil: "load" });
  await page.evaluate(() => document.fonts.ready);
  const row = await measure(page);
  expect(await page.locator(".nav-left .breadcrumb-segment").count(), "fixture page must carry a trail").toBeGreaterThan(0);
  const where = `split=${row.split} trail=[${row.name.top},${row.name.bottom}] links=[${row.links.top},${row.links.bottom}] icons=[${row.icons.top},${row.icons.bottom}]`;
  expect(row.split, where).toBe(true);
  expect(overlaps(row.icons, row.name), where).toBe(true);
  expect(row.icons.bottom, where).toBeLessThanOrEqual(row.links.top);
});

test("a breadcrumb page settles on one layout and one fold at every width", async ({ page }) => {
  await page.goto(TRAIL_PAGE, { waitUntil: "load" });
  await page.evaluate(() => document.fonts.ready);
  for (let width = 340; width <= 1400; width += 30) {
    await page.setViewportSize({ width, height: 900 });
    const states = await page.evaluate(async () => {
      const content = document.querySelector(".nav-content") as HTMLElement;
      const seen: string[] = [];
      for (let i = 0; i < 16; i++) {
        await new Promise((r) => requestAnimationFrame(r));
        const folded = document.querySelectorAll(".nav-left [data-trail-crumb][hidden]").length;
        seen.push(`${content.hasAttribute("data-nav-split")}/${folded}`);
      }
      return seen.slice(6);
    });
    expect(new Set(states).size, `${width}px split/folded per frame: ${states.join(",")}`).toBe(1);
  }
});

test("at 320px a breadcrumb trail stays beside the controls while its folded form fits", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 900 });
  await page.goto(TRAIL_PAGE, { waitUntil: "load" });
  await page.evaluate(() => document.fonts.ready);
  await setName(page, "Harbour");
  const row = await measure(page);
  const state = await page.evaluate(() => {
    const left = document.querySelector(".nav-left") as HTMLElement;
    const end = left.getBoundingClientRect().right;
    return {
      overflows: [...left.children].some((el) => !(el as HTMLElement).hidden && el.getBoundingClientRect().right > end + 1),
      overflowX: document.documentElement.scrollWidth > window.innerWidth,
    };
  });
  const where = `split=${row.split} trail=[${row.name.top},${row.name.bottom}] icons=[${row.icons.top},${row.icons.bottom}]`;
  expect(overlaps(row.icons, row.name), where).toBe(true);
  expect(state.overflows, "the folded trail fits its box").toBe(false);
  expect(state.overflowX, "no sideways scroll").toBe(false);
});

test("at 320px a trail too long to sit beside the controls puts them below", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 900 });
  await page.goto(TRAIL_PAGE, { waitUntil: "load" });
  await page.evaluate(() => document.fonts.ready);
  const row = await measure(page);
  const where = `split=${row.split} trail=[${row.name.top},${row.name.bottom}] icons=[${row.icons.top},${row.icons.bottom}]`;
  expect(row.split, where).toBe(false);
  expect(row.icons.top, where).toBeGreaterThanOrEqual(row.name.bottom - 1);
});

test("the split engages on a narrow masthead", async ({ page }) => {
  // Precondition: the fixture must actually exercise the split, or the
  // row assertions above would pass on a single-row nav.
  await page.setViewportSize({ width: 360, height: 900 });
  await page.goto("/", { waitUntil: "load" });
  const row = await measure(page);
  expect(row.split, "fixture must wrap the masthead at 360px").toBe(true);
});
