/**
 * Render gate: six under-44px header icon controls get an invisible hit
 * area reaching 44px without moving the visible pill or glyph, and the
 * reading-size control gets the same `:focus-visible` ring every sibling
 * header control has.
 *
 * Both halves need a real engine and cannot be read off the emitted HTML:
 *
 *   1. Hit area. Each `::before` pseudo-element's reach is an `inset`
 *      resolved against the control's OWN laid-out box — a cascade + layout
 *      question across two stylesheets (site.css, search.css) that only a
 *      real engine answers. `elementFromPoint` at a point outside the
 *      visible box is the one test that actually proves a click there
 *      ACTIVATES the right control rather than just proving a box exists at
 *      that size on paper.
 *
 *      `.nav-theme-btn` and `.nav-search-btn` sit in `.nav-icons` with a
 *      fixed 4px `gap` between adjacent toggles (site.css). Two adjacent
 *      36px boxes sharing a 4px gap can each reach AT MOST 2px into it
 *      without overlapping — 18px (half the visible box) + 2px = 20px from
 *      centre, which is the mathematical ceiling, not a point safely inside
 *      it. A point tested at exactly that ceiling lands on a closed/open
 *      boundary that different sub-pixel layouts (this fixture's own
 *      breadcrumb trail shifts the cluster by a fraction of a px between
 *      1440 and 390) resolve to either side of. The inward/constrained
 *      assertions below therefore probe 19px — provably inside the 2px
 *      split — and the free/outward sides, which reach the full 22px
 *      (44px box), probe the full 20px the spec asks for.
 *
 *      `.nav-lang-link` (present whenever the fixture ships a translation)
 *      sits between those two and is smaller still — its own reach stays
 *      inside `.nav-lang-toggle`'s box rather than splitting the `.nav-icons`
 *      gap, so it needs no ceiling math, only proof that it doesn't cross
 *      into either button's own territory. See its CSS comment for why.
 *
 *   2. Focus ring. `getComputedStyle` after `.focus()` resolves the
 *      `var(--moss-color-ui-accent)` the outline paints with, which
 *      `outline-color` on an un-focused element would not show at all —
 *      exactly the kind of cascade+state resolution `no-important-cascade`
 *      and its siblings exist to check with a real engine rather than by
 *      grepping the emitted CSS text.
 *
 * Run via:
 *   npx playwright test -c playwright/header-hit-areas.config.ts
 */
import { test, expect, type Page } from "@playwright/test";

async function setTheme(page: Page, theme: "light" | "dark") {
  await page.evaluate((t) => localStorage.setItem("moss-theme", t), theme);
  await page.reload({ waitUntil: "load" });
}

/** `elementFromPoint` `dx,dy` from `sel`'s centre; returns the hit element's class (or tag). */
async function hitAt(page: Page, sel: string, dx: number, dy: number) {
  return page.evaluate(
    ({ sel, dx, dy }) => {
      const el = document.querySelector(sel);
      if (!el) return null;
      const r = el.getBoundingClientRect();
      const cx = r.left + r.width / 2 + dx;
      const cy = r.top + r.height / 2 + dy;
      const hit = document.elementFromPoint(cx, cy);
      return hit ? hit.className || hit.tagName : null;
    },
    { sel, dx, dy },
  );
}

/**
 * Scroll to `y` and wait two animation frames — nav-island.ts reads scroll
 * direction from a running total, so a step it never actually saw a
 * dispatched `scroll` event for reads as the opposite direction on the
 * next one. Mirrors footnote-target.spec.ts's own `scrollAndSettle`.
 */
async function scrollAndSettle(page: Page, y: number): Promise<void> {
  await page.evaluate(
    (target) =>
      new Promise<void>((resolve) => {
        window.scrollTo(0, target);
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
      }),
    y,
  );
}

async function visibleBox(page: Page, sel: string) {
  return page.evaluate((s) => {
    const el = document.querySelector(s);
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { w: r.width, h: r.height };
  }, sel);
}

for (const width of [1440, 390]) {
  test(`.nav-theme-btn and .nav-search-btn reach a 44px hit area at ${width}px without moving the 36px pill`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/", { waitUntil: "load" });

    // Visible box unmoved: the ::before is absolutely positioned, so it can
    // never grow the button's own layout size, but a regression that
    // resized the button itself (rather than adding a hit-area pseudo)
    // would still slip past a click-only test.
    for (const sel of [".nav-theme-btn", ".nav-search-btn"]) {
      const box = await visibleBox(page, sel);
      expect(box, `${sel} must exist`).not.toBeNull();
      expect(box!.w, `${sel} visible width`).toBeCloseTo(36, 0);
      expect(box!.h, `${sel} visible height`).toBeCloseTo(36, 0);
    }

    // Outward (free) sides: the full 44px box, tested at the spec's 20px.
    expect(
      await hitAt(page, ".nav-theme-btn", 20, 0),
      "20px outward (right) of .nav-theme-btn must still hit it",
    ).toBe("nav-theme-btn");
    expect(
      await hitAt(page, ".nav-search-btn", -20, 0),
      "20px outward (left) of .nav-search-btn must still hit it",
    ).toBe("nav-search-btn");

    // Inward (shared-gap) sides: the split's provable 2px ceiling, tested at
    // 19px — see the file header for why not 20px.
    expect(
      await hitAt(page, ".nav-theme-btn", -19, 0),
      "19px inward (left, toward the toggle cluster) of .nav-theme-btn must still hit it, not a neighbour",
    ).toBe("nav-theme-btn");
    expect(
      await hitAt(page, ".nav-search-btn", 19, 0),
      "19px inward (right, toward the toggle cluster) of .nav-search-btn must still hit it, not a neighbour",
    ).toBe("nav-search-btn");

    // The vertical (block) sides are unconstrained on both buttons — full 20px.
    for (const sel of [".nav-theme-btn", ".nav-search-btn"]) {
      expect(await hitAt(page, sel, 0, 20), `${sel} +20px down`).toBe(
        sel.slice(1),
      );
      expect(await hitAt(page, sel, 0, -20), `${sel} -20px up`).toBe(
        sel.slice(1),
      );
    }
  });
}

test(".nav-lang-link reaches a 44px hit area without leaving .nav-lang-toggle's own box, and without overlapping .nav-search-btn or .nav-theme-btn", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/", { waitUntil: "load" });

  // Visible box unmoved — the smallest control in the cluster, well under 44px
  // on both axes, which is exactly why it needs this test at all.
  const box = await visibleBox(page, ".nav-lang-link");
  expect(box, ".nav-lang-link must exist on a translated build").not.toBeNull();
  expect(box!.w, ".nav-lang-link visible width").toBeLessThan(20);
  expect(box!.h, ".nav-lang-link visible height").toBeLessThan(30);

  // Both inline sides stay inside .nav-lang-toggle's own footprint (see the
  // CSS comment) — probed well short of the CSS's own -35px/-7px insets so
  // the assertion doesn't hug that exact boundary, only proves real reach.
  expect(
    await hitAt(page, ".nav-lang-link", -30, 0),
    "30px toward .nav-search-btn must still hit .nav-lang-link, not .nav-search-btn",
  ).toBe("nav-lang-link");
  expect(
    await hitAt(page, ".nav-lang-link", 6, 0),
    "6px toward .nav-theme-btn must still hit .nav-lang-link, not .nav-theme-btn",
  ).toBe("nav-lang-link");

  // Block sides are unconstrained, like the two button siblings — same 20px probe.
  expect(await hitAt(page, ".nav-lang-link", 0, 20), ".nav-lang-link +20px down").toBe(
    "nav-lang-link",
  );
  expect(await hitAt(page, ".nav-lang-link", 0, -20), ".nav-lang-link -20px up").toBe(
    "nav-lang-link",
  );

  // Neither neighbour's own hit area moved to make room — this rule doesn't
  // need them to, since it never leaves .nav-lang-toggle's own box.
  expect(await hitAt(page, ".nav-search-btn", -20, 0)).toBe("nav-search-btn");
  expect(await hitAt(page, ".nav-search-btn", 19, 0)).toBe("nav-search-btn");
  expect(await hitAt(page, ".nav-theme-btn", 20, 0)).toBe("nav-theme-btn");
  expect(await hitAt(page, ".nav-theme-btn", -19, 0)).toBe("nav-theme-btn");
});

test("bilingual cluster: a click just outside each of the three controls' own visible box activates that control, not its neighbour", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/", { waitUntil: "load" });

  // 3px past each control's own rendered edge — deliberately small, unlike
  // the 44px-reach tests above, to answer a narrower question: does the
  // language toggle's hit area (or either button's) leak onto a neighbour
  // right at the boundary where the three controls actually meet.
  const searchW = (await visibleBox(page, ".nav-search-btn"))!.w;
  const themeW = (await visibleBox(page, ".nav-theme-btn"))!.w;
  const linkBox = (await visibleBox(page, ".nav-lang-link"))!;

  expect(
    await hitAt(page, ".nav-search-btn", -(searchW / 2 + 3), 0),
    "just outside .nav-search-btn's free (outward) edge",
  ).toBe("nav-search-btn");
  expect(
    await hitAt(page, ".nav-theme-btn", themeW / 2 + 3, 0),
    "just outside .nav-theme-btn's free (outward) edge",
  ).toBe("nav-theme-btn");
  expect(
    await hitAt(page, ".nav-lang-link", -(linkBox.w / 2 + 3), 0),
    "just outside .nav-lang-link's left edge, toward .nav-search-btn",
  ).toBe("nav-lang-link");
  expect(
    await hitAt(page, ".nav-lang-link", linkBox.w / 2 + 3, 0),
    "just outside .nav-lang-link's right edge, toward .nav-theme-btn",
  ).toBe("nav-lang-link");
  expect(
    await hitAt(page, ".nav-lang-link", 0, -(linkBox.h / 2 + 3)),
    "just outside .nav-lang-link's top edge",
  ).toBe("nav-lang-link");
  expect(
    await hitAt(page, ".nav-lang-link", 0, linkBox.h / 2 + 3),
    "just outside .nav-lang-link's bottom edge",
  ).toBe("nav-lang-link");
});

test("the two toggles' hit areas paint nothing — no fill, no border, no outline of their own", async ({
  page,
}) => {
  await page.goto("/", { waitUntil: "load" });
  for (const sel of [".nav-theme-btn", ".nav-search-btn", ".nav-lang-link"]) {
    const before = await page.evaluate((s) => {
      const cs = getComputedStyle(document.querySelector(s)!, "::before");
      return {
        background: cs.backgroundColor,
        borderStyle: cs.borderStyle,
        outlineStyle: cs.outlineStyle,
      };
    }, sel);
    expect(before.background, `${sel}::before must be transparent`).toBe(
      "rgba(0, 0, 0, 0)",
    );
    expect(before.borderStyle, `${sel}::before must paint no border`).toBe(
      "none",
    );
    expect(before.outlineStyle, `${sel}::before must paint no outline`).toBe(
      "none",
    );
  }
});

test("dark theme does not move either toggle's hit area", async ({
  page,
}) => {
  await page.goto("/", { waitUntil: "load" });
  await setTheme(page, "dark");
  expect(
    await page.evaluate(() =>
      document.documentElement.getAttribute("data-theme"),
    ),
  ).toBe("dark");

  expect(await hitAt(page, ".nav-theme-btn", 20, 0)).toBe("nav-theme-btn");
  expect(await hitAt(page, ".nav-search-btn", -20, 0)).toBe("nav-search-btn");
  expect(await hitAt(page, ".nav-theme-btn", -19, 0)).toBe("nav-theme-btn");
  expect(await hitAt(page, ".nav-search-btn", 19, 0)).toBe("nav-search-btn");
});

test(".moss-nav-island-sections reaches a 44px hit area on all four sides without moving the 32px icon", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/field-reports/long-form-dispatch/", { waitUntil: "load" });
  // nav-island.ts reveals the island only on an UPWARD scroll past the
  // masthead (see the comment on .moss-heading-anchor's scroll-padding in
  // site.css for the same scroll-direction rule) — scroll down, then up.
  await scrollAndSettle(page, 400);
  await scrollAndSettle(page, 200);
  await expect(page.locator(".moss-nav-island")).toHaveAttribute(
    "data-shown",
    "true",
  );

  const box = await visibleBox(page, ".moss-nav-island-sections");
  expect(box!.w, "visible width").toBeCloseTo(32, 0);
  expect(box!.h, "visible height").toBeCloseTo(32, 0);

  // .moss-nav-island-actions holds this button alone (no neighbour to
  // split with — see its contract entry), and .moss-nav-island-bar's own
  // min-height/padding leave exactly 6px of slack on every side, so all
  // four directions reach the full 20px.
  for (const [name, dx, dy] of [
    ["right", 20, 0],
    ["left", -20, 0],
    ["up", 0, -20],
    ["down", 0, 20],
  ] as const) {
    expect(
      await hitAt(page, ".moss-nav-island-sections", dx, dy),
      `${name}: 20px from .moss-nav-island-sections' centre must still hit it, ` +
        "not the decorative .moss-nav-island-progress rule riding its bottom edge",
    ).toBe("moss-nav-island-sections");
  }
});

test(".font-trigger reaches a 44px hit area without moving the pill or overlapping the heading above it", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/field-reports/long-form-dispatch/", { waitUntil: "load" });

  const box = await visibleBox(page, ".font-trigger");
  expect(box, ".font-trigger must exist on a dated article page").not.toBeNull();
  // The visible pill is short (~29px) and close to 44px wide already; only
  // the height needed real headroom, which `.date-line`'s own 24px clearance
  // to the H1 above supplies.
  expect(box!.h).toBeLessThan(32);

  for (const [name, dx, dy] of [
    ["up", 0, -20],
    ["down", 0, 20],
    ["left", -20, 0],
    ["right", 20, 0],
  ] as const) {
    const hit = await hitAt(page, ".font-trigger", dx, dy);
    expect(
      hit,
      `${name}: 20px from .font-trigger's centre must still hit it`,
    ).toContain("font-trigger");
  }
});

test(".mobile-menu-button reaches a 44px hit area below the hamburger breakpoint", async ({
  page,
}) => {
  // .mobile-menu-button only renders `display: block` under the 20rem
  // (320px) hamburger breakpoint (site.css) — neither 1440 nor 390 shows it
  // at all, so this is the one control in the set that needs its own,
  // narrower viewport to be reachable.
  await page.setViewportSize({ width: 300, height: 700 });
  await page.goto("/", { waitUntil: "load" });

  const box = await visibleBox(page, ".mobile-menu-button");
  expect(box!.w, "visible width").toBeCloseTo(32, 0);
  expect(box!.h, "visible height").toBeCloseTo(32, 0);

  // .nav-right's own gap (24px, --moss-space-md) clears both .nav-left and
  // .nav-icons well past the 44px box, so all four directions reach 20px.
  for (const [name, dx, dy] of [
    ["right", 20, 0],
    ["left", -20, 0],
    ["up", 0, -20],
    ["down", 0, 20],
  ] as const) {
    expect(
      await hitAt(page, ".mobile-menu-button", dx, dy),
      `${name}: 20px from .mobile-menu-button's centre must still hit it`,
    ).toBe("mobile-menu-button");
  }
});

test(".font-trigger and .font-pill button share the header's own focus-visible ring, not the browser default", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/field-reports/long-form-dispatch/", { waitUntil: "load" });

  async function outlineOf(sel: string) {
    await page.locator(sel).focus();
    return page.locator(sel).evaluate((el) => {
      const cs = getComputedStyle(el);
      return {
        color: cs.outlineColor,
        style: cs.outlineStyle,
        width: cs.outlineWidth,
        offset: cs.outlineOffset,
      };
    });
  }

  const reference = await outlineOf(".nav-theme-btn");
  expect(reference.style, "sanity: the reference control must have a ring at all").toBe(
    "solid",
  );

  const trigger = await outlineOf(".font-trigger");
  expect(trigger, ".font-trigger's ring must match .nav-theme-btn's exactly").toEqual(
    reference,
  );

  // Open with the keyboard, not a click: theme.ts's own open() focuses a
  // pill button as part of the SAME handler that opens the pill, and a
  // browser's focus-visible heuristic tracks input modality page-wide — a
  // real mouse click anywhere flips it to "mouse" and the very next
  // programmatic focus() (this one included) then renders with no ring,
  // browser default included, regardless of what CSS asks for. That is
  // correct default behaviour for a mouse user and exactly why this test
  // stays on the keyboard for its whole run.
  await page.locator(".font-trigger").focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".font-pill")).toHaveClass(/visible/);
  // .font-pill's own open transition (opacity/transform) runs 280ms
  // (site.css); re-focusing a still-animating element mid-transition landed
  // the focus back on .font-trigger instead in earlier runs of this gate.
  await page.waitForTimeout(300);

  const pillButton = await outlineOf('.font-pill button[data-scale=""]');
  expect(
    pillButton,
    ".font-pill button's ring must match .nav-theme-btn's exactly",
  ).toEqual(reference);
});
