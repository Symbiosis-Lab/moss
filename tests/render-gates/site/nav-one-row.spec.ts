/**
 * Render gate: a masthead whose logo, links and toggles fit on one row once
 * the gaps tighten stays on one row; only a masthead that cannot fit even
 * tight splits into two.
 *
 * The lockup is 44px tall and sized so the default gaps overflow the 390px
 * phone width by a few pixels. Whether the row fits is a question of measured
 * widths, so only a layout engine can answer it.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-one-row.config.ts
 */

import { test, expect, type Page } from "@playwright/test";

async function masthead(page: Page) {
  return page.evaluate(() => {
    const content = document.querySelector(".nav-content") as HTMLElement;
    const logo = document.querySelector(".site-logo") as HTMLElement;
    const icons = document.querySelector(".nav-icons") as HTMLElement;
    const lastLink = [...document.querySelectorAll<HTMLElement>(".nav-links a")].pop()!;
    const header = document.querySelector("header") as HTMLElement;
    return {
      split: content.hasAttribute("data-nav-split"),
      compact: content.hasAttribute("data-nav-compact"),
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

test("a near-fitting masthead keeps links and toggles on the logo's row at 390px", async ({ page }) => {
  await open(page, 390);
  const m = await masthead(page);
  expect(m.split, "must not split").toBe(false);
  expect(m.compact, "must tighten instead").toBe(true);
  expect(Math.abs(m.linkTop - m.logoTop)).toBeLessThan(40);
  expect(Math.abs(m.iconsTop - m.logoTop)).toBeLessThan(40);
  expect(m.headerHeight).toBeLessThan(140);
  expect(m.overflowX).toBeLessThanOrEqual(0);
});

test("a masthead that fits at full spacing is not tightened at 1280px", async ({ page }) => {
  await open(page, 1280);
  const m = await masthead(page);
  expect(m.split).toBe(false);
  expect(m.compact).toBe(false);
});

test("a masthead that cannot fit even tight still splits at 330px", async ({ page }) => {
  await open(page, 330);
  const m = await masthead(page);
  expect(m.split).toBe(true);
  expect(m.compact).toBe(false);
  expect(m.linkTop).toBeGreaterThan(m.logoTop + 30);
});
