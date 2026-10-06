/**
 * The built site's JupyterLite opens the site's notebook.
 *
 * This is the gate a bundle-pin bump must pass: the pin (notebook.rs) fixes
 * WHICH bundle ships, and this asserts the bundle still means what moss's
 * config patch and contents index assume. The 0.7→0.8 bump changed an unset
 * `contentsAllJsonFile` from "use the default" to "serve nothing" — every
 * layer below this one stayed green while opening any notebook failed with
 * "Could not find content with path".
 */
import { test, expect } from "@playwright/test";

test("JupyterLite renders the notebook's cells from moss's contents index", async ({
  page,
}) => {
  // Straight to the JupyterLite notebooks app, as the viewer iframe would.
  await page.goto("/jupyter/notebooks/index.html?path=analysis.ipynb");

  // The sentinel lives in the notebook's first markdown cell, so it can only
  // appear if the app fetched api/contents/ and opened the file. When the
  // contents wiring is broken this never renders — the app shows the
  // "Could not find content" dialog instead. The `<p>` narrows it to the
  // RENDERED cell (the text also exists as a CodeMirror source line).
  await expect(
    page.locator("p", { hasText: "moss-notebook-gate-sentinel" }),
  ).toBeVisible({ timeout: 90_000 });
});

test("the viewer page wraps the notebook in a JupyterLite iframe", async ({
  page,
}) => {
  const response = await page.goto("/analysis.html");
  expect(response?.status()).toBe(200);
  const src = await page.locator("iframe").getAttribute("src");
  expect(src).toContain("/jupyter/notebooks/?path=analysis.ipynb");
});
