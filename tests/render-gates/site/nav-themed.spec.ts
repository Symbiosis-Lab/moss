/**
 * Render gate: a page with a stylesheet of its own keeps the flex masthead
 * that stylesheet was written against.
 *
 * The build marks such a masthead `data-nav-themed`, and the plain name's
 * float layout stays off it. "menu": the theme hides the toggles and shows
 * moss's menu button up to 760px with an auto start margin; on a flex row the
 * button ends the name's row, where under the float it would start a second
 * one. "split": the theme sizes the flex row and styles the split, and a long
 * plain name with five links gets `data-nav-split` where it does not fit.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-themed.config.ts
 */

import { test, expect, type Page } from "@playwright/test";
import { gatePort } from "../../../playwright/gate-ports";

const SPLIT_HOME = `http://localhost:${gatePort("nav-themed:split")}/`;

async function open(page: Page, url: string, width: number) {
  await page.setViewportSize({ width, height: 900 });
  await page.goto(url, { waitUntil: "load" });
  await page.evaluate(() => document.fonts.ready);
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
}

for (const width of [375, 600, 700, 760]) {
  test(`a themed menu button stays on the name's row at ${width}px`, async ({ page }) => {
    await open(page, "/", width);
    const m = await page.evaluate(() => {
      const content = document.querySelector<HTMLElement>(".nav-content")!;
      const rect = (sel: string) => document.querySelector(sel)!.getBoundingClientRect();
      const name = rect(".site-name");
      const button = rect(".mobile-menu-button");
      return {
        themed: content.hasAttribute("data-nav-themed"),
        display: getComputedStyle(content).display,
        nameTop: name.top,
        nameBottom: name.bottom,
        buttonTop: button.top,
        buttonBottom: button.bottom,
        buttonWidth: button.width,
        rowBox: (() => {
          const cs = getComputedStyle(content);
          return content.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
        })(),
      };
    });
    const where = JSON.stringify(m);
    expect(m.themed, where).toBe(true);
    expect(m.display, where).toBe("flex");
    expect(m.buttonWidth, where).toBeGreaterThan(0);
    expect(m.buttonTop, where).toBeLessThan(m.nameBottom);
    expect(m.buttonBottom, where).toBeGreaterThan(m.nameTop);
    const row = Math.max(m.nameBottom, m.buttonBottom) - Math.min(m.nameTop, m.buttonTop);
    expect(m.rowBox, where).toBeLessThanOrEqual(row + 1);
  });
}

for (const width of [700, 900]) {
  test(`a themed long plain name splits from its links at ${width}px`, async ({ page }) => {
    await open(page, SPLIT_HOME, width);
    const m = await page.evaluate(() => {
      const content = document.querySelector<HTMLElement>(".nav-content")!;
      const name = document.querySelector(".site-name")!.getBoundingClientRect();
      const links = document.querySelector(".nav-links")!.getBoundingClientRect();
      return {
        themed: content.hasAttribute("data-nav-themed"),
        split: content.hasAttribute("data-nav-split"),
        display: getComputedStyle(content).display,
        nameBottom: name.bottom,
        linksTop: links.top,
        contentHeight: content.getBoundingClientRect().height,
      };
    });
    const where = JSON.stringify(m);
    expect(m.themed, where).toBe(true);
    expect(m.split, where).toBe(true);
    expect(m.display, where).toBe("grid");
    expect(m.linksTop, where).toBeGreaterThanOrEqual(m.nameBottom - 0.5);
    expect(m.contentHeight, where).toBeGreaterThanOrEqual(76);
  });
}
