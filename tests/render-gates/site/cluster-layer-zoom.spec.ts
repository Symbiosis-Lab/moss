/**
 * Render gate: on a map whose places all sit on a historical layer, clicking
 * a cluster zooms in to its members (or opens their ring) and never leaves
 * the reader at a shallower zoom than before the click.
 *
 * Run via:
 *   npx playwright test -c playwright/cluster-layer-zoom.config.ts
 */
import { test, expect, type Page } from "@playwright/test";

async function open(page: Page): Promise<void> {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("places/", { waitUntil: "domcontentloaded" });
  await expect(page.locator("figure[data-moss-places-explorer]")).toHaveAttribute("data-moss-places-explorer-ready", "ready", { timeout: 10000 });
  await page.waitForTimeout(500);
}

const zoomOf = (page: Page): number => Number(new URL(page.url()).searchParams.get("z"));

test("a cluster click over a layer does not zoom out", async ({ page }) => {
  await open(page);
  const cluster = page.locator('.moss-places-marker[data-count="3"]');
  await expect(cluster, "the three nearby works open as one cluster of three").toHaveCount(1);
  const before = zoomOf(page);
  expect(before, "the explorer opens fitted to the layer").toBeGreaterThan(0);

  await cluster.click();
  await page.waitForTimeout(600);

  expect(zoomOf(page), "no shallower than before the click").toBeGreaterThanOrEqual(before - 1e-6);
  await expect(page.locator('.moss-places-marker[data-count="3"]'), "the cluster has split").toHaveCount(0);
  expect(await page.locator(".moss-places-marker").count(), "its members are still on screen").toBeGreaterThanOrEqual(2);
});

test("a work's locator over a layer opens at the layer's depth, not a regional view", async ({ page }) => {
  await open(page);
  const all = zoomOf(page);
  await page.goto("places/?article=/gate-morning/&embed=1", { waitUntil: "domcontentloaded" });
  await expect(page.locator("figure[data-moss-places-explorer]")).toHaveAttribute("data-moss-places-explorer-ready", "ready", { timeout: 10000 });
  await page.waitForTimeout(500);
  expect(zoomOf(page), "framed on the layer like the places page, not on the region").toBeGreaterThanOrEqual(all - 1e-6);
});
