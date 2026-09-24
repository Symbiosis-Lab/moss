// Blueprint placeholder — the ONE placeholder for an <img> that fails to load,
// whether the reference is broken for good (deleted/renamed/typo'd, never
// registered in the AssetRegistry) or the bytes just haven't been produced yet
// (a background WebP encode, an ffmpeg thumbnail).
//
// PREVIEW ONLY. Implementation: crates/moss-build/src/ops/serve/js/asset-placeholder.js,
// injected into <head> by the desktop app's preview server, which also
// carries the `.moss-img-fallback` marker outline. A published site has none
// of this: it can never contain a broken image, because moss refuses to
// deploy one.
//
// TWO INVARIANTS LIVE HERE, and only a real engine can check either:
//
//  1. IDENTITY SURVIVES. The placeholder swaps `src` on the SAME <img> rather
//     than replacing it. An earlier version replaced the failed <img>/<picture>
//     with a new <span>, silently dropping class/id/attributes and breaking
//     every context-specific CSS rule keyed on the tag (.site-logo,
//     .moss-hero img, .moss-card-cover > img) — precisely the cases the
//     placeholder exists to cover.
//
//  2. IT IS REVERSIBLE. When the asset lands, the real image must paint with NO
//     RELOAD. An earlier version dropped `srcset` and DELETED the <picture>'s
//     <source> children, which left the preview's URL-keyed asset swap
//     nothing to match: a pending image that errored once stayed a blueprint
//     grid until the user reloaded by hand, however many asset-ready events
//     arrived. jsdom cannot answer this — it does not fetch, decode, or run
//     source-set selection — so the assertion belongs here.
//
// Reads the BUILT bundle, which is what the preview server injects verbatim.
// The injection point itself is pinned by a Rust test elsewhere — the
// assertion below only guards against this harness testing an empty or wrong
// file.
import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { mossBuildAssets, openCrateDir } from '../../support/crate-paths';

const CSS = fs.readFileSync(path.join(mossBuildAssets(), 'css/site.css'), 'utf8');
const SHELL_HTML = fs.readFileSync(
  path.join(mossBuildAssets(), 'templates/shell.html'), 'utf8');
// The marker outline the preview server injects alongside the script. Kept in
// sync by hand with ASSET_PLACEHOLDER_STYLE in preview/iframe_bridge.rs — and by
// `the_placeholder_brings_its_own_marker_style`, which fails if it disappears.
const PLACEHOLDER_CSS = `
.moss-img-fallback{outline:1px solid rgba(20,60,130,.25);outline-offset:-1px}
[data-theme="dark"] .moss-img-fallback{outline-color:rgba(90,155,255,.3)}`;
const PLACEHOLDER_JS = fs.readFileSync(
  path.join(openCrateDir('moss-build'), 'src/ops/serve/js/asset-placeholder.js'), 'utf8');

function pageHtml(body) {
  // Mirrors what the preview server serves: the site's own CSS, plus the
  // placeholder's style and script injected at the top of <head>.
  return `<!doctype html><html><head><meta charset="utf-8"><style>${PLACEHOLDER_CSS}</style><script>${PLACEHOLDER_JS}</script>
<style>${CSS}</style>
</head><body>${body}</body></html>`;
}

// `about:invalid` is a reserved, never-fetchable pseudo-URL (WHATWG URL spec)
// that both Chromium and WebKit fail immediately and offline — a deterministic
// <img> `error` with no network dependency. The recovery tests below need a
// routable URL instead, so they use ORIGIN + a Playwright route.
const BROKEN_SRC = 'about:invalid#missing-image';

// Real, decodable bytes per format, one per extension the fixture serves —
// bytes and extension have to agree: a `<source type="image/webp">` served PNG bytes
// fires `load` with naturalWidth 0 in Chromium — a silent decode failure that
// looks exactly like a broken restore. (Cost an hour; hence this comment.)
const REAL_BYTES = {
  // 1×1 red PNG.
  '.png': Buffer.from(
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC',
    'base64'),
  // 4×4 red lossy WebP, straight out of a real encoder (libwebp via PIL).
  //
  // NOT the 50-byte TRANSPARENT_WEBP_1X1 that `preview/server/placeholder.rs`
  // serves: WebKit renders that one (naturalWidth 1) but REJECTS
  // `img.decode()` on it, apparently because it is a fully-transparent
  // alpha-only lossless image. Chromium decodes it fine. That is a fixture
  // problem, not a product one — nothing in moss calls `decode()` — but it
  // makes the liveness check below fail in webkit only.
  '.webp': Buffer.from(
    'UklGRjoAAABXRUJQVlA4IC4AAACQAQCdASoEAAQAAUAmJaACdLoAA5gA/vCbQ/4DdfFtMv/ucD/uyf/2yf+pAAAA',
    'base64'),
};
REAL_BYTES['.jpg'] = REAL_BYTES['.png'];

const MIME = { '.png': 'image/png', '.webp': 'image/webp', '.jpg': 'image/png' };

/** Pick decodable bytes + a matching Content-Type for a request path. */
function bytesFor(pathname) {
  const ext = Object.keys(REAL_BYTES).find((e) => pathname.endsWith(e)) ?? '.png';
  return { body: REAL_BYTES[ext], contentType: MIME[ext] };
}

/** Wait for the placeholder script's async DOM mutation to settle. */
async function settle(page) {
  await page.waitForTimeout(150);
}

/**
 * Assert `locator` is painting real decoded bytes from a real URL.
 *
 * Deliberately NOT `naturalWidth > 0`, for two reasons that each produced a
 * misleading result while writing these tests:
 *   - the blueprint data-URI is itself a valid image, so naturalWidth > 0 passes
 *     while the restore is completely broken;
 *   - `naturalWidth` is DENSITY-CORRECTED. An image selected as a srcset's
 *     `1600w` candidate at ~800 CSS px of layout reports intrinsic width / 2, so
 *     a small fixture rounds to 0 even though it decoded perfectly.
 * `decode()` rejects on a decode failure and is density-independent.
 */
async function expectRealBytes(locator, message) {
  await expect.poll(
    () => locator.evaluate(async (el) => {
      if (el.currentSrc.startsWith('data:')) return 'still-placeholder';
      try { await el.decode(); } catch { return 'decode-failed'; }
      return 'painted';
    }),
    { message },
  ).toBe('painted');
}

test('the placeholder is preview-only and the harness runs the real bundle', async () => {
  // Not browser assertions, but they belong next to the harness they protect.
  // The first two are the whole point of this change: a published page carries
  // no placeholder, so the shell and site.css must be clean. The last two stop
  // every test below from passing against an empty or wrong file.
  expect(SHELL_HTML).not.toContain('moss-img-fallback');
  expect(CSS).not.toContain('moss-img-fallback');
  expect(PLACEHOLDER_JS).toContain('moss-img-fallback');
  expect(PLACEHOLDER_JS).toContain('__mossAssetPlaceholder');
});

// ---------------------------------------------------------------- identity

test('a broken bare <img> gets its own src swapped, not replaced by a new element', async ({ page }) => {
  await page.setContent(pageHtml(
    `<img id="img" src="${BROKEN_SRC}" width="400" height="300" alt="A cat">`));
  await settle(page);

  const img = page.locator('#img');
  await expect(img).toHaveCount(1); // still the SAME <img>, not replaced
  await expect(img).toHaveClass(/moss-img-fallback/);
  await expect(img).toHaveAttribute('alt', 'A cat');
  await expect(img).toHaveAttribute('width', '400');
  await expect(img).toHaveAttribute('height', '300');

  const [naturalWidth, complete, src] = await img.evaluate(
    el => [el.naturalWidth, el.complete, el.src]);
  expect(naturalWidth).toBeGreaterThan(0); // a real, successfully-decoded image now
  expect(complete).toBe(true);
  expect(src.startsWith('data:image/svg+xml')).toBe(true);
});

test('a broken <img> in <picture> neutralizes the <source> but keeps the element', async ({ page }) => {
  // A <picture>'s <source> outranks the <img>'s own src in source-set
  // selection, so the blueprint would never be chosen while a matching
  // `<source srcset>` is live. Moving `srcset` to `data-moss-ph-srcset`
  // neutralizes it AND keeps the undo to a single attribute write. Deleting the
  // element — what this used to do — is what made the swap unrecoverable.
  await page.setContent(pageHtml(
    `<picture id="pic"><source srcset="${BROKEN_SRC}" type="image/webp"><img id="img" src="${BROKEN_SRC}" width="800" height="600" alt="Cover"></picture>`));
  await settle(page);

  await expect(page.locator('#pic')).toHaveCount(1);
  await expect(page.locator('#pic > source')).toHaveCount(1); // NOT removed
  await expect(page.locator('#pic > source[srcset]')).toHaveCount(0); // but inert
  await expect(page.locator('#pic > source')).toHaveAttribute(
    'data-moss-ph-srcset', BROKEN_SRC);

  const img = page.locator('#img');
  await expect(img).toHaveClass(/moss-img-fallback/);
  const [naturalWidth, complete] = await img.evaluate(el => [el.naturalWidth, el.complete]);
  expect(naturalWidth).toBeGreaterThan(0); // the blueprint really painted
  expect(complete).toBe(true);
});

test('a broken site-logo <img> keeps its class, empty alt, and aria-hidden (no width/height at all)', async ({ page }) => {
  // ImageContext::SiteLogo (crates/moss-core/src/render/image.rs) emits
  // `<img class="site-logo" src="…" alt="" aria-hidden="true">` with NO
  // width/height — sizing comes entirely from `.site-logo` CSS. A
  // replace-the-element design would strip the class (breaking nav-bar sizing)
  // and turn a fully-hidden decorative logo into an AT-announced "image".
  await page.setContent(pageHtml(
    `<img id="logo" class="site-logo" src="${BROKEN_SRC}" alt="" aria-hidden="true">`));
  await settle(page);

  const logo = page.locator('#logo');
  await expect(logo).toHaveCount(1);
  await expect(logo).toHaveClass(/site-logo/);
  await expect(logo).toHaveClass(/moss-img-fallback/);
  await expect(logo).toHaveAttribute('aria-hidden', 'true');
  await expect(logo).toHaveAttribute('alt', '');
  expect(await logo.evaluate(el => el.complete)).toBe(true);
});

test('a broken card-cover <img> keeps the object-fit:cover styling from .moss-card-cover > img', async ({ page }) => {
  // `.moss-card-cover > img { object-fit: cover; … }` is a TAG selector — it
  // only matches an actual <img> in that exact position. Replacing the <img>
  // with a <span> would silently stop this rule (and .moss-hero img,
  // .moss-collection-cover img, …) from applying at all.
  await page.setContent(pageHtml(
    `<div class="moss-card-cover"><img id="cov" src="${BROKEN_SRC}" width="800" height="600" alt="cover"></div>`));
  await settle(page);

  const cov = page.locator('#cov');
  await expect(cov).toHaveCount(1);
  expect(await cov.evaluate(el => getComputedStyle(el).objectFit)).toBe('cover');
});

test('a successfully-loading image is left untouched', async ({ page }) => {
  const ok = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='10'%3E%3Crect width='10' height='10'/%3E%3C/svg%3E";
  await page.setContent(pageHtml(`<img id="img" src="${ok}" width="10" height="10" alt="fine">`));
  await settle(page);

  await expect(page.locator('#img')).toHaveCount(1);
  await expect(page.locator('#img')).not.toHaveClass(/moss-img-fallback/);
  expect(await page.locator('#img').evaluate(el => el.src)).toBe(ok);
});

test('many simultaneous broken images on one page (gallery case) all resolve without error', async ({ page }) => {
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));

  const N = 20;
  const imgs = Array.from({ length: N },
    (_, i) => `<img class="g" src="${BROKEN_SRC}" width="200" height="150" alt="photo ${i}">`
  ).join('');
  await page.setContent(pageHtml(`<div id="gallery">${imgs}</div>`));
  await settle(page);

  await expect(page.locator('#gallery img.moss-img-fallback')).toHaveCount(N);
  await expect(page.locator('#gallery img')).toHaveCount(N);
  expect(errors).toEqual([]);
});

// ------------------------------------------------------------- reversibility

const ORIGIN = 'http://moss-gate.test';

/**
 * Serve a harness page at ORIGIN where image requests 404 until `land()` is
 * called, after which they return real PNG bytes. This is the pending-asset
 * timeline: the page paints while the encoder is still working.
 */
async function servePending(page, body) {
  let landed = false;
  await page.route(`${ORIGIN}/**`, async (route) => {
    const url = new URL(route.request().url());
    if (url.pathname === '/') {
      return route.fulfill({ contentType: 'text/html', body: pageHtml(body) });
    }
    if (!landed) return route.fulfill({ status: 404, body: 'not yet' });
    return route.fulfill(bytesFor(url.pathname));
  });
  await page.goto(`${ORIGIN}/`);
  return () => { landed = true; };
}

/** Drive the same entry point iframe-bridge's applyAssetSwap calls. */
function restore(page, absPath) {
  return page.evaluate(
    (p) => window.__mossAssetPlaceholder.restore(p), absPath);
}

test('a pending <picture> ladder paints the real image after restore, with no reload', async ({ page }) => {
  // THE regression this whole change exists for. Note the event names a single
  // rung of a multi-candidate ladder — matching per candidate rather than
  // against the whole srcset attribute is what makes this find anything.
  const land = await servePending(page,
    `<picture id="pic">
       <source srcset="/assets/photo.w800.webp 800w, /assets/photo.w1600.webp 1600w" type="image/webp">
       <img id="img" src="/assets/photo.png" width="800" height="600" alt="Cover">
     </picture>`);
  await settle(page);

  const img = page.locator('#img');
  await expect(img).toHaveClass(/moss-img-fallback/);
  expect(await img.evaluate(el => el.src.startsWith('data:'))).toBe(true);

  // The encode lands, and the swap fires — no navigation, no reload.
  land();
  expect(await restore(page, '/assets/photo.w1600.webp')).toBe(1);

  await expect(img).not.toHaveClass(/moss-img-fallback/);
  await expect(page.locator('#pic > source[srcset]')).toHaveCount(1);
  await expectRealBytes(img, 'the real bytes must paint without a reload');
});

test('a pending bare <img> paints the real image after restore', async ({ page }) => {
  // The `.thumb.jpg` case the deleted thumb-swap.js used to own on its own.
  const land = await servePending(page,
    `<img id="img" src="/video/clip.thumb.jpg" width="400" height="300" alt="">`);
  await settle(page);

  await expect(page.locator('#img')).toHaveClass(/moss-img-fallback/);

  land();
  expect(await restore(page, '/video/clip.thumb.jpg')).toBe(1);

  await expect(page.locator('#img')).not.toHaveClass(/moss-img-fallback/);
  await expectRealBytes(page.locator('#img'), 'the restored thumbnail must paint');
});

test('restore leaves other pending images on their placeholder', async ({ page }) => {
  const land = await servePending(page,
    `<img id="a" src="/assets/a.webp" width="100" height="100" alt="">
     <img id="b" src="/assets/b.webp" width="100" height="100" alt="">`);
  await settle(page);

  land();
  expect(await restore(page, '/assets/a.webp')).toBe(1);

  await expect(page.locator('#a')).not.toHaveClass(/moss-img-fallback/);
  await expect(page.locator('#b')).toHaveClass(/moss-img-fallback/);
});

test('a restore whose bytes are still missing re-places the blueprint', async ({ page }) => {
  // A swap can fire ahead of the bytes. The placeholder must come back rather
  // than leaving the browser's broken-image icon exposed, and must stay
  // restorable for the next event.
  await servePending(page,
    `<img id="img" src="/assets/photo.webp" width="100" height="100" alt="">`);
  await settle(page);

  expect(await restore(page, '/assets/photo.webp')).toBe(1); // never landed
  await settle(page);

  const img = page.locator('#img');
  await expect(img).toHaveClass(/moss-img-fallback/);
  expect(await img.evaluate(el => el.src.startsWith('data:'))).toBe(true);
  expect(await restore(page, '/assets/photo.webp')).toBe(1); // still restorable
});

// ------------------------------------------------------------------- video

test('a broken <video> gets the grid as its poster, in a real media stack', async ({ page }) => {
  // The engine question jsdom cannot answer: does a <video> whose source fails
  // fire `error` at all, so the capture-phase listener ever sees it? (An <img>
  // does; a media element reports failures through its own error model, and
  // WebKit and Chromium disagree about plenty else in that model.) Without this,
  // the video half of the placeholder is proven only against a synthetic event.
  await page.setContent(pageHtml(
    `<video id="v" src="${BROKEN_SRC}" width="320" height="180"></video>`));
  await settle(page);

  const v = page.locator('#v');
  await expect(v).toHaveClass(/moss-img-fallback/);
  // The grid goes in `poster` — a <video> cannot show an SVG data-URI as media.
  expect(await v.evaluate((el) => el.getAttribute('poster') ?? '')).toContain('data:image/svg+xml');
  // And the dead URL is off the element, so the engine stops re-requesting it.
  expect(await v.evaluate((el) => el.hasAttribute('src'))).toBe(false);
  // Same element, same box: the poster fills the space the video would have.
  const box = await v.boundingBox();
  expect(box.width).toBeGreaterThan(0);
  expect(box.height).toBeGreaterThan(0);
});

test('a <video> advances past a failed <source>, and the placeholder does not stop it', async ({ page }) => {
  // Pins the engine fact that `build/media/video.rs`'s ladder comment rests on,
  // in the engines moss ships against. Unlike <picture>, a media element's
  // resource selection (WHATWG HTML 4.8.11.5) re-enters its search loop when a
  // candidate fails, so the dead first <source> is not the end of the story;
  // what moss cannot recover from is a failure AFTER HAVE_METADATA.
  //
  // The placeholder must stay out of the way, and does: the failure fires
  // `error` at the <source> element, and the capture-phase listener only acts on
  // HTMLImageElement / HTMLVideoElement targets.
  //
  // The bytes at clip.mp4 are not a decodable video and deliberately so — this
  // asserts which candidate the element COMMITTED to (`currentSrc` is set before
  // the fetch), not that it played, so the gate needs no codec the CI browsers
  // may not ship. The two engines reach it by different routes, both fine:
  // chromium returns "maybe" for the HLS type and really fetches the 404, webkit
  // returns "" and skips it on type alone.
  await page.route(`${ORIGIN}/**`, async (route) => {
    const url = new URL(route.request().url());
    if (url.pathname === '/') {
      return route.fulfill({ contentType: 'text/html', body: pageHtml(
        `<video id="v" muted playsinline width="320" height="180">
           <source src="/video/clip.hls/master.m3u8" type="application/vnd.apple.mpegurl">
           <source src="/video/clip.mp4" type="video/mp4">
         </video>`) });
    }
    if (url.pathname === '/video/clip.mp4') {
      return route.fulfill({ contentType: 'video/mp4', body: Buffer.from('not a decodable mp4') });
    }
    return route.fulfill({ status: 404, body: 'no ladder' });
  });
  await page.goto(`${ORIGIN}/`);

  const v = page.locator('#v');
  await expect.poll(() => v.evaluate((el) => el.currentSrc))
    .toBe(`${ORIGIN}/video/clip.mp4`);

  // Exhausting the children is NOT an element error: networkState parks at
  // NETWORK_NO_SOURCE and `video.error` stays null. Anything that watched
  // `video.error` to notice a broken ladder would never fire.
  //
  // Polled, because the claim is where it PARKS, not how fast it gets there:
  // WebKitGTK reads NETWORK_LOADING (2) at the instant `currentSrc` is set
  // and only settles to 3 once GStreamer has rejected the bytes (CI, 2026-09-19).
  await expect.poll(() => v.evaluate((el) => el.networkState), { timeout: 15_000 }).toBe(3);
  const after = await v.evaluate((el) => ({
    networkState: el.networkState,
    error: el.error === null,
    grid: el.classList.contains('moss-img-fallback'),
  }));
  expect(after.networkState).toBe(3); // NETWORK_NO_SOURCE
  expect(after.error).toBe(true);
  expect(after.grid).toBe(false); // the placeholder never saw a <video> error
});

test("a converting video's real poster survives the placeholder", async ({ page }) => {
  // moss generates a thumbnail poster, which can be a good frame of a video
  // whose .mp4 is still converting. The blueprint borrows that slot and gives it
  // back — losing it would replace a real frame with a grid permanently.
  //
  // Routable 404 rather than BROKEN_SRC: restore() matches on a site-relative
  // path, and `about:invalid#…` has no path to normalize to.
  await servePending(page,
    `<video id="v" src="/video/clip.mp4" poster="/video/clip.thumb.jpg" width="320" height="180"></video>`);
  await settle(page);
  await expect(page.locator('#v')).toHaveClass(/moss-img-fallback/);

  // Read the result in the SAME task as the restore. Not paranoia: the bytes
  // still 404 here, so the engine fires `error` again the moment it retries the
  // restored URL, and the placeholder goes straight back on — which is the next
  // assertion. Polling for the in-between state would race that.
  const after = await page.evaluate(() => {
    const restored = window.__mossAssetPlaceholder.restore('/video/clip.mp4');
    const v = document.getElementById('v');
    return {
      restored,
      poster: v.getAttribute('poster'),
      src: v.getAttribute('src'),
      grid: v.classList.contains('moss-img-fallback'),
    };
  });
  expect(after.restored).toBe(1);
  expect(after.poster).toBe('/video/clip.thumb.jpg'); // the real frame is back
  expect(after.src).toContain('/video/clip.mp4?_t='); // cache-busted, so it refetches
  expect(after.grid).toBe(false);

  // Still missing on the retry, so the grid returns — and returns ONCE, over
  // the author's poster rather than over a previous grid. A placeholder that
  // stashed its own output here would lose the real frame on the second failure.
  await settle(page);
  await expect(page.locator('#v')).toHaveClass(/moss-img-fallback/);
  expect(await page.evaluate(
    () => document.getElementById('v').getAttribute('data-moss-ph-poster'),
  )).toBe('/video/clip.thumb.jpg');
});
