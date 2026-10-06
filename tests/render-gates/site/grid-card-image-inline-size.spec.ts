/**
 * A grid-card image must fill the card's inline axis, in both writing modes.
 *
 * The two whole-cell link-card shapes take different code paths to the same
 * result:
 *  - the internal cell (`a.moss-grid-card[data-kind="link"]`) fills via
 *    `.moss-grid-card :is(.moss-image, picture, img) { inline-size: 100% }`
 *    (site.css) — a logical property, so it reads as the card's inline axis
 *    in either writing mode.
 *  - the external cell (`a.moss-card[data-external]`) has no
 *    `.moss-grid-card` figure at all; its cover is `.moss-card-cover`, a flex
 *    item that stretches to the card's cross (inline) axis and whose `> img`
 *    fills it with physical `width: 100%; height: 100%` inside an
 *    `aspect-ratio` box — same end result, reached through flex stretch
 *    instead of a logical `inline-size` declaration.
 *
 * The source image here is deliberately tinier (40×30) than any card, so
 * nothing about the assertion depends on a real `sizes="auto"` lazy fetch
 * resolving inside the test — a fix that only caps an OVERSIZED image
 * (`max-width: 100%`, already there before either rule) would never show a
 * regression, because a cap never stretches a small source up.
 *
 * Under vertical typesetting (`writing-mode: vertical-rl`) the inline axis
 * is the box's physical HEIGHT, not its width — and the base rule the
 * internal cell's image has to outrank (`article figure:not(.video-figure)
 * img`) sets a PHYSICAL `height: auto`, which is the inline axis there. Both
 * pages are checked here, and an `expect.soft` per card means a regression
 * on either card shape still shows every card that broke, not just the
 * first.
 *
 * Site: tests/e2e/helpers/gate-sites.ts → GRID_CARD_IMAGE_INLINE_SIZE_GATE, a
 * dedicated site rather than a reuse of GRID_MOBILE_COLLAPSE_GATE: that one
 * turns `implicit_figure` off to test the plain-`<p>` shape, and this gate
 * needs a real `<figure class="moss-image">` around the image.
 */
import { test, expect, type Page } from "@playwright/test";

const DESKTOP = { width: 1280, height: 900 };

/** The card's own inline-axis content size (border-box inline size minus
 * inline padding), and its image's rendered inline-axis size — "inline"
 * meaning whichever physical axis `writing-mode` currently maps it to. */
async function cardImageInlineSizes(page: Page, gridSelector: string) {
  return page.$$eval(`${gridSelector} > :is(a.moss-grid-card, a.moss-card)`, async (cards) => {
    // Every cell image here is `loading="lazy"`: the page's `load` event
    // does not wait for it, so measuring right after `goto()` races a fetch
    // that may not have started. `decode()` waits for the resource itself,
    // the same fix `grid-mobile-collapse.spec.ts`'s `cardGeometry` uses.
    await Promise.all(cards.map((card) => (card.querySelector("img") as HTMLImageElement).decode()));
    return cards.map((card) => {
      const cs = getComputedStyle(card);
      const vertical = cs.writingMode.startsWith("vertical");
      const cardRect = card.getBoundingClientRect();
      const img = card.querySelector("img") as HTMLImageElement;
      const imgRect = img.getBoundingClientRect();
      const inlinePadding = vertical
        ? parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom)
        : parseFloat(cs.paddingLeft) + parseFloat(cs.paddingRight);
      return {
        isExternal: card.hasAttribute("data-external"),
        cardInline: (vertical ? cardRect.height : cardRect.width) - inlinePadding,
        imgInline: vertical ? imgRect.height : imgRect.width,
      };
    });
  });
}

for (const [page_, url] of [
  ["horizontal", "/"],
  ["vertical", "/vertical/"],
] as const) {
  test(`external and internal image-link cards both fill the card's inline size (${page_})`, async ({
    page,
  }) => {
    await page.setViewportSize(DESKTOP);
    await page.goto(url);
    const cards = await cardImageInlineSizes(page, ".moss-grid");

    expect(cards).toHaveLength(2);
    expect(cards.map((c) => c.isExternal)).toEqual([true, false]);
    for (const { isExternal, cardInline, imgInline } of cards) {
      const label = isExternal ? "external" : "internal";
      expect.soft(imgInline, `${label} card's image must render, not collapse to 0`).toBeGreaterThan(0);
      expect
        .soft(imgInline / cardInline, `${label} card's image should fill ~all of the card's inline size`)
        .toBeGreaterThanOrEqual(0.9);
    }
  });
}

/**
 * A grid-card caption is card copy — the same kind of text as
 * `.moss-card-title` — not a photo credit under a full-width plate, so it
 * should turn with the rest of a vertical-typesetting page instead of
 * following `site/vertical.css`'s `figure figcaption { writing-mode:
 * horizontal-tb }` exception, which exists for ordinary in-article figures.
 *
 * Since bbb3c0a5 only the internal whole-cell link card
 * (`a.moss-grid-card[data-kind="link"]`) still carries a `<figcaption>` at
 * all — its body is the author's own image markdown, rendered through the
 * ordinary figure path. The external card (`a.moss-card[data-external]`,
 * bbb3c0a5's unified shell) has no figcaption; its title lives in
 * `.moss-card-title`, an ordinary card-copy span with no horizontal-tb
 * carve-out of its own, so it is checked here instead — same claim
 * (card copy turns with the page), same regression this gate is for,
 * carried by the shape that now exists rather than the one that was
 * deleted. The standalone figure appended to the vertical fixture page is
 * the control — an ordinary article figure the horizontal exception still
 * applies to, unaffected by either grid-card shape.
 */
test("grid-card copy runs vertical; an ordinary figure's caption still runs horizontal (vertical)", async ({
  page,
}) => {
  await page.setViewportSize(DESKTOP);
  await page.goto("/vertical/");

  const cardCaptions = await page.$$eval(".moss-grid > a.moss-grid-card[data-kind='link'] figcaption", (nodes) =>
    nodes.map((node) => {
      const rect = node.getBoundingClientRect();
      return { writingMode: getComputedStyle(node).writingMode, width: rect.width, height: rect.height };
    }),
  );
  expect(cardCaptions).toHaveLength(1);
  for (const { writingMode, width, height } of cardCaptions) {
    expect.soft(writingMode).toBe("vertical-rl");
    expect.soft(height, "a vertical caption's box should read taller than wide").toBeGreaterThan(width);
  }

  const externalTitle = await page.$eval(
    ".moss-grid > a.moss-card[data-external] .moss-card-title",
    (node) => {
      const rect = node.getBoundingClientRect();
      return { writingMode: getComputedStyle(node).writingMode, width: rect.width, height: rect.height };
    },
  );
  expect.soft(externalTitle.writingMode).toBe("vertical-rl");
  expect
    .soft(externalTitle.height, "a vertical title's box should read taller than wide")
    .toBeGreaterThan(externalTitle.width);

  const standaloneWritingMode = await page.$eval(
    "article > figure.moss-image > figcaption",
    (node) => getComputedStyle(node).writingMode,
  );
  expect(standaloneWritingMode).toBe("horizontal-tb");
});

/**
 * `.moss-grid[data-scroll] > .moss-grid-card { min-inline-size: 0;
 * scroll-snap-align: start }` used to name only ONE of the shapes a scroll
 * row's direct children actually take. A whole-cell link card and a
 * generated link preview both render their own `a.moss-grid-card` (site.css
 * governs them already), but a cell whose bare link names a page in the
 * build is replaced wholesale by that page's `a.moss-card` — no
 * `.moss-grid-card` wrapper at all (`grid_cells.rs`'s `card_markup`) — so it
 * fell through the selector and kept the grid item's default `auto` minimum
 * size and no scroll-snap stop. `scroll-row.md`'s row mixes all three shapes
 * over three columns so it actually scrolls; every direct child must show
 * the same snap behavior regardless of which one it is.
 */
test("a scroll row's page-card cells get the same snap treatment as its .moss-grid-card ones", async ({
  page,
}) => {
  await page.setViewportSize(DESKTOP);
  await page.goto("/scroll-row/");

  const row = page.locator(".moss-grid[data-scroll]");
  await expect(row).toHaveAttribute("data-columns", "3");

  const cells = await row.evaluate((el) =>
    Array.from(el.children).map((child) => ({
      className: child.className,
      scrollSnapAlign: getComputedStyle(child).scrollSnapAlign,
    })),
  );

  expect(cells).toHaveLength(4);
  const pageCard = cells.find((c) => c.className.split(" ").includes("moss-card"));
  expect(pageCard, "the bare link to /about/ should render as a direct a.moss-card child").toBeTruthy();
  for (const { className, scrollSnapAlign } of cells) {
    expect.soft(scrollSnapAlign, `${className} should snap`).toBe("start");
  }
});

const MOBILE = { width: 390, height: 844 };

/**
 * `overflow-x: auto` on `.moss-grid[data-scroll]` only clips a descendant
 * positioned against the ROW's own containing block. A theme's visually
 * hidden `figcaption` (`position: absolute` with no positioned ancestor of
 * its own) is instead positioned against the initial containing block, so
 * its static position — far to the right for a card late in a long row —
 * escaped the row's clip entirely and widened the whole document, not just
 * the row. `scroll-overflow.md` mixes ten cards with
 * SCROLL_OVERFLOW_THEME_CSS's caption hider (see gate-sites.ts) to force
 * exactly that.
 */
for (const viewport of [DESKTOP, MOBILE] as const) {
  test(`a scroll row's hidden captions do not widen the page (${viewport.width}px)`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await page.goto("/scroll-overflow/");

    const row = page.locator(".moss-grid[data-scroll]");
    const rowMetrics = await row.evaluate((el) => ({ scrollWidth: el.scrollWidth, clientWidth: el.clientWidth }));
    expect(rowMetrics.scrollWidth, "the row itself should still scroll").toBeGreaterThan(rowMetrics.clientWidth);

    const docMetrics = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }));
    expect(
      docMetrics.scrollWidth,
      "a scroll row's hidden captions must not widen the whole page",
    ).toBeLessThanOrEqual(docMetrics.clientWidth);
  });
}

/**
 * Under vertical typesetting the row transposes (site/vertical.css): it
 * scrolls along the block axis, physical height, instead. But the page's OWN
 * scroll axis stays physically horizontal either way (columns run
 * right-to-left), so an escaped caption is still a page-width regression
 * here, not a page-height one — the same fix, checked on the axis that
 * actually matters for this typesetting mode.
 */
test("a scroll row's hidden captions do not widen a vertical-typesetting page", async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await page.goto("/scroll-overflow-vertical/");

  const row = page.locator(".moss-grid[data-scroll]");
  const rowMetrics = await row.evaluate((el) => ({ scrollHeight: el.scrollHeight, clientHeight: el.clientHeight }));
  expect(
    rowMetrics.scrollHeight,
    "the row itself should still scroll along its own (block) axis",
  ).toBeGreaterThan(rowMetrics.clientHeight);

  const docMetrics = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    clientWidth: document.documentElement.clientWidth,
  }));
  expect(
    docMetrics.scrollWidth,
    "a vertical page's own scroll axis is still physically horizontal; hidden captions must not widen it",
  ).toBeLessThanOrEqual(docMetrics.clientWidth);
});

/**
 * Owner's rule: a `{scroll}` row whose cards already fit its column count
 * (`scroll-row-fits.md`, 3 cells over `:::grid 3`) renders exactly like the
 * plain grid on a screen wide enough to show every card, and only becomes a
 * real scroller once the viewport narrows past the SAME breakpoint that
 * collapses a plain grid to one column — never pinned to one behavior or
 * the other. `data-fits` (moss-core's `GridShortcode::fits_without_scrolling`)
 * is what the stylesheet keys the wide-screen layout off, and
 * `scroll-row.ts`'s `fit()` is what pulls the row in and out of the tab
 * order and toggles its dots to match.
 */
test("a fitting scroll row renders as a plain grid at 1280px: equal tracks, no overflow, no dots, no tab stop", async ({
  page,
}) => {
  await page.setViewportSize(DESKTOP);
  await page.goto("/scroll-row-fits/");

  const row = page.locator(".moss-grid[data-scroll]");
  await expect(row).toHaveAttribute("data-fits", "");
  await expect(row).toHaveAttribute("data-columns", "3");

  const metrics = await row.evaluate((el) => ({
    scrollWidth: el.scrollWidth,
    clientWidth: el.clientWidth,
    tabIndex: (el as HTMLElement).tabIndex,
    widths: Array.from(el.children).map((c) => c.getBoundingClientRect().width),
  }));
  expect(metrics.scrollWidth, "no overflow to scroll").toBeLessThanOrEqual(metrics.clientWidth + 1);
  expect(metrics.tabIndex, "not a pointless tab stop when nothing scrolls").toBe(-1);
  expect(metrics.widths).toHaveLength(3);
  for (const w of metrics.widths) expect(w).toBeCloseTo(metrics.widths[0], 0);

  // The dots exist in the DOM (built for any row with 2+ cards) but stay
  // `hidden` once `fit()` sees the row isn't actually scrollable — "no
  // dots" is a visibility claim, not a DOM-absence one.
  await expect(page.locator(".moss-scroll-dots")).toBeHidden();

  // "Renders exactly like the plain grid" is a box-model claim too, not just
  // a track-width one: `.moss-grid[data-scroll]`'s always-on `padding-block:
  // 4px` exists to stop the row's own overflow clip from cutting a focus
  // ring (site.css's comment on that rule) — a reason that stops applying
  // the moment this same media query sets `overflow-x: visible` back. A
  // plain `.moss-grid` carries no padding at all, so a fitting row left at
  // 4px is 8px taller than its plain twin and insets its cards 4px further
  // from the top/bottom edges than `align-items: flex-start` would if the
  // padding had actually gone.
  const padding = await row.evaluate((el) => {
    const cs = getComputedStyle(el);
    return { top: cs.paddingTop, bottom: cs.paddingBottom };
  });
  expect(padding, "no padding leftover from the always-scrolling rule").toEqual({
    top: "0px",
    bottom: "0px",
  });
});

test("the same fitting scroll row becomes a slideshow at 390px: ~1.3 cards, scrollable, 3 dots, focusable", async ({
  page,
}) => {
  await page.setViewportSize(MOBILE);
  await page.goto("/scroll-row-fits/");

  const row = page.locator(".moss-grid[data-scroll]");
  const metrics = await row.evaluate((el) => ({
    scrollWidth: el.scrollWidth,
    clientWidth: el.clientWidth,
    tabIndex: (el as HTMLElement).tabIndex,
    firstCardWidth: (el.children[0] as HTMLElement).getBoundingClientRect().width,
  }));
  expect(metrics.scrollWidth, "narrow screens scroll").toBeGreaterThan(metrics.clientWidth);
  expect(metrics.tabIndex, "a real scroller is keyboard reachable").toBe(0);
  // ~1.3 cards per view: the first card should read narrower than the full
  // row width, wide enough that roughly a third of a second card peeks.
  expect(metrics.firstCardWidth / metrics.clientWidth).toBeLessThan(0.85);

  const dots = page.locator(".moss-scroll-dots button");
  await expect(dots).toHaveCount(3);
  await expect(page.locator(".moss-scroll-dots")).toBeVisible();
});

test("the fitting scroll row's vertical-typesetting twin also fits at 1280px", async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await page.goto("/scroll-row-fits-vertical/");

  const row = page.locator(".moss-grid[data-scroll]");
  await expect(row).toHaveAttribute("data-fits", "");

  const metrics = await row.evaluate((el) => ({
    scrollHeight: el.scrollHeight,
    clientHeight: el.clientHeight,
    tabIndex: (el as HTMLElement).tabIndex,
  }));
  expect(metrics.scrollHeight, "no overflow along the transposed (block) axis").toBeLessThanOrEqual(
    metrics.clientHeight + 1,
  );
  expect(metrics.tabIndex, "not a pointless tab stop when nothing scrolls").toBe(-1);
  await expect(page.locator(".moss-scroll-dots")).toBeHidden();
});

// WebKit rasterizes a transform-scaled dot at its unscaled size and
// resamples it, so scale()-sized dots rendered lumpy on Retina. Every dot,
// in every state, must be an unscaled square of an even whole-pixel size
// (which also centers it on whole pixels inside its 24px cell).
test("a scroll row's page-control dots are unscaled even-pixel squares in every state", async ({ page }) => {
  await page.goto("/scroll-dots-dynamic/");
  const nav = page.locator(".moss-scroll-dots");
  await expect(nav).toHaveAttribute("data-indicator", "dynamic");
  const dots = await nav.evaluate((el) =>
    [...el.querySelectorAll("button")].map((b) => {
      const cs = getComputedStyle(b, "::before");
      return { cls: b.className, current: b.getAttribute("aria-current"), w: cs.width, h: cs.height, t: cs.transform };
    }),
  );
  expect(dots.some((d) => d.cls.includes("is-edge-end"))).toBe(true);
  expect(dots.some((d) => d.current === "true")).toBe(true);
  for (const d of dots) {
    expect(d.t, `transform on ${JSON.stringify(d)}`).toBe("none");
    expect(d.w, `square ${JSON.stringify(d)}`).toBe(d.h);
    const px = parseFloat(d.w);
    expect(Number.isInteger(px) && px % 2 === 0, `even whole px ${JSON.stringify(d)}`).toBe(true);
  }
});
