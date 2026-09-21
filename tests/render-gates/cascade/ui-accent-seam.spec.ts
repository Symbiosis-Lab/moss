/**
 * Render gate for Task 2.5: the --moss-color-ui-accent seam.
 *
 * Two sites, because the seam only exists as a difference between them:
 *
 *   DEFAULT (9373)   no theme CSS. --moss-color-ui-accent falls back to
 *                    var(--moss-color-accent), so both probes read the accent.
 *   OVERRIDE (9374)  theme CSS points ui-accent at var(--moss-color-text). The
 *                    chrome probe moves; the content probe must NOT.
 *
 * The override site's *unchanged* content probe is the real assertion: if
 * `article a` were ever repointed at --moss-color-ui-accent, a reader opting
 * into quiet chrome would silently lose their link colour too.
 *
 * Only a real engine can answer this — jsdom resolves neither var() chaining
 * nor @layer precedence.
 *
 * The two scratch sites come from tests/e2e/helpers/gate-sites.ts, built by the
 * playwright config at parse time and served by its two webServers.
 *
 * Run via:
 *   npx playwright test -c playwright/ui-accent-seam.config.ts
 */
import { test, expect } from "@playwright/test";

const DEFAULT_BASE = "http://localhost:9373/";
const OVERRIDE_BASE = "http://localhost:9374/";
const PROBE_PAGE = "ui-accent-seam-probe.html";

const EXPECTED_ACCENT = "rgb(45, 90, 45)"; // #2d5a2d
const EXPECTED_TEXT = "rgb(44, 40, 37)"; // #2c2825

/**
 * Read both probes from one render.
 *
 * `waitUntil: "load"` is required, not stylistic: WebKit may not have applied
 * <link> CSS at domcontentloaded, and every custom-property lookup then comes
 * back as an empty string.
 */
async function probes(page: import("@playwright/test").Page, base: string) {
  await page.goto(`${base}${PROBE_PAGE}`, { waitUntil: "load" });
  return page.evaluate(() => {
    const color = (sel: string) => {
      const el = document.querySelector(sel);
      return el ? window.getComputedStyle(el).color : null;
    };
    return { chrome: color("#chrome-probe"), content: color("#content-probe") };
  });
}

// One test per site rather than one per probe: the two probes are two readings
// of a single render, and the pair is the assertion — reading them apart could
// not express "one moved and the other did not".
test("without an override, chrome and content accents are the same colour", async ({
  page,
}) => {
  const seen = await probes(page, DEFAULT_BASE);
  expect(
    seen.chrome,
    "--moss-color-ui-accent must fall back to var(--moss-color-accent)",
  ).toBe(EXPECTED_ACCENT);
  expect(seen.content, "--moss-color-accent is the accent green").toBe(EXPECTED_ACCENT);
});

test("overriding ui-accent moves chrome and leaves content untouched", async ({
  page,
}) => {
  const seen = await probes(page, OVERRIDE_BASE);
  expect(
    seen.chrome,
    "chrome must follow the user's ui-accent override to the text neutral",
  ).toBe(EXPECTED_TEXT);
  expect(
    seen.content,
    "content links read --moss-color-accent directly and must NOT follow the " +
      "ui-accent override — if this fails, `article a` was repointed at ui-accent",
  ).toBe(EXPECTED_ACCENT);
});
