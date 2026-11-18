/**
 * Render gate: a theme that hosts a Regular and a Bold, and sets no weight,
 * gets its bold text in the Bold and its headings in the Regular they drew.
 *
 * moss asks for both at weight 480. A browser matching 480 against faces at
 * 400 and 700 picks 400, so without the build's derived weight the text's Bold
 * file is never fetched: its face stays "unloaded", which is what this reads,
 * in BOTH chromium and webkit. The title family's Bold must stay unloaded.
 *
 * Run via:
 *   npx playwright test -c playwright/hosted-bold.config.ts
 */

import { test, expect } from "@playwright/test";

test("bold text draws in the hosted family's Bold and the page title keeps its Regular", async ({ page }) => {
  await page.goto("hosted/", { waitUntil: "load" });
  await page.evaluate(() => document.fonts.ready);
  const seen = await page.evaluate(() => {
    const weight = (sel: string) => {
      const el = document.querySelector(sel);
      return el ? getComputedStyle(el).fontWeight : null;
    };
    const bold = (family: string) =>
      [...document.fonts]
        .filter((f) => f.family.replace(/["']/g, "") === family && f.weight === "700")
        .map((f) => f.status);
    return {
      strong: weight("article strong"),
      title: weight(".moss-article-title"),
      textBold: bold("Hosted Text"),
      titleBold: bold("Hosted Title"),
    };
  });
  expect(seen.strong, "bold text asks for the Bold").toBe("700");
  expect(seen.title, "the page title keeps moss's heading weight").toBe("480");
  expect(seen.textBold, "the text family's Bold file is the one drawn").toEqual(["loaded"]);
  expect(seen.titleBold, "the title family's Bold file is never drawn").toEqual(["unloaded"]);
});
