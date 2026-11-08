/**
 * A map over a historical layer that fills only part of its 3:2 frame is cut
 * to the layer: the figure takes the layer's aspect and its svg fills it. The
 * places root, an explorer, keeps filling the window, and a tall figure never
 * outgrows the window's height.
 *
 * Run via:
 *   npx playwright test -c playwright/place-map-cut.config.ts
 */
import { test, expect, type Page } from "@playwright/test";

const SIZES = [{ width: 390, height: 844 }, { width: 1280, height: 860 }];

async function boxes(page: Page, selector: string) {
  return page.locator(selector).first().evaluate((figure) => {
    const box = (el: Element) => {
      const r = el.getBoundingClientRect();
      return { x: r.x, y: r.y, w: r.width, h: r.height };
    };
    return { figure: box(figure), svg: box(figure.querySelector("svg")!), cut: figure.hasAttribute("data-map-cut"), ar: getComputedStyle(figure).getPropertyValue("--moss-place-map-ar") };
  });
}

for (const size of SIZES) {
  test(`a place page's map takes the layer's shape and its svg fills it, at ${size.width}px`, async ({ page }) => {
    await page.setViewportSize(size);
    await page.goto("places/churchyard-gate/", { waitUntil: "domcontentloaded" });
    const { figure, svg, cut, ar } = await boxes(page, "figure.moss-place-map");
    expect(cut, "the figure is cut to the layer").toBe(true);
    expect(Math.abs(figure.w / figure.h - Number(ar)), `figure ${figure.w}x${figure.h} follows its shape ${ar}`).toBeLessThan(0.02);
    expect(Math.abs(figure.w / figure.h - 1.5), "no longer the fixed 3:2 frame").toBeGreaterThan(0.03);
    expect(Math.abs(svg.w - figure.w) + Math.abs(svg.h - figure.h), "the svg fills the figure").toBeLessThan(2);
  });

  test(`a figure much taller than wide is held to 80% of the window's height, at ${size.width}px`, async ({ page }) => {
    await page.setViewportSize(size);
    await page.goto("places/churchyard-gate/", { waitUntil: "domcontentloaded" });
    await page.locator("figure.moss-place-map").evaluate((el) => (el as HTMLElement).style.setProperty("--moss-place-map-ar", "0.6"));
    const { figure } = await boxes(page, "figure.moss-place-map");
    expect(figure.h, `figure ${figure.w}x${figure.h}`).toBeLessThanOrEqual(size.height * 0.8 + 1);
    expect(figure.h, "and still tall").toBeGreaterThan(figure.w);
  });

  test(`the locator beside a located page's prose takes the layer's shape inside its wrapper, at ${size.width}px`, async ({ page }) => {
    await page.setViewportSize(size);
    await page.goto("posts/gate/", { waitUntil: "domcontentloaded" });
    const wrapper = await page.locator(".moss-place-locator").first().evaluate((el) => el.getBoundingClientRect().width);
    // A locator hydrates into an iframe and hides its svg, so only the figure's own box is measured.
    const { figure, cut, ar } = await boxes(page, ".moss-place-locator figure.moss-place-map");
    expect(cut).toBe(true);
    expect(Math.abs(figure.w / figure.h - Number(ar)), `figure ${figure.w}x${figure.h} follows its shape ${ar}`).toBeLessThan(0.02);
    expect(figure.w, "within its wrapper").toBeLessThanOrEqual(wrapper + 1);
  });

  test(`the places root still fills the window, at ${size.width}px`, async ({ page }) => {
    await page.setViewportSize(size);
    await page.goto("places/", { waitUntil: "domcontentloaded" });
    const figure = page.locator("figure[data-moss-places-explorer]");
    await expect(figure).toHaveAttribute("data-moss-places-explorer-ready", "ready", { timeout: 10000 });
    const box = await figure.evaluate((el) => el.getBoundingClientRect().toJSON());
    expect(box.width, "the explorer spans the window").toBeGreaterThanOrEqual(size.width - 2);
    expect(box.height, "and is not cut to the layer's shape").toBeGreaterThanOrEqual(480);
  });
}
