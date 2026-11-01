/**
 * Grid tile chrome styles the grid's own cells, not a listing embedded in one.
 *
 * Site: tests/e2e/helpers/gate-sites.ts → GRID_TILE_SCOPE_GATE. `/` has a
 * listing in a `:::grid` cell and a real tile; `/plain/` has the same listing
 * outside any grid.
 */
import { test, expect, type Page } from "@playwright/test";

const rowStyle = (page: Page, selector: string) =>
  page.$$eval(selector, (els) =>
    els.map((el) => {
      const cs = getComputedStyle(el);
      return {
        radius: cs.borderTopLeftRadius,
        overflow: cs.overflow,
        background: cs.backgroundColor,
      };
    }),
  );

const coverStyle = (page: Page, selector: string) =>
  page.$$eval(selector, (els) =>
    els.map((el) => {
      const cs = getComputedStyle(el);
      return { radius: cs.borderTopLeftRadius, ratio: cs.aspectRatio, margin: cs.marginTop };
    }),
  );

test("a listing embedded in a grid cell looks as it does outside a grid", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });

  await page.goto("/plain/");
  const outside = await rowStyle(page, ".moss-cards .moss-card");
  expect(outside.length, "the listing outside a grid rendered no rows").toBeGreaterThan(0);

  await page.goto("/");
  const inside = await rowStyle(page, ".moss-grid-card .moss-cards .moss-card");
  expect(inside.length, "the listing in the grid cell rendered no rows").toBe(outside.length);
  for (const row of inside) expect(row).toEqual(outside[0]);
});

test("images in an embedded listing do not take the cell's picture tile", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/plain/");
  const outside = await coverStyle(page, ".moss-cards img");
  expect(outside.length, "the listing outside a grid rendered no image").toBeGreaterThan(0);

  await page.goto("/");
  const inside = await coverStyle(page, ".moss-grid-card .moss-cards img");
  expect(inside.length).toBe(outside.length);
  for (const img of inside) expect(img).toEqual(outside[0]);
});

test("a real grid tile keeps its rounded clip", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  const tiles = await rowStyle(page, ".moss-grid > a.moss-card");
  expect(tiles.length).toBe(1);
  expect(tiles[0].radius).toBe("6px");
  expect(tiles[0].overflow).toBe("hidden");
});
