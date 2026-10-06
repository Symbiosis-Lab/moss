/**
 * Render gate: --moss-font-heading-weight token wiring.
 *
 * Verifies in BOTH chromium and webkit that:
 *
 *   1. The user's :root override `--moss-font-heading-weight: 440` actually
 *      controls the computed font-weight of .moss-article-title (the page title)
 *      and an in-content h2 — both must resolve to 440.
 *
 *   2. Without the override the defaults would be 480; with 440 the token is
 *      demonstrably wired (not a no-op).
 *
 * The h2 half of that used to be the opposite assertion — that the token
 * STOPPED at article subheads, which pinned 500 of their own. Two weights
 * answering one question meant a theme moving the token moved half the page;
 * the pin is gone and the token now owns every heading.
 *
 * These assertions require a real browser render — jsdom cannot compute
 * font-weight from CSS custom properties.
 *
 * Run via:
 *   npx playwright test -c playwright/heading-weight.config.ts
 */

import { test, expect } from "@playwright/test";

// Relative, so the port lives in exactly one place — the config's baseURL.
// It was hardcoded to 8744 here, which is comments-cascade's port; the two
// gates could not run concurrently and this one silently probed whichever
// site happened to be served.
//
// token-heading.md is a standalone article, so it gets an auto-injected
// .moss-article-title, and its body `## heading` gives us the in-content case.
const ARTICLE_URL = "token-heading/";

// Browsers normalise font-weight to a number string.
const EXPECTED_WEIGHT = "440";

// The token's reach, from one render: the masthead and an in-article subhead.
//
// The h2 case is the one that moved. `article h1..h6` used to pin
// font-weight: 500 alongside the body font family (48b5a9f7e, "lighten article
// heading weight 600 -> 500"), which outranked the `h1..h6` rule that reads the
// token — so the token governed the masthead and nothing else, and a theme
// setting it moved half the page. The pin is deleted; article subheads still
// take the BODY FAMILY (that is the serif-title-only model and is untouched),
// they just no longer carry a second opinion about weight. Nothing changes on
// the CJK sans, where the PingFang pin resolves both 480 and 500 to Medium.
test("--moss-font-heading-weight drives every heading, masthead and article subhead alike", async ({
  page,
}) => {
  await page.goto(ARTICLE_URL, { waitUntil: "domcontentloaded" });
  const weights = await page.evaluate(() => {
    const weight = (sel: string) => {
      const el = document.querySelector(sel);
      return el ? window.getComputedStyle(el).fontWeight : null;
    };
    return { title: weight(".moss-article-title"), h2: weight("article h2") };
  });
  expect(weights.title, ".moss-article-title must follow the token").toBe(
    EXPECTED_WEIGHT,
  );
  expect(
    weights.h2,
    "article h2 must follow the token too — one weight authority, not two",
  ).toBe(EXPECTED_WEIGHT);
});
