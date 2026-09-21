/**
 * Render gate for native feature CSS @layer contract.
 *
 * Verifies in BOTH chromium and webkit that:
 *
 *   1. User `.comment-author { color: rgb(0, 128, 0) }` in @layer themes
 *      wins over the native comments.css rule in @layer plugins — proving
 *      the "user CSS wins" cascade contract for native feature CSS.
 *
 *   2. The comment section itself still renders (`.moss-comments` element
 *      exists) — confirming the layer wrapping doesn't break rendering.
 *
 * Run via:
 *   npx playwright test -c playwright/comments-cascade.config.ts
 */
import { test, expect } from "@playwright/test";

// Relative, so the port lives only in the config's baseURL.
// hello.md renders at /hello/index.html.
const ARTICLE_PAGE = "hello/";

// One navigation, both assertions: they read the same render, and the fixture
// site is built once for the whole gate. Loading the page a second time to
// check `.moss-comments` proved nothing the first load had not already fixed.
test("user CSS in @layer themes beats native comments CSS in @layer plugins", async ({
  page,
}) => {
  // The baked page links Artalk from the configured comment server
  // (example.com in the fixture), and a classic <script src> blocks
  // DOMContentLoaded — so every navigation sat waiting out a connection to a
  // host that does not exist, ~13 s a time. The gate is about which stylesheet
  // wins, so cut the network at the browser: aborting everything off-origin
  // makes it hermetic as well as ~14× faster.
  await page.route("**/*", (route) =>
    new URL(route.request().url()).hostname === "localhost"
      ? route.continue()
      : route.abort(),
  );
  await page.goto(ARTICLE_PAGE, { waitUntil: "domcontentloaded" });

  // The comment section is baked into the static HTML; layer wrapping must not
  // have stopped it rendering.
  await expect(
    page.locator(".moss-comments"),
    ".moss-comments must exist in the baked article page",
  ).toHaveCount(1);

  // .comment-author only exists once Artalk has run, which it deliberately
  // never does here — so synthesize the node. The cascade question is about
  // the class, not about who created the element.
  const color = await page.evaluate(() => {
    const tmp = document.createElement("div");
    tmp.className = "comment-author";
    document.body.appendChild(tmp);
    const computed = window.getComputedStyle(tmp).color;
    tmp.remove();
    return computed;
  });
  expect(
    color,
    ".comment-author computed color must be rgb(0, 128, 0) — user @layer themes beats @layer plugins",
  ).toBe("rgb(0, 128, 0)");
});
