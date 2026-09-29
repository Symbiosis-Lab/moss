/**
 * The preview server's own injections must never change a page's LAYOUT
 * relative to the static build it is a live view of.
 *
 * The preview server (`moss-cli build <folder> --serve`,
 * `inject_preview_assets` in crates/moss-build/src/ops/serve/iframe_bridge.rs)
 * adds CSS and script the static build never ships — the divider-drag
 * cheap-reflow style, the comment-form preview shim, the asset-placeholder
 * script, and more. All of it is meant to be invisible: a page should look
 * and measure the same in the preview iframe as it does published. It has
 * not always been:
 *
 *  - `PREVIEW_CHEAP_REFLOW_STYLE` sets `content-visibility: auto` plus a
 *    `contain-intrinsic-size: auto none auto 600px` fallback on article
 *    media, so an off-screen figure skips layout cheaply during a divider
 *    drag. A `:::grid N {scroll}` row lays its cards on ONE shared grid row
 *    track (`grid-auto-flow: column`); a card currently outside the row's own
 *    visible area fell back to the 600px placeholder HEIGHT, and because the
 *    cards share one track, that height became every card's height, not just
 *    the off-screen one — measured once as a 669px preview row against a
 *    269px published one. Fixed by scoping the selector away from
 *    `.moss-grid[data-scroll]` content entirely (be18a723).
 *  - The one-value form of the same rule (`auto 600px`, applying to BOTH
 *    axes) made an off-screen figure report a 600px intrinsic WIDTH too;
 *    inside a `.moss-grid` whose tracks are `repeat(N, 1fr)`, that width
 *    became each track's automatic minimum and blew a 3-column grid out to
 *    600px per column. Fixed by moving to the two-value form (`auto none
 *    auto 600px` — see the const's own doc comment in iframe_bridge.rs).
 *  - Other preview-only padding/geometry regressions of the same shape have
 *    shown up before as dead space that only ever existed in the preview.
 *
 * All three are one class of bug: something scoped "preview-only" leaks into
 * layout math the static build never runs. Rather than pin each incident by
 * name, this gate compares EVERY element's box, on a page deliberately built
 * from the shapes that class of bug has hit — a big scroll row, a wrapping
 * grid, standalone figures, a table embed, a hero, and enough filler prose
 * that later media start off-screen — between the two servers, in both
 * engines, at desktop and mobile widths, AFTER a full scroll pass.
 *
 * Only after, not before: measured directly while writing this gate, a
 * page's FIRST paint diverges from the static build on every off-screen
 * article figure, wrapping-grid cell, and embed, not just scroll-row cards —
 * `content-visibility: auto`'s placeholder height is showing on anything
 * that has never been laid out yet, which is the feature working as
 * designed (it is what makes a divider-drag cheap), not a bug. A full
 * top-to-bottom, every-scroller pass is what a real visit eventually
 * amounts to; measuring once everything has had a chance to be laid out at
 * least once is the point at which "the same as static" is an honest claim
 * to check. The be18a723 regression this gate was built to catch is a real
 * counter-example that still shows up at that point: the shared grid-row
 * track's last few cards (the ones that land off-screen again once the row
 * scrolls back to rest) never picked up a correct height — confirmed by
 * reverting be18a723's selector change locally, rebuilding moss-cli, and
 * running this gate: it failed naming exactly those cards, on both engines.
 *
 * Both servers build the SAME source fixture
 * (tests/e2e/helpers/gate-sites.ts → PREVIEW_PARITY_GATE): the static one is
 * `buildScratchSite`'s usual throwaway `python3 -m http.server` over a real
 * `moss-cli build`; the preview one is the real `moss-cli build --serve`.
 * Neither is a stand-in for the other — comparing them IS the test.
 *
 * Ports: playwright/gate-ports.ts, keys `preview-parity-gate:static` /
 * `:preview`. Config: playwright/preview-parity-gate.config.ts.
 *
 *   npx playwright test -c playwright/preview-parity-gate.config.ts
 */
import { test, expect, type Page } from "@playwright/test";
import { gatePort } from "../../../playwright/gate-ports";

const STATIC_BASE = `http://localhost:${gatePort("preview-parity-gate:static")}/`;
const PREVIEW_BASE = `http://localhost:${gatePort("preview-parity-gate:preview")}/`;

const VIEWPORTS = [
  { name: "desktop", width: 1280, height: 800 },
  { name: "mobile", width: 390, height: 844 },
] as const;

// Every id/class/attribute `inject_preview_assets` (and the two middleware
// steps around it — `inject_shell_frame_class`, `inject_placeholder_into_head`)
// can add to a served page, enumerated from
// crates/moss-build/src/ops/serve/iframe_bridge.rs. This gate hits the
// preview server directly with no `__moss_shell` query param, so
// `shell_mounted` is false and the owner-controls / shell-frame branches
// never fire in practice — the selector still lists them, so a future call
// site that DOES set the marker stays excluded rather than silently failing
// this gate on chrome that was never meant to be compared.
const PREVIEW_ONLY_SELECTOR = [
  "#moss-bridge",
  "#moss-comment-preview-shim",
  "#moss-comment-owner-style",
  "#moss-comment-owner-controls",
  "#moss-img-fallback-style",
  "#moss-img-fallback",
  "style#moss-preview-tooltip",
  "style#moss-preview-cheap-reflow",
  "style#moss-preview-slot-reveal",
  ".moss-shell-frame",
  ".moss-fm-flash",
  "[data-moss-hide-comment]",
].join(", ");

interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface BoxSnapshot {
  boxes: Record<string, Box>;
  excludedPaths: string[];
}

/**
 * Every element's box inside `<main>` (moss places the hero OUTSIDE `<main>`
 * for full-width display — see shell.rs — so it is not measured directly
 * here, but it still occupies space above `<main>` and any preview-only
 * effect on it would shift everything below), keyed by a DOM path stable
 * across the two servers (tag + position among same-tag siblings, from
 * `<main>` down — not id/class, which a future preview injection could add
 * harmlessly). An `<img>`'s path carries its `alt` text so a failing diff
 * names the card, not just a position.
 */
async function measureBoxes(page: Page): Promise<BoxSnapshot> {
  return page.evaluate((excludeSelector) => {
    const main = document.querySelector("main");
    if (!main) return { boxes: {}, excludedPaths: [] };

    function isExcluded(el: Element): boolean {
      let node: Element | null = el;
      while (node && node !== main) {
        if (node.matches(excludeSelector)) return true;
        node = node.parentElement;
      }
      return false;
    }

    function pathFor(el: Element): string {
      const parts: string[] = [];
      let node: Element | null = el;
      while (node && node !== main) {
        const parent: Element | null = node.parentElement;
        if (!parent) break;
        const sameTag = Array.from(parent.children).filter((c) => c.tagName === node!.tagName);
        const idx = sameTag.indexOf(node) + 1;
        const alt = node.tagName === "IMG" ? node.getAttribute("alt") : null;
        const label = alt ? `${node.tagName.toLowerCase()}[alt=${JSON.stringify(alt)}]` : `${node.tagName.toLowerCase()}:nth-of-type(${idx})`;
        parts.unshift(label);
        node = parent;
      }
      return parts.join(" > ") || "main";
    }

    const boxes: Record<string, { x: number; y: number; w: number; h: number }> = {};
    const excludedPaths: string[] = [];
    const round = (n: number) => Math.round(n * 100) / 100;

    const rootBox = main.getBoundingClientRect();
    boxes["main"] = { x: round(rootBox.x), y: round(rootBox.y), w: round(rootBox.width), h: round(rootBox.height) };

    for (const el of Array.from(main.querySelectorAll("*"))) {
      const path = pathFor(el);
      if (isExcluded(el)) {
        excludedPaths.push(path);
        continue;
      }
      const r = el.getBoundingClientRect();
      boxes[path] = { x: round(r.x), y: round(r.y), w: round(r.width), h: round(r.height) };
    }
    return { boxes, excludedPaths };
  }, PREVIEW_ONLY_SELECTOR);
}

/**
 * Top-to-bottom the whole page, then left-to-right-to-left every horizontal
 * scroller under `<main>` (`.moss-grid[data-scroll]`), one frame per step —
 * exactly the interaction that flips `content-visibility: auto` elements
 * between skipped and rendered, which is the state this gate is checking
 * for. Ends back at rest (scrollTop 0, every scroller's scrollLeft 0) so the
 * "settled" measurement reads like a page nobody has touched yet, the same
 * state a first-time visitor's browser would report.
 */
async function scrollDance(page: Page): Promise<void> {
  await page.evaluate(async () => {
    const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    const doc = document.documentElement;
    const stepY = Math.max(200, window.innerHeight * 0.8);
    for (let y = 0; y <= doc.scrollHeight; y += stepY) {
      window.scrollTo(0, y);
      await frame();
    }
    window.scrollTo(0, doc.scrollHeight);
    await frame();

    const scrollers = Array.from(document.querySelectorAll("main [data-scroll]")) as HTMLElement[];
    for (const el of scrollers) {
      const maxX = el.scrollWidth - el.clientWidth;
      if (maxX <= 0) continue;
      const stepX = Math.max(100, el.clientWidth * 0.8);
      for (let x = 0; x <= maxX; x += stepX) {
        el.scrollLeft = x;
        await frame();
      }
      el.scrollLeft = maxX;
      await frame();
      for (let x = maxX; x >= 0; x -= stepX) {
        el.scrollLeft = x;
        await frame();
      }
      el.scrollLeft = 0;
      await frame();
    }
    window.scrollTo(0, 0);
    await frame();

    // A big scroll row's dot indicator (scroll-row.ts's `buildDots`, active
    // past MAX_DOT_COUNT cards) reacts to an IntersectionObserver, which
    // fires asynchronously — not on the same frame as the `scrollLeft`
    // assignment above. Measuring immediately caught the two independent
    // page loads with the observer's callback still in flight, at slightly
    // different points, which read as a few px of drift confined entirely
    // to the dots: nothing to do with page layout, everything to do with
    // this wait being too short. A real timer tick (not just rAF) is what
    // lets a macrotask-queued observer callback run before the boxes below
    // are read.
    await new Promise((resolve) => setTimeout(resolve, 200));
    await frame();
  });
}

// `build_style_tag()` (iframe_bridge.rs) hides the native scrollbar on every
// preview response — deliberately: the app shell paints its own fake
// scrollbar over a translucent titlebar, and an unhidden native one would
// peek out from under it. A real visitor's browser never gets this rule, so
// the static build's scrollbar reserves its usual gutter width and the
// preview's does not — confirmed by running this fixture directly: WebKit
// (which renders a classic, space-reserving scrollbar) showed a uniform 3px
// horizontal offset on every single boxed element, static vs preview, with
// an otherwise-correct binary. Chromium's default headless scrollbar
// already reserves no width, so it never showed this — engine-dependent,
// not a bug either engine's numbers were pointing at. Applying the SAME
// scrollbar-hiding rule to the static page before measuring neutralizes the
// one difference this gate is not for, without touching anything the fix
// this gate guards actually changes.
const NEUTRALIZE_SCROLLBAR_CSS =
  "html{scrollbar-width:none}html::-webkit-scrollbar,body::-webkit-scrollbar{display:none;width:0;height:0}";

/** Navigate, then wait past both `networkidle` and every image's decode.
 *
 * A `loading="lazy"` image far down a page this long never starts fetching
 * at `networkidle` on its own, and calling `decode()` alone does not force
 * it either — confirmed by running this fixture directly: the first few
 * images (already near the initial viewport) decoded in 0ms, everything
 * past them timed out at 3s. Flipping `loading` to `"eager"` first is what
 * actually starts the fetch; `decode()` then resolves immediately once the
 * bytes are in. This is a page-load concern, unrelated to the
 * content-visibility skip/render state the scroll dance below exercises —
 * every image is fully decoded before either measurement. */
async function gotoReady(page: Page, base: string, path: string): Promise<void> {
  await page.goto(`${base}${path}`, { waitUntil: "networkidle" });
  await page.addStyleTag({ content: NEUTRALIZE_SCROLLBAR_CSS });
  await page.evaluate(async () => {
    const imgs = Array.from(document.images);
    imgs.forEach((img) => {
      img.loading = "eager";
    });
    await Promise.all(imgs.map((img) => (img.complete ? Promise.resolve() : img.decode().catch(() => {}))));
  });
}

/** Boxes within 1px on every axis count as a match — sub-pixel rounding
 * differences between two independent server processes are not the
 * regression this gate exists to catch. */
function diffBoxes(staticBoxes: Record<string, Box>, previewBoxes: Record<string, Box>): string[] {
  const diffs: string[] = [];
  const keys = new Set([...Object.keys(staticBoxes), ...Object.keys(previewBoxes)]);
  for (const key of Array.from(keys).sort()) {
    const a = staticBoxes[key];
    const b = previewBoxes[key];
    if (!a) {
      diffs.push(`${key}: present in preview only (w=${b.w} h=${b.h})`);
      continue;
    }
    if (!b) {
      diffs.push(`${key}: present in static only (w=${a.w} h=${a.h})`);
      continue;
    }
    const dx = Math.abs(a.x - b.x);
    const dy = Math.abs(a.y - b.y);
    const dw = Math.abs(a.w - b.w);
    const dh = Math.abs(a.h - b.h);
    if (dx > 1 || dy > 1 || dw > 1 || dh > 1) {
      diffs.push(
        `${key}: static={x:${a.x},y:${a.y},w:${a.w},h:${a.h}} preview={x:${b.x},y:${b.y},w:${b.w},h:${b.h}} ` +
          `(Δx=${dx.toFixed(1)} Δy=${dy.toFixed(1)} Δw=${dw.toFixed(1)} Δh=${dh.toFixed(1)})`,
      );
    }
  }
  return diffs;
}

/** Navigate, run the scroll dance, and measure — the one comparison point
 * this gate asserts on. See the file header for why first paint is not
 * measured too. */
async function captureSettled(page: Page, base: string): Promise<BoxSnapshot> {
  await gotoReady(page, base, "/");
  await scrollDance(page);
  return measureBoxes(page);
}

for (const viewport of VIEWPORTS) {
  test(`preview injections do not move main's layout after a full scroll pass (${viewport.name})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });

    const staticResult = await captureSettled(page, STATIC_BASE);
    const previewResult = await captureSettled(page, PREVIEW_BASE);

    // Reported separately, per the exclusion contract above — this gate
    // asserts on everything else, not on these.
    test.info().annotations.push({
      type: "preview-only elements excluded from comparison",
      description: previewResult.excludedPaths.length
        ? previewResult.excludedPaths.sort().join("; ")
        : "(none found in main)",
    });

    const diffs = diffBoxes(staticResult.boxes, previewResult.boxes);
    expect(diffs, `layout differs after a full scroll pass, static vs preview (${viewport.name})`).toEqual([]);
  });
}
