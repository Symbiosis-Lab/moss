/**
 * Render gate: the media-collection lightbox's close/prev/next buttons get
 * a visible, ROUND focus ring on a real 44px+ square box (not the tall
 * narrow rectangle their bare "×"/"‹"/"›" glyph used to lay out as), and the
 * GitHub link's hover/focus shape matches its round mark instead of painting
 * a square behind it.
 *
 * All three need a real engine:
 *
 *   1. `.lightbox-close`/`.lightbox-nav` had no `:focus-visible` rule at
 *      all, so what actually painted was the browser's OWN default — only a
 *      real engine renders that default to compare against. Opened with
 *      Enter, not a click — see the fixture comment in gate-sites.ts for why
 *      a click first would poison the result.
 *   2. `.github-link`'s hover fill is `background` with no `border-radius`;
 *      whether that paints a circle or a square behind the round mark is a
 *      rendered-box question, not something the CSS text alone answers.
 *   3. Whether the three lightbox buttons' boxes are actually SQUARE (not
 *      just both ≥44px) and whether their `outline` — which follows
 *      `border-radius` — really stays clear of the box's own corners is a
 *      layout-and-paint question: `cornerPixelRGB` screenshots a tiny clip
 *      at each corner and decodes it via canvas, the same technique
 *      place-map-tokens.spec.ts uses to measure a rendered dot's size.
 *
 * Run via:
 *   npx playwright test -c playwright/lightbox-github-shapes.config.ts
 */
import { test, expect, type Page } from "@playwright/test";

async function outlineOf(page: Page, sel: string) {
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

test("the lightbox close button gets a visible ring when reached by keyboard, not the browser default", async ({
  page,
}) => {
  await page.goto("/videos/", { waitUntil: "load" });

  // Real keyboard path: the media tile is a tabindex="0" <figure>; Enter
  // opens it. A mouse click here would still open the lightbox, but it
  // would also flip the page's input-modality heuristic, and every focus
  // after that — even this test's own explicit .focus() calls — would then
  // resolve :focus-visible false regardless of what the CSS says, which is
  // a false reading of the fixture, not of the control.
  await page.locator('[data-type="video"]').first().focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".lightbox:not([hidden])")).toHaveCount(1);

  // .focus(), not a Tab walk: WebKit (matching real Safari without the OS
  // "Full Keyboard Access" setting most people never turn on) leaves plain
  // `<button>`s out of the Tab order entirely — Tab here cycles the two
  // native <video> players and never reaches .lightbox-close at all, in
  // either this fixture or a real site. That is a platform default outside
  // any of these fixes; what the CSS controls, and what this asserts, is
  // the ring a button that DOES receive keyboard focus gets.
  await page.locator(".lightbox-close").focus();
  expect(
    await page.evaluate(() => document.activeElement?.matches(":focus-visible")),
    "a keyboard-focused button must be :focus-visible",
  ).toBe(true);

  const outline = await outlineOf(page, ".lightbox-close");
  expect(outline.style, "must paint a real ring, not the browser default").toBe("solid");
  // rgba(255, 255, 255, 0.6) — see the CSS comment for why white rather than
  // --moss-color-ui-accent: this overlay is always near-black, regardless
  // of site theme, the same reasoning the immersive iframe's own
  // fullscreen/new-window buttons already used.
  expect(outline.color).toBe("rgba(255, 255, 255, 0.6)");
  expect(outline.width).toBe("2px");
});

test("the lightbox close button paints no ring at rest or on a plain hover", async ({
  page,
}) => {
  await page.goto("/videos/", { waitUntil: "load" });
  await page.locator('[data-type="video"]').first().click();
  await expect(page.locator(".lightbox:not([hidden])")).toHaveCount(1);

  const rest = await outlineOf(page, ".lightbox-close");
  expect(rest.style).toBe("none");

  await page.hover(".lightbox-close");
  const hover = await outlineOf(page, ".lightbox-close");
  expect(hover.style, "hover alone (mouse, no keyboard) must not show the ring").toBe(
    "none",
  );
});

test("the GitHub link's hover fill and focus ring are round, matching its mark", async ({
  page,
}) => {
  await page.goto("/", { waitUntil: "load" });

  const radius = await page
    .locator(".github-link")
    .evaluate((el) => getComputedStyle(el).borderRadius);
  expect(radius, ".github-link must carve a radius, not paint a square behind a round mark").not.toBe(
    "0px",
  );

  // A real mouse action in an earlier test of this file (or anywhere else
  // in this worker's shared browser process — Chromium's input-modality
  // heuristic is not reliably scoped per page/context under CDP automation)
  // can leave the NEXT page's first .focus() reading :focus-visible false
  // even with no click of its own. One throwaway keyboard event resets it.
  await page.keyboard.press("Shift");
  await page.locator(".nav-theme-btn").focus();
  const refOutline = await outlineOf(page, ".nav-theme-btn");
  expect(refOutline.style, "sanity: the reference control must have a ring at all").toBe(
    "solid",
  );

  await page.locator(".github-link").focus();
  const linkOutline = await outlineOf(page, ".github-link");
  expect(
    linkOutline,
    ".github-link's ring must match .nav-theme-btn's exactly, not the browser default",
  ).toEqual(refOutline);
});

/** Average RGB of a small clip centred on one corner of `box`, decoded via canvas (mirrors place-map-tokens.spec.ts's own pixel-sampling technique). */
async function cornerPixelRGB(
  page: Page,
  box: { left: number; top: number; right: number; bottom: number },
  corner: "tl" | "tr" | "bl" | "br",
): Promise<[number, number, number]> {
  const size = 6;
  const x = (corner.includes("l") ? box.left : box.right) - size / 2;
  const y = (corner.includes("t") ? box.top : box.bottom) - size / 2;
  const shot = await page.screenshot({ clip: { x, y, width: size, height: size } });
  return page.evaluate(async (png) => {
    const image = new Image();
    image.src = `data:image/png;base64,${png}`;
    await image.decode();
    const canvas = document.createElement("canvas");
    canvas.width = image.width;
    canvas.height = image.height;
    const context = canvas.getContext("2d")!;
    context.drawImage(image, 0, 0);
    const data = context.getImageData(0, 0, canvas.width, canvas.height).data;
    let r = 0, g = 0, b = 0, n = 0;
    for (let i = 0; i < data.length; i += 4) {
      r += data[i]; g += data[i + 1]; b += data[i + 2]; n += 1;
    }
    return [r / n, g / n, b / n] as [number, number, number];
  }, shot.toString("base64"));
}

test("each lightbox button is a real circle: at least 44x44, a radius that makes it round, and a focus ring that doesn't reach the box's corners", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/videos/", { waitUntil: "load" });

  // Keyboard path throughout, like the first test in this file — a mouse
  // click anywhere in this process can flip the page's input-modality
  // heuristic and make every later .focus() read :focus-visible false.
  await page.locator('[data-type="video"]').first().focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".lightbox:not([hidden])")).toHaveCount(1);

  for (const sel of [".lightbox-close", ".lightbox-prev", ".lightbox-next"]) {
    const box = await page.locator(sel).evaluate((el) => {
      const r = el.getBoundingClientRect();
      return { left: r.left, top: r.top, right: r.right, bottom: r.bottom, w: r.width, h: r.height };
    });
    expect(box.w, `${sel} width`).toBeGreaterThanOrEqual(44);
    expect(box.h, `${sel} height`).toBeGreaterThanOrEqual(44);
    // Square, not just two independently-large dimensions — border-radius:
    // 50% on a non-square box draws an oval, the same bug at a bigger size.
    expect(box.w, `${sel} must be square for border-radius: 50% to draw a circle`).toBeCloseTo(
      box.h,
      0,
    );

    const radius = await page.locator(sel).evaluate((el) => getComputedStyle(el).borderRadius);
    expect(radius, `${sel} must carve a radius`).not.toBe("0px");

    // Rest: no ring painted anywhere, corners included.
    const restCorners = await Promise.all(
      (["tl", "tr", "bl", "br"] as const).map((c) => cornerPixelRGB(page, box, c)),
    );

    await page.locator(sel).focus();
    expect(
      await page.evaluate(() => document.activeElement?.matches(":focus-visible")),
      `${sel} must be :focus-visible after a real keyboard focus`,
    ).toBe(true);

    const focusCorners = await Promise.all(
      (["tl", "tr", "bl", "br"] as const).map((c) => cornerPixelRGB(page, box, c)),
    );

    (["tl", "tr", "bl", "br"] as const).forEach((corner, i) => {
      const [rr, rg, rb] = restCorners[i];
      const [fr, fg, fb] = focusCorners[i];
      const diff = Math.abs(rr - fr) + Math.abs(rg - fg) + Math.abs(rb - fb);
      expect(
        diff,
        `${sel}'s ${corner} corner must read the same focused as at rest — a ring inscribed ` +
          "in a circle stays well clear of its square box's corners",
      ).toBeLessThan(24);
    });

    // Blur (not Escape, which would close the lightbox) before the next
    // control's own "at rest" baseline.
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  }
});
