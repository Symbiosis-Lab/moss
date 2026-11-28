/**
 * Render gate: a masthead whose logo, links and toggles overflow one row
 * keeps the toggles beside the logo and moves the links to row 2, and a
 * masthead that fits keeps its full gaps.
 *
 * The lockup is 44px tall and sized so the default gaps overflow the 390px
 * phone width by a few pixels. Whether the row fits is a question of measured
 * widths, so only a layout engine can answer it.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-one-row.config.ts
 */

import { test, expect, type Page } from "../../support/browser-test";
import { holdScript, settle, shifts } from "./first-paint";

// A built theme stylesheet would make the masthead the theme's flex row, so the 44px lockup the float is measured against rides in moss's own sheet.
test.beforeEach(async ({ page }) => {
  await page.route(/\.css(\?|$)/, async (route) => {
    const response = await route.fetch();
    await route.fulfill({ response, body: `${await response.text()}\n.site-logo { height: 44px; }\n` });
  });
});

async function masthead(page: Page) {
  return page.evaluate(() => {
    const content = document.querySelector(".nav-content") as HTMLElement;
    const logo = document.querySelector(".site-name .site-logo") as HTMLElement;
    const icons = document.querySelector(".nav-icons") as HTMLElement;
    const lastLink = [...document.querySelectorAll<HTMLElement>(".nav-links a")].pop()!;
    const header = document.querySelector("header") as HTMLElement;
    return {
      logoRight: logo.getBoundingClientRect().right,
      lastLinkRight: lastLink.getBoundingClientRect().right,
      iconsLeft: icons.getBoundingClientRect().left,
      logoTop: logo.getBoundingClientRect().top,
      linkTop: lastLink.getBoundingClientRect().top,
      iconsTop: icons.getBoundingClientRect().top,
      iconsRight: icons.getBoundingClientRect().right,
      contentRight: content.getBoundingClientRect().right,
      headerHeight: header.getBoundingClientRect().height,
      overflowX: document.documentElement.scrollWidth - document.documentElement.clientWidth,
    };
  });
}

async function open(page: Page, width: number) {
  await page.setViewportSize({ width, height: 844 });
  await page.goto("/", { waitUntil: "load" });
  await page.evaluate(() => document.fonts.ready);
}

// No gap tightens after the first paint, so links that only fit tight take row 2 and nothing moves.
test("a near-fitting masthead keeps the toggles on the logo's row and its links below, still, at 390px", async ({ page, browserName }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await holdScript(page);
  await page.goto("/", { waitUntil: "load" });
  await settle(page);
  // Layout-shift entries are Chromium's.
  const shift = browserName === "chromium" ? await shifts(page) : null;
  const m = await masthead(page);
  const where = `${shift?.entries.join("\n") ?? ""}\n${JSON.stringify(m)}`;
  expect(m.linkTop, `the links take row 2: ${where}`).toBeGreaterThan(m.logoTop + 30);
  expect(Math.abs(m.iconsTop - m.logoTop), `the toggles stay on row 1: ${where}`).toBeLessThan(40);
  expect(m.overflowX, where).toBeLessThanOrEqual(0);
  if (shift) {
    expect(shift.paintedFirst, `the first paint must come before the script: ${where}`).toBe(true);
    expect(shift.total, where).toBeLessThan(0.01);
  }
});

test("a masthead that fits at full spacing is not tightened at 1280px", async ({ page }) => {
  await open(page, 1280);
  const m = await masthead(page);
  const where = JSON.stringify(m);
  expect(Math.abs(m.linkTop - m.logoTop), where).toBeLessThan(40);
  // The group break between the links and the toggles keeps its full width.
  expect(m.iconsLeft - m.lastLinkRight, where).toBeGreaterThanOrEqual(24);
});

test("above phone width a near-fitting masthead splits rather than tightening", async ({ page }) => {
  await open(page, 1280);
  // Widen the name until the links overflow its row by 2px at full gaps: the name's end margin and the toggles' start margin.
  await page.evaluate(() => {
    const content = document.querySelector(".nav-content") as HTMLElement;
    const name = document.querySelector(".nav-left .site-name") as HTMLElement;
    const px = (el: Element, prop: "columnGap" | "marginInlineStart" | "marginInlineEnd") => parseFloat(getComputedStyle(el)[prop]) || 0;
    const links = [...document.querySelectorAll<HTMLElement>(".nav-links > :not(.nav-logo-strut)")];
    const linkRow = links.reduce((s, l) => s + l.offsetWidth, 0) + px(document.querySelector(".nav-links")!, "columnGap") * (links.length - 1);
    const icons = document.querySelector(".nav-icons") as HTMLElement;
    const rest = linkRow + icons.offsetWidth + px(icons, "marginInlineStart") + px(name, "marginInlineEnd");
    name.style.display = "inline-block";
    name.style.width = `${content.clientWidth - rest + 2}px`;
    window.dispatchEvent(new Event("resize"));
  });
  await page.waitForTimeout(100);
  const m = await masthead(page);
  const where = JSON.stringify(m);
  expect(m.linkTop, `the links move to row 2: ${where}`).toBeGreaterThan(m.logoTop + 30);
  expect(Math.abs(m.iconsTop - m.logoTop), `the toggles stay on row 1: ${where}`).toBeLessThan(40);
  // Not tightened: the toggles keep the full group break from the name.
  expect(m.iconsLeft - m.logoRight, where).toBeGreaterThanOrEqual(24);
  expect(Math.abs(m.iconsRight - m.contentRight), where).toBeLessThanOrEqual(1);
});

test("a masthead that cannot fit even tight still splits at 330px", async ({ page }) => {
  await open(page, 330);
  const m = await masthead(page);
  const where = JSON.stringify(m);
  expect(m.linkTop, where).toBeGreaterThan(m.logoTop + 30);
  expect(Math.abs(m.iconsTop - m.logoTop), `the toggles stay on row 1: ${where}`).toBeLessThan(40);
  expect(m.overflowX, where).toBeLessThanOrEqual(0);
});

// The lockup is taller than the toggles, sized in px or in the name's em; a theme may also hide the name's text and leave the logo alone.
for (const name of ["shown", "hidden"]) {
  for (const logo of ["44px", "52px", "2.5em"]) {
    // An em logo with the name's text at 0 has no height.
    if (logo.endsWith("em") && name === "hidden") continue;
    for (const width of [900, 1280]) {
      test(`a ${logo} logo with its name ${name} has the toggles and links centred on it at ${width}px`, async ({ page }) => {
        await page.setViewportSize({ width, height: 844 });
        await page.goto("/", { waitUntil: "load" });
        await page.addStyleTag({ content: `.site-logo { height: ${logo}; }${name === "hidden" ? " .site-name { font-size: 0; }" : ""}` });
        await page.evaluate(() => document.fonts.ready);
        const m = await page.evaluate(() => {
          const mid = (s: string) => { const r = document.querySelector(s)!.getBoundingClientRect(); return (r.top + r.bottom) / 2; };
          // Where the logo's box lies before moss nudges the image up a little.
          const nudge = parseFloat(getComputedStyle(document.querySelector(".site-name .site-logo")!).top) || 0;
          return { logo: mid(".site-name .site-logo") - nudge, toggle: mid(".nav-icons > button"), link: mid(".nav-links a") };
        });
        const where = JSON.stringify(m);
        expect(Math.abs(m.toggle - m.logo), where).toBeLessThanOrEqual(1);
        expect(Math.abs(m.link - m.logo), where).toBeLessThanOrEqual(1);
      });
    }
  }
}

// Vertical typesetting lays the masthead out as a flex strip, where the floats do not apply: the logo's copies stay hidden and the toggles end the strip.
for (const width of [412, 900, 1280]) {
  test(`a vertical page keeps the masthead a flex strip with the toggles at its end at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/scroll/", { waitUntil: "load" });
    await page.evaluate(() => document.fonts.ready);
    const m = await page.evaluate(() => {
      const content = document.querySelector(".nav-content") as HTMLElement;
      const box = (s: string) => document.querySelector(s)!.getBoundingClientRect();
      return {
        writingMode: getComputedStyle(content).writingMode,
        display: getComputedStyle(content).display,
        struts: [...document.querySelectorAll(".nav-logo-strut")].map((el) => getComputedStyle(el).display),
        contentTop: box(".nav-content").top,
        contentBottom: box(".nav-content").bottom,
        nameTop: box(".site-name").top,
        linksBottom: box(".nav-links").bottom,
        iconsTop: box(".nav-icons").top,
        iconsBottom: box(".nav-icons").bottom,
      };
    });
    const where = JSON.stringify(m);
    expect(m.writingMode, where).toBe("vertical-rl");
    expect(m.display, where).toBe("flex");
    expect(m.struts.length, where).toBeGreaterThan(0);
    expect(m.struts.every((d) => d === "none"), where).toBe(true);
    expect(Math.abs(m.nameTop - m.contentTop), where).toBeLessThanOrEqual(1);
    expect(Math.abs(m.iconsBottom - m.contentBottom), where).toBeLessThanOrEqual(1);
    expect(m.iconsTop, where).toBeGreaterThanOrEqual(m.linksBottom);
  });
}
