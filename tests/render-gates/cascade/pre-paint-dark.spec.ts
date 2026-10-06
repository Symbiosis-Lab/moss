/**
 * Render gate for Task 2.3: the pre-paint data-theme script.
 *
 * The fixture's theme CSS has ONLY a `[data-theme="dark"]` rule and NO
 * `@media (prefers-color-scheme: dark)` block. So with a dark-preference OS and
 * empty localStorage, the dark background can only appear if the inline
 * pre-paint script read matchMedia and set `data-theme` on <html> before the
 * stylesheet was parsed. Remove the script and this goes red.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts, built by the
 * playwright config at parse time and served by its webServer.
 *
 * Run via:
 *   npx playwright test -c playwright/pre-paint-dark.config.ts
 */
import { test, expect } from "@playwright/test";

const EXPECTED_BG = "rgb(17, 17, 17)"; // #111111

// One render, both readings — the attribute is implied by the background, but
// reading it costs nothing and says which half broke.
test("pre-paint script applies the system dark preference before first paint", async ({
  page,
}) => {
  // Clear any localStorage a previous run left, then reload so the script runs
  // with the system preference as its only input.
  await page.goto("./", { waitUntil: "domcontentloaded" });
  await page.evaluate(() => localStorage.removeItem("moss-theme"));
  await page.reload({ waitUntil: "domcontentloaded" });

  const seen = await page.evaluate(() => ({
    dataTheme: document.documentElement.getAttribute("data-theme"),
    bg: window.getComputedStyle(document.body).backgroundColor,
  }));

  expect(
    seen.dataTheme,
    "pre-paint script must set <html data-theme=\"dark\"> from matchMedia when " +
      "localStorage is empty",
  ).toBe("dark");
  expect(
    seen.bg,
    `body background must be ${EXPECTED_BG}, which requires data-theme to have been ` +
      "set BEFORE stylesheet parse — the fixture has no @media dark block to fall back on",
  ).toBe(EXPECTED_BG);
});
