/**
 * Render gate: a breadcrumb masthead does not move when nav-split.ts runs.
 *
 * The script runs after the first paint, so on a phone the stylesheet alone
 * lays out the first frame. The two-row layouts with and without the script
 * differ only in where the toggles sit, so nothing below moves. Each case holds the
 * script back until the first frame has painted, then reads the page's
 * layout-shift entries (Chromium reports them) and compares the rows with
 * scripts off against the rows with them on: the header keeps its height and
 * only the toggles change rows.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-first-paint.config.ts
 */

import { test, expect, type Page, newPage } from "../../support/browser-test";
import { holdScript, settle, shifts, themeKeepsNavRightABox } from "./first-paint";

const LONG_TRAIL = "/survey/river/fourth-volume/";
const SHORT_TRAIL = "/notes/rain/";

type Band = [number, number];
const overlaps = (a: Band, b: Band) => a[0] < b[1] && b[0] < a[1];

interface Masthead {
  mode: string;
  header: number;
  trail: Band;
  links: Band;
  link: Band;
  icons: Band;
  linksStart: number;
  iconsStart: number;
}

/** `forceSplit` sets the split in the same task as the reading, so the script cannot undo it in between. */
async function masthead(page: Page, forceSplit = false): Promise<Masthead> {
  return page.evaluate((force) => {
    if (force) document.querySelector(".nav-content")!.setAttribute("data-nav-split", "");
    const box = (s: string) => document.querySelector(s)!.getBoundingClientRect();
    const band = (s: string): [number, number] => [box(s).top, box(s).bottom];
    const content = document.querySelector(".nav-content")!;
    return {
      mode: ["split", "compact", "stack"].find((m) => content.hasAttribute(`data-nav-${m}`)) ?? "none",
      header: box("header").height,
      trail: band(".nav-left"),
      links: band(".nav-links"),
      link: band(".nav-links a"),
      icons: band(".nav-icons"),
      linksStart: box(".nav-links").left,
      iconsStart: box(".nav-icons").left,
    };
  }, forceSplit);
}

// 412 is a phone; 700 is high in the phone band, where a foldable trail fits
// one row; 900 is past it.
const CASES: Array<[string, string, boolean]> = [
  ["a long trail", LONG_TRAIL, false],
  ["a short trail", SHORT_TRAIL, false],
  ["a theme box", LONG_TRAIL, true],
];
for (const [name, path, themeBox] of CASES) {
  for (const width of [412, 700, 900]) {
    test(`the masthead of ${name} does not move when the script runs at ${width}px`, async ({ page, browserName }) => {
      test.skip(browserName !== "chromium", "layout-shift entries are Chromium's");
      await page.setViewportSize({ width, height: 900 });
      if (themeBox) await themeKeepsNavRightABox(page);
      await holdScript(page);
      await page.goto(path, { waitUntil: "load" });
      await settle(page);
      const shift = await shifts(page);
      const where = `${shift.entries.join("\n")}\n${JSON.stringify(await masthead(page))}`;
      expect(shift.paintedFirst, `the first paint must come before the script: ${where}`).toBe(true);
      expect(shift.total, where).toBeLessThan(0.01);
    });
  }
}

test("the scripted split keeps the header the stylesheet laid out, and moves only the toggles", async ({ browser, page }, testInfo) => {
  const off = await newPage(browser, {
    baseURL: testInfo.project.use.baseURL,
    javaScriptEnabled: false,
    viewport: { width: 412, height: 900 },
  });
  await off.goto(LONG_TRAIL, { waitUntil: "load" });
  const plain = await masthead(off);
  await off.close();
  expect(plain.header, `the fixture trail must need two rows at 412px: ${JSON.stringify(plain)}`).toBeGreaterThan(100);
  // With scripts off the toggles wrap to row 2 with the links.
  expect(overlaps(plain.icons, plain.links), JSON.stringify(plain)).toBe(true);

  await page.setViewportSize({ width: 412, height: 900 });
  await page.goto(LONG_TRAIL, { waitUntil: "load" });
  await settle(page);
  const scripted = await masthead(page);
  const where = `plain=${JSON.stringify(plain)} scripted=${JSON.stringify(scripted)}`;
  expect(scripted.mode, where).toBe("split");
  expect(overlaps(scripted.icons, scripted.trail), where).toBe(true);
  expect(Math.abs(scripted.header - plain.header), where).toBeLessThanOrEqual(1);
  expect(Math.abs(scripted.links[0] - plain.links[0]), where).toBeLessThanOrEqual(1);

  // Above the phone band only the attribute's rules apply: the same rows.
  await page.setViewportSize({ width: 900, height: 900 });
  const wide = await masthead(page, true);
  expect(overlaps(wide.icons, wide.trail), JSON.stringify(wide)).toBe(true);
  expect(wide.links[0], JSON.stringify(wide)).toBeGreaterThanOrEqual(wide.icons[1] - 1);
});

// The toggles are not inside .nav-right, so a theme that keeps it a box
// leaves the masthead exactly as moss lays it out.
for (const javaScriptEnabled of [false, true]) {
  test(`a theme that keeps .nav-right a box leaves the masthead as it is, scripts ${javaScriptEnabled ? "on" : "off"}`, async ({ browser }, testInfo) => {
    const read = async (themed: boolean) => {
      const page = await newPage(browser, {
        baseURL: testInfo.project.use.baseURL,
        javaScriptEnabled,
        viewport: { width: 412, height: 900 },
      });
      if (themed) await themeKeepsNavRightABox(page);
      await page.goto(LONG_TRAIL, { waitUntil: "load" });
      if (javaScriptEnabled) await settle(page);
      const m = await masthead(page);
      await page.close();
      return m;
    };
    const themed = await read(true);
    const plain = await read(false);
    const where = JSON.stringify({ themed, plain });
    expect(themed.header, where).toBeCloseTo(plain.header, 0);
    expect(themed.icons, where).toEqual(plain.icons);
    expect(themed.links, where).toEqual(plain.links);
  });
}

// The script's fit test must agree with the layout it predicts: whenever the
// masthead is not split, the toggles share the trail's row, level with a link
// beside them. Fractional widths make a near-fit row wrap at some widths and not others, so every width is tried.
for (const path of [LONG_TRAIL, SHORT_TRAIL]) {
  test(`on ${path} the toggles share the trail's row, level with any link beside them, at every width`, async ({ page }) => {
    await page.setViewportSize({ width: 1000, height: 900 });
    await page.goto(path, { waitUntil: "load" });
    await settle(page);
    const misfits: string[] = [];
    for (let width = 330; width <= 1000; width += 1) {
      await page.setViewportSize({ width, height: 900 });
      await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
      const m = await masthead(page);
      if (!overlaps(m.icons, m.trail)) misfits.push(`${width}px ${m.mode}`);
      const dy = (m.link[0] + m.link[1] - m.icons[0] - m.icons[1]) / 2;
      if (overlaps(m.link, m.icons) && Math.abs(dy) > 1.5) misfits.push(`${width}px link ${dy.toFixed(1)}`);
    }
    expect(misfits).toEqual([]);
  });
}
