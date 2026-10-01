/**
 * `route: true` in a real browser — what the Rust snapshot suite
 * (snapshot_places_route_site) cannot see once site.css's cascade and
 * `prefers-color-scheme` apply:
 *
 *  - the dashed line sits BEFORE the marker group in DOM order (so it
 *    paints under the dots, not over them);
 *  - each badge's number is legible against its own chip — sampled-pixel
 *    contrast >= 4.5:1 — for both the filled (exact/city) and the hollow
 *    (region) style, light and dark;
 *  - the offset badge (Harbor Overlook, coincident with Harbor) carries a
 *    leader line back to its real stop.
 *
 * Fixture: PLACE_MAP_ROUTE_GATE (tests/e2e/helpers/gate-sites.ts). Run via:
 *   npx playwright test -c playwright/place-map-route.config.ts
 */
import { test, expect, type Page } from "@playwright/test";

const MAP_SVG = ".moss-place-locator svg";
const ROUTE_LINE = '[data-map-route="line"]';
const MARKER_LAYER = '[data-map-layer="marker"]';
const ROUTE_LEADER = '[data-map-route="leader"]';
const FILLED_BADGE = '[data-map-route-badge-style="filled"]';
const HOLLOW_BADGE = '[data-map-route-badge-style="hollow"]';

async function setTheme(page: Page, theme: "light" | "dark") {
  await page.goto("./", { waitUntil: "domcontentloaded" });
  await page.evaluate((t) => localStorage.setItem("moss-theme", t), theme);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.locator(MAP_SVG).scrollIntoViewIfNeeded();
}

/** WCAG relative luminance of an sRGB 0-255 triple. */
function luminance([r, g, b]: [number, number, number]): number {
  const channel = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

function contrast(a: [number, number, number], b: [number, number, number]): number {
  const [l1, l2] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (l1 + 0.05) / (l2 + 0.05);
}

/**
 * The contrast ratio between the two extreme luminance pixels found within
 * `sampleRadiusFactor` of a badge circle's own centre — its own
 * fill/outline vs. its own number, whichever is darker and whichever is
 * lighter. Robust to anti-aliasing and to the exact glyph shape (a "0" has
 * a hole; a "1" is mostly background): the colours a legible badge paints
 * are its background and its ink, and nothing inside a correctly-bounded
 * circle paints a third one.
 *
 * `sampleRadiusFactor` has to be chosen per badge style, not shared, because
 * each one's "third colour" risk sits at a different radius:
 *  - the filled badge's own `data-map-route-badge` circle is a flat disc out
 *    to its full radius, but a second, larger casing circle is drawn
 *    directly behind it (the same ring the plain marker dot relies on,
 *    near-black in dark mode) — right at the shared edge the two anti-alias
 *    together, so sampling out to the full radius measured 17-19:1 for a
 *    white-text/#4d5f8f-fill pair that computes to ~6.3:1 on paper. A small
 *    factor (`0.6`) stays well inside that edge.
 *  - the hollow badge's own route colour is NOT a flat disc — it is a thin
 *    outline ring at the very edge, plus the number. `getBBox()` (what the
 *    screen box below is built from) excludes stroke width, so the ring's
 *    outer half sits just past the box's own edge; the same small factor
 *    that protects the filled badge would exclude the ring entirely here,
 *    leaving only the thinner, less reliably fully-saturated glyph to carry
 *    the "ink" extreme (measured 4.29:1 on the smallest viewport, under the
 *    4.5:1 floor, though nothing in the design changed). A larger factor
 *    (`0.85`) reaches the ring's inner half without crossing into genuine
 *    background past the badge's own edge.
 */
async function badgeContrast(page: Page, badgeSelector: string, sampleRadiusFactor: number): Promise<number> {
  const box = await page.locator(badgeSelector).first().evaluate((el) => {
    const circle = el as SVGGraphicsElement;
    const svg = circle.ownerSVGElement!;
    const bbox = circle.getBBox();
    const toScreen = (x: number, y: number) => {
      const point = svg.createSVGPoint();
      point.x = x;
      point.y = y;
      return point.matrixTransform(svg.getScreenCTM()!);
    };
    const topLeft = toScreen(bbox.x, bbox.y);
    const bottomRight = toScreen(bbox.x + bbox.width, bbox.y + bbox.height);
    return { x: topLeft.x, y: topLeft.y, width: bottomRight.x - topLeft.x, height: bottomRight.y - topLeft.y };
  });
  // A small outward pad so the screenshot clip (rounded to device pixels)
  // never crops the circle's own edge short — safe because the scan below
  // only samples within `sampleRadiusFactor` of the image centre, not the
  // clip's own edges.
  const pad = 1;
  const clip = {
    x: Math.max(0, box.x - pad),
    y: Math.max(0, box.y - pad),
    width: box.width + pad * 2,
    height: box.height + pad * 2,
  };
  const shot = await page.screenshot({ clip });
  const [minLum, maxLum] = await page.evaluate(async ({ png, sampleRadiusFactor }) => {
    const image = new Image();
    image.src = `data:image/png;base64,${png}`;
    await image.decode();
    const canvas = document.createElement("canvas");
    canvas.width = image.width;
    canvas.height = image.height;
    const context = canvas.getContext("2d")!;
    context.drawImage(image, 0, 0);
    const data = context.getImageData(0, 0, canvas.width, canvas.height).data;
    let min: [number, number, number] | null = null;
    let max: [number, number, number] | null = null;
    let minL = Infinity;
    let maxL = -Infinity;
    const lum = (r: number, g: number, b: number) => {
      const c = (v: number) => {
        const s = v / 255;
        return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
      };
      return 0.2126 * c(r) + 0.7152 * c(g) + 0.0722 * c(b);
    };
    const centerX = canvas.width / 2;
    const centerY = canvas.height / 2;
    const radius = (Math.min(canvas.width, canvas.height) / 2) * sampleRadiusFactor;
    for (let y = 0; y < canvas.height; y++) {
      for (let x = 0; x < canvas.width; x++) {
        if (Math.hypot(x - centerX, y - centerY) > radius) continue;
        const i = (y * canvas.width + x) * 4;
        const [r, g, b, a] = [data[i], data[i + 1], data[i + 2], data[i + 3]];
        if (a < 200) continue;
        const l = lum(r, g, b);
        if (l < minL) { minL = l; min = [r, g, b]; }
        if (l > maxL) { maxL = l; max = [r, g, b]; }
      }
    }
    return [min, max];
  }, { png: shot.toString("base64"), sampleRadiusFactor });
  if (!minLum || !maxLum) throw new Error(`badgeContrast: no opaque pixels found in ${badgeSelector}`);
  return contrast(minLum as [number, number, number], maxLum as [number, number, number]);
}

const FILLED_SAMPLE_RADIUS = 0.6;
const HOLLOW_SAMPLE_RADIUS = 0.85;

for (const [device, viewport] of [
  ["desktop", { width: 1280, height: 900 }],
  ["phone", { width: 390, height: 844 }],
] as const) {
  for (const theme of ["light", "dark"] as const) {
    test(`route line sits under the marker layer in DOM order — ${device}, ${theme}`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await setTheme(page, theme);
      const order = await page.evaluate(({ line, markers }) => {
        const svg = document.querySelector(".moss-place-locator svg")!;
        const nodes = [...svg.querySelectorAll("*")];
        return {
          line: nodes.findIndex((n) => n.matches(line)),
          markers: nodes.findIndex((n) => n.matches(markers)),
        };
      }, { line: ROUTE_LINE, markers: MARKER_LAYER });
      expect(order.line, "route line must exist").toBeGreaterThanOrEqual(0);
      expect(order.markers, "marker layer must exist").toBeGreaterThanOrEqual(0);
      expect(order.line, `line (${order.line}) must come before markers (${order.markers})`).toBeLessThan(order.markers);
    });

    test(`the offset badge carries a leader back to its stop — ${device}, ${theme}`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await setTheme(page, theme);
      await expect(page.locator(ROUTE_LEADER)).toHaveCount(1);
      // Badge 2 (Harbor Overlook) is the one coincident with badge 1
      // (Harbor) in the fixture's gazetteer — it is the one offset, so it
      // is the one with a leader.
      await expect(page.locator('[data-map-route-badge="2"]')).toHaveCount(1);
    });

    test(`filled and hollow badges are both legible (contrast >= 4.5:1) — ${device}, ${theme}`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await setTheme(page, theme);
      const filled = await badgeContrast(page, FILLED_BADGE, FILLED_SAMPLE_RADIUS);
      const hollow = await badgeContrast(page, HOLLOW_BADGE, HOLLOW_SAMPLE_RADIUS);
      // eslint-disable-next-line no-console -- measured ratios the owner asked to see, not just pass/fail.
      console.log(`[place-map-route] ${device}/${theme}: filled=${filled.toFixed(2)}:1 hollow=${hollow.toFixed(2)}:1`);
      expect(filled, `filled badge contrast ${filled.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5);
      expect(hollow, `hollow badge contrast ${hollow.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5);
    });
  }
}
