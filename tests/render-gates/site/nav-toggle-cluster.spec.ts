/**
 * Render gate: the nav toggle cluster is one optical family.
 *
 * Four assertions, each of which needs a real engine and cannot be read off
 * the emitted HTML:
 *
 *   1. The three `.nav-icons` children (search button, language toggle, theme
 *      button) lay out at the SAME height — the 2.25rem box. Search used to be
 *      the only one with that box, so it rendered taller and heavier than its
 *      siblings. Only layout answers this: the heights come from three
 *      different rules in two different stylesheets.
 *
 *   2. `.nav-search-btn` has NO resting background. It used to carry a
 *      permanent `--moss-color-surface` fill, which over a hero image read as a
 *      bright disc and made search the loudest element in the nav. The pill is
 *      a hover affordance now. `background: none` computing to transparent is a
 *      cascade question across two stylesheets, so it needs the engine; the
 *      radius that makes it a pill does not, and is asserted in Rust.
 *
 *   3. `.moss-search__seam` — the hairline under the search input — paints
 *      `--moss-border-light`, NOT the accent. It was `--moss-color-accent-quiet`
 *      once, which put a green rule under a field that is always focused and
 *      read as a focus underline. Fixed in 51ddaa0ec, when the rule still lived
 *      in site.css; 21b9af407 then moved it into this partial carrying the
 *      fixed value. Nothing covered it either time. Note that neither commit
 *      has shipped — the newest tag is v0.7.24 and search is unreleased — so a
 *      site still showing the green rule is running a moss that predates the
 *      fix, not hitting a regression.
 *      Custom-property resolution through `var()` is exactly what jsdom cannot
 *      do, and the seam only exists after the overlay is opened by the runtime.
 *
 *   4. When a long breadcrumb pushes `.nav-right` onto row 2, the links sit at
 *      the START edge and the toggle cluster at the END edge. This is the one
 *      assertion here that is about the OTHER group boundary, and it is the
 *      only way to catch a specific regression shape: `.nav-right`'s
 *      `flex: 1 1 auto` and `.nav-icons`'s `margin-inline-start: auto` used to
 *      live inside `@media (max-width: 32rem)`, so the same overflow got edge
 *      alignment on a phone and a clumped-right group on a wide screen. Both
 *      are unconditional now and key off whether nav-right actually wrapped —
 *      which only a layout engine can tell you, because it depends on measured
 *      text width, not on a breakpoint. (Do not "simplify" this to
 *      `flex: 1 1 100%`: that forces the wrap even when the content fits, and
 *      split minimal-nav sites across two rows. See the "Edge alignment" block
 *      in site.css.)
 *
 * The fixture turns search on via `[site].search`.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-toggle-cluster.config.ts
 */

import { test, expect, type Page } from "@playwright/test";

/** Computed background-color of a throwaway element painted with `value`. */
async function resolveColor(page: Page, value: string): Promise<string> {
  return page.evaluate((v: string) => {
    const probe = document.createElement("div");
    probe.style.backgroundColor = v;
    document.body.appendChild(probe);
    const computed = window.getComputedStyle(probe).backgroundColor;
    probe.remove();
    return computed;
  }, value);
}

test.beforeEach(async ({ page }) => {
  // Wide enough that nav-right stays on row 1 with the links beside the cluster.
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/", { waitUntil: "load" });
});

test("the three nav toggles share one 2.25rem box", async ({ page }) => {
  const heights = await page.evaluate(() => {
    const ruler = document.createElement("div");
    ruler.style.height = "2.25rem";
    document.body.appendChild(ruler);
    const expected = ruler.getBoundingClientRect().height;
    ruler.remove();
    const children = [...document.querySelectorAll(".nav-icons > *")];
    return {
      expected,
      classes: children.map((el) => el.className),
      measured: children.map((el) => el.getBoundingClientRect().height),
    };
  });

  // All three must be present, or the gate is measuring a nav that never had
  // the problem. Their ORDER is provable from the emitted HTML and is pinned in
  // a Rust test (`test_icon_cluster_order_is_search_language_theme`), so it is
  // deliberately not re-asserted here.
  expect([...heights.classes].sort()).toEqual([
    "nav-lang-toggle",
    "nav-search-btn",
    "nav-theme-btn",
  ]);
  for (const [i, h] of heights.measured.entries()) {
    expect(h, `${heights.classes[i]} must be the 2.25rem box`).toBeCloseTo(
      heights.expected,
      1,
    );
  }
});

test("the search button has no resting fill", async ({ page }) => {
  const resting = await page
    .locator(".nav-search-btn")
    .evaluate((el) => window.getComputedStyle(el).backgroundColor);
  expect(resting, "search must be transparent at rest, like its siblings").toBe(
    "rgba(0, 0, 0, 0)",
  );

  // That the pill still EXISTS — `border-radius: 18px` — is provable from the
  // emitted text and is asserted in `test_css_search_button_keeps_its_pill`.
  // Reading it back through getComputedStyle here would just echo a literal
  // declaration with no cascade or var() resolution involved, which is the
  // definition of an assertion at the wrong level.
});

test("the search overlay seam is a neutral hairline, not an accent underline", async ({
  page,
}) => {
  await page.locator(".nav-search-btn").click();
  const seam = page.locator(".moss-search__seam");
  await expect(seam).toBeVisible();

  const painted = await seam.evaluate(
    (el) => window.getComputedStyle(el).backgroundColor,
  );
  const borderLight = await resolveColor(page, "var(--moss-border-light)");
  const accent = await resolveColor(page, "var(--moss-color-accent)");
  const accentQuiet = await resolveColor(page, "var(--moss-color-accent-quiet)");

  expect(painted, "seam must paint --moss-border-light").toBe(borderLight);
  expect(painted).not.toBe(accent);
  expect(painted, "the green focus-underline regression").not.toBe(accentQuiet);
});

test("a wrapped nav row puts the links at the start edge and the toggles at the end", async ({
  page,
}) => {
  // 1024px: a desktop width, comfortably above the 48rem medium-nav breakpoint,
  // so any wrap here is driven by the breadcrumb's measured width and not by a
  // media query. This is the width at which the old ≤32rem-scoped rules failed.
  await page.setViewportSize({ width: 1024, height: 900 });
  await page.goto(
    "/programmes-and-long-form-reporting/field-notes-from-a-very-long-section-title/",
    { waitUntil: "load" },
  );

  const box = await page.evaluate(() => {
    const q = (sel: string) => document.querySelector(sel);
    const content = q(".nav-content") as HTMLElement;
    const left = q(".nav-left") as HTMLElement;
    const right = q(".nav-right") as HTMLElement;
    const links = q(".nav-links") as HTMLElement;
    const icons = q(".nav-icons") as HTMLElement;
    if (!content || !left || !right || !links || !icons) return null;
    // Content box of .nav-content — the edges the two groups must align to.
    // .nav-content carries padding-bottom only, but read the inline padding
    // rather than assume it, so this stays true if that ever changes.
    const cs = window.getComputedStyle(content);
    const cRect = content.getBoundingClientRect();
    return {
      contentStart: cRect.left + parseFloat(cs.paddingLeft),
      contentEnd: cRect.right - parseFloat(cs.paddingRight),
      leftTop: left.getBoundingClientRect().top,
      rightTop: right.getBoundingClientRect().top,
      linksStart: links.getBoundingClientRect().left,
      iconsEnd: icons.getBoundingClientRect().right,
    };
  });

  expect(box, ".nav-content and both groups must exist").not.toBeNull();
  const b = box!;

  // Precondition: the breadcrumb actually pushed nav-right to a second row.
  // Without this the two edge assertions below would pass trivially on a
  // single-row nav, and the gate would be asserting nothing.
  expect(
    b.rightTop,
    "fixture must be wide/long enough to wrap nav-right onto row 2 — " +
      "if this fails the breadcrumb got shorter, not the layout broken",
  ).toBeGreaterThan(b.leftTop);

  expect(b.linksStart, "links must sit at the start edge").toBeCloseTo(
    b.contentStart,
    0,
  );
  expect(b.iconsEnd, "toggles must sit at the end edge").toBeCloseTo(
    b.contentEnd,
    0,
  );
});
