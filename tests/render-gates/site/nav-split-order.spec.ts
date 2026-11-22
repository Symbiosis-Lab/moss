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

import { test, expect, type Page } from "../../support/browser-test";
import { holdScript, settle, shifts } from "./first-paint";
import { gatePort } from "../../../playwright/gate-ports";

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
    // Links beside the toggles, or on a row of their own wholly below them.
    if (!overlaps(row.icons, row.links)) expect(row.icons.bottom, where).toBeLessThanOrEqual(row.links.top);
  });
}

// The fixture's name is long enough to wrap beside the toggles on a phone.
for (const width of [412, 700, 900]) {
  test(`the long-name masthead does not move when the script runs at ${width}px`, async ({ page, browserName }) => {
    test.skip(browserName !== "chromium", "layout-shift entries are Chromium's");
    await page.setViewportSize({ width, height: 900 });
    await holdScript(page);
    await page.goto("/", { waitUntil: "load" });
    await settle(page);
    const shift = await shifts(page);
    const where = `${shift.entries.join("\n")}\n${JSON.stringify(await measure(page))}`;
    expect(shift.paintedFirst, `the first paint must come before the script: ${where}`).toBe(true);
    expect(shift.total, where).toBeLessThan(0.01);
  });
}

async function setName(page: Page, text: string): Promise<void> {
  await page.evaluate((t) => {
    document.querySelector(".site-name")!.textContent = t;
    window.dispatchEvent(new Event("resize"));
  }, text);
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
}

// The name starts on the start edge; the links that drop below it end on the end edge.
for (const width of [412, 900]) {
  test(`the long name starts on the masthead's start edge at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/", { waitUntil: "load" });
    await settle(page);
    const edges = await page.evaluate(() => {
      const content = document.querySelector(".nav-content") as HTMLElement;
      const start = content.getBoundingClientRect().left + parseFloat(getComputedStyle(content).paddingLeft);
      const end = content.getBoundingClientRect().right - parseFloat(getComputedStyle(content).paddingRight);
      const name = document.querySelector(".site-name")!.getClientRects()[0].left;
      const link = document.querySelector(".nav-links")!.getBoundingClientRect().right;
      return { start, end, name, link };
    });
    expect(edges.name, JSON.stringify(edges)).toBeCloseTo(edges.start, 0);
    expect(Math.abs(edges.end - edges.link), JSON.stringify(edges)).toBeLessThanOrEqual(1);
  });
}

// The strut holds row 1 at the toggles' size token while the name grows in
// rem: at large text the toggles still sit level with the name's first line.
for (const [label, rootSize] of [["200% text", "200%"], ["a 20px default font", "20px"]]) {
  for (const width of [412, 900]) {
    test(`with ${label} the toggles sit level with the name's first line at ${width}px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.addInitScript((size) => {
        document.addEventListener("DOMContentLoaded", () => { document.documentElement.style.fontSize = size; });
      }, rootSize);
      await page.goto("/", { waitUntil: "load" });
      await settle(page);
      const m = await page.evaluate(() => {
        const icons = document.querySelector(".nav-icons")!.getBoundingClientRect();
        const line = document.querySelector(".site-name")!.getClientRects()[0];
        const links = [...document.querySelectorAll(".nav-links a")].map((a) => a.getBoundingClientRect());
        return {
          iconsMid: (icons.top + icons.bottom) / 2,
          lineTop: line.top,
          lineBottom: line.bottom,
          linksUnderToggles: links.some((r) => r.right > icons.left && r.left < icons.right && r.bottom > icons.top && r.top < icons.bottom),
        };
      });
      const where = JSON.stringify(m);
      expect(m.iconsMid, where).toBeGreaterThanOrEqual(m.lineTop);
      expect(m.iconsMid, where).toBeLessThanOrEqual(m.lineBottom);
      expect(m.linksUnderToggles, where).toBe(false);
    });
  }
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
  // A plain name splits by layout, not by the attribute: the links leave the toggles' row.
  expect(row.links.top, `fixture must wrap the masthead at 360px: ${JSON.stringify(row)}`).toBeGreaterThanOrEqual(row.icons.bottom - 1);
});

// The toggles come first in the markup, so they must be first to the eye too:
// row 1 at the inline end, on the plain home and on a trail page, with the
// masthead in either mode and either direction.
const SITES: Array<[string, string]> = [
  ["brand", ""],
  ["nav", `http://localhost:${gatePort("nav-split-order:nav")}`],
];
for (const [mode, origin] of SITES) {
  for (const path of ["/", TRAIL_PAGE]) {
    for (const width of [412, 900]) {
      test(`${mode} ${path} at ${width}px: focus runs skip link, toggles, name, links`, async ({ page, browserName }) => {
        test.skip(browserName !== "chromium", "WebKit's Tab skips links unless the system setting is on");
        await page.setViewportSize({ width, height: 900 });
        await page.goto(`${origin}${path}`, { waitUntil: "load" });
        await settle(page);
        const groups: string[] = [];
        for (let i = 0; i < 16 && groups.at(-1) !== "links"; i += 1) {
          await page.keyboard.press("Tab");
          const group = await page.evaluate(() => {
            const el = document.activeElement;
            if (!el || el === document.body) return "none";
            if (el.matches(".moss-skip-link")) return "skip";
            for (const [sel, name] of [[".nav-icons", "toggles"], [".nav-left", "name"], [".mobile-menu-button", "button"], [".nav-links", "links"]]) {
              if (el.closest(sel)) return name;
            }
            return el.className || el.nodeName;
          });
          if (groups.at(-1) !== group) groups.push(group);
        }
        expect(groups).toEqual(mode === "nav" ? ["skip", "toggles", "links"] : ["skip", "toggles", "name", "links"]);
      });

      for (const dir of ["ltr", "rtl"]) {
        test(`${mode} ${path} at ${width}px ${dir}: the toggles hold row 1's inline end`, async ({ page }) => {
          await page.setViewportSize({ width, height: 900 });
          if (dir === "rtl") await page.addInitScript(() => document.documentElement.setAttribute("dir", "rtl"));
          await page.goto(`${origin}${path}`, { waitUntil: "load" });
          await settle(page);
          const m = await page.evaluate(() => {
            const box = (s: string) => document.querySelector(s)?.getBoundingClientRect();
            const content = box(".nav-content")!;
            const icons = box(".nav-icons")!;
            const first = box(".nav-left") ?? box(".nav-links")!;
            const style = getComputedStyle(document.querySelector(".nav-content")!);
            const start = content.left + parseFloat(style.paddingLeft);
            const end = content.right - parseFloat(style.paddingRight);
            return { iconsTop: icons.top, firstTop: first.top, firstBottom: first.bottom, edge: document.documentElement.dir === "rtl" ? icons.left - start : end - icons.right };
          });
          expect(m.iconsTop, JSON.stringify(m)).toBeLessThan(m.firstBottom);
          expect(Math.abs(m.edge), JSON.stringify(m)).toBeLessThanOrEqual(1);
        });
      }
    }
  }
}

/** The links' end and first-link start, and the toggles' start, as offsets from the start edge in the page's direction. */
async function lineEnds(page: Page) {
  return page.evaluate(() => {
    const rtl = getComputedStyle(document.documentElement).direction === "rtl";
    const content = document.querySelector(".nav-content") as HTMLElement;
    const c = content.getBoundingClientRect();
    const style = getComputedStyle(content);
    const start = rtl ? c.right - parseFloat(style.paddingRight) : c.left + parseFloat(style.paddingLeft);
    const at = (x: number) => Math.abs(x - start);
    const width = c.width - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
    const links = [...document.querySelectorAll(".nav-links a")].map((a) => a.getBoundingClientRect());
    const icons = document.querySelector(".nav-icons")!.getBoundingClientRect();
    return {
      oneRow: links.every((l) => l.top < icons.bottom && icons.top < l.bottom),
      firstLinkStart: at(rtl ? links[0].right : links[0].left),
      linksEnd: Math.max(...links.map((l) => at(rtl ? l.left : l.right))),
      iconsStart: at(rtl ? icons.right : icons.left),
      width,
    };
  });
}

// A short name keeps the one-row masthead: name at the start, links ending one
// group gap before the toggles at the end.
const SHORT = `http://localhost:${gatePort("nav-split-order:short")}/`;
for (const dir of ["ltr", "rtl"]) {
  for (const width of [412, 700, 900, 1280]) {
    test(`a short name's links end a group gap before the toggles at ${width}px, ${dir}`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      if (dir === "rtl") await page.addInitScript(() => document.documentElement.setAttribute("dir", "rtl"));
      await page.goto(SHORT, { waitUntil: "load" });
      await settle(page);
      const m = await lineEnds(page);
      const groupGap = Math.min(32, Math.max(24, width * 0.04));
      const where = JSON.stringify({ ...m, groupGap });
      expect(m.oneRow, where).toBe(true);
      expect(Math.abs(m.iconsStart - m.linksEnd - groupGap), where).toBeLessThanOrEqual(1);
    });
  }
}

// Links too many to share a short name's row end the next row at the end edge, under the toggles.
for (const dir of ["ltr", "rtl"]) {
  test(`a short name's links that leave its row end at the end edge at 900px, ${dir}`, async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 900 });
    if (dir === "rtl") await page.addInitScript(() => document.documentElement.setAttribute("dir", "rtl"));
    await page.goto("/", { waitUntil: "load" });
    await settle(page);
    await setName(page, "Garden");
    const m = await lineEnds(page);
    expect(m.oneRow, `the fixture's links must leave the name's row: ${JSON.stringify(m)}`).toBe(false);
    expect(Math.abs(m.width - m.linksEnd), JSON.stringify(m)).toBeLessThanOrEqual(1);
  });
}

// The name gap is the narrow one: this name and three links fit one row at 900px
// with 24px after the name, and not with the 32px group gap there.
test("the long name and three links share one row at 900px", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 900 });
  await page.goto("/", { waitUntil: "load" });
  await settle(page);
  await page.evaluate(() => document.querySelectorAll(".nav-links > :nth-child(n + 4)").forEach((el) => el.remove()));
  const m = await lineEnds(page);
  expect(m.oneRow, JSON.stringify(m)).toBe(true);
});

/** A wrapped name's line pitch, the name's and the nav text's line heights, and how far the first link's text sits below the top of the name's last line. */
async function nameLines(page: Page) {
  return page.evaluate(() => {
    const name = document.querySelector(".site-name") as HTMLElement;
    const range = document.createRange();
    range.selectNodeContents(name);
    const tops = [...new Set([...range.getClientRects()].map((r) => Math.round(r.top * 10) / 10))].sort((a, b) => a - b);
    const link = document.createRange();
    link.selectNodeContents(document.querySelector(".nav-links a")!);
    return {
      lines: tops.length,
      pitch: tops[1] - tops[0],
      nameLeading: parseFloat(getComputedStyle(name).lineHeight),
      navLeading: parseFloat(getComputedStyle(document.querySelector(".nav-content")!).lineHeight),
      linkBelowName: link.getClientRects()[0].top - tops[tops.length - 1],
    };
  });
}

// A wrapped name keeps its own line pitch. It shares the nav text's line boxes,
// so the pitch is never below the nav text's line height: where the name's font
// is the smaller (the default theme on a phone), that is the cap.
for (const [width, name] of [
  [412, "Nav Split Order Gate Site With A Long Name"],
  [900, "Nav Split Order Gate Site With A Long Name That Runs On Further Still Across The Whole Row"],
] as const) {
  test(`a two-line name keeps its line pitch at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/", { waitUntil: "load" });
    await settle(page);
    await setName(page, name);
    const m = await nameLines(page);
    const where = JSON.stringify(m);
    expect(m.lines, where).toBe(2);
    expect(Math.abs(m.pitch - Math.max(m.nameLeading, m.navLeading)), where).toBeLessThanOrEqual(1);
  });
}

// Links on their own row start where the name's last line box ends, which the
// strut at the name's end holds at the toggles' height and the drop clearance
// below that line's top. The recorded distances are these; the engines differ by a pixel or so.
for (const [width, gaps] of [[412, { chromium: 51, webkit: 50.3 }], [900, { chromium: 54.7, webkit: 56 }]] as const) {
  test(`links on their own row sit their recorded distance below the name's last line at ${width}px`, async ({ page, browserName }) => {
    const gap = gaps[browserName as keyof typeof gaps];
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/", { waitUntil: "load" });
    await settle(page);
    const rows = await lineEnds(page);
    expect(rows.oneRow, `the fixture's links must leave the name's row: ${JSON.stringify(rows)}`).toBe(false);
    const m = await nameLines(page);
    expect(Math.abs(m.linkBelowName - gap), JSON.stringify(m)).toBeLessThanOrEqual(1);
  });
}
