#!/usr/bin/env node
// I-fidelity (dissolve-module-design.md section 6, unit 2 step A): the
// painter registry's print, painted at p=0, must match a screenshot of the
// live members it captured, per grid cell, in colour. Runs against the
// standalone fixture (fixtures/watercolor-capture/), never against the
// landing page -- step B wires the landing's own capture through the same
// registry and runs this invariant again per scene.
//
// Every pixel comparison happens in Node, from raw bytes: the composite
// print is exported once as a PNG data URL (byte extraction, not
// comparison, happens in-page -- unavoidable, since only the page can ask a
// <canvas> for its own bytes) and the live comparison target is a real
// Playwright screenshot (already a Node-side PNG buffer). Both are decoded
// by the same small PNG decoder check-landing-invariants.mjs already
// carries (averagePixelPNG's rationale applies here word for word: decoding
// in Node has nothing left to race). No page-side pixel diff of any kind.
//
// WATERCOLOR_ABLATE=<kind> deletes that painter from the registry right
// before capture, for the red/green demonstration the ground rules require
// of every new assertion: run once with WATERCOLOR_ABLATE=img (RED, every
// img member's cells fail because the registry can no longer capture them),
// then unset it (GREEN).
import { loadPlaywright, launchChromium } from './landing-harness.mjs';
import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { inflateSync } from 'node:zlib';
import { extname, join, resolve as resolvePath, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = fileURLToPath(new URL('.', import.meta.url));
const WORKTREE_ROOT = resolvePath(HERE, '..');
const assert = (cond, msg) => { if (!cond) throw new Error(msg); };

const MIME = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.mp4': 'video/mp4', '.svg': 'image/svg+xml', '.woff2': 'font/woff2' };
// A dedicated server, not landing-harness's serveDirectory: that one always
// answers 200 with chunked transfer-encoding and no Content-Length, which
// every other check tolerates (nothing else here plays a <video>) but
// WebKit does not -- measured directly, isolated from this fixture: the
// same sample.mp4 reaches readyState>=2 within a second once the response
// carries Content-Length and Accept-Ranges, and never does without them
// (networkState stuck at NETWORK_NO_SOURCE). Root and Range support are
// scoped to this script's own server so the fix cannot change behaviour for
// the other fifteen check-landing-*.mjs scripts sharing serveDirectory.
async function serveWorktree(root) {
  const absRoot = resolvePath(root);
  const server = createServer(async (req, res) => {
    try {
      const url = new URL(req.url, 'http://localhost');
      const target = resolvePath(join(absRoot, decodeURIComponent(url.pathname)));
      if (target !== absRoot && !target.startsWith(absRoot + sep)) { res.writeHead(403); res.end(); return; }
      const st = await stat(target).catch(() => null);
      if (!st || st.isDirectory()) { res.writeHead(404); res.end('Not found'); return; }
      const type = MIME[extname(target)] || 'application/octet-stream';
      const range = req.headers.range && /^bytes=(\d*)-(\d*)$/.exec(req.headers.range);
      if (range) {
        const start = range[1] ? Number(range[1]) : 0, end = range[2] ? Number(range[2]) : st.size - 1;
        const body = await readFile(target);
        res.writeHead(206, { 'Content-Type': type, 'Content-Length': end - start + 1, 'Accept-Ranges': 'bytes', 'Content-Range': `bytes ${start}-${end}/${st.size}` });
        res.end(body.subarray(start, end + 1));
        return;
      }
      const body = await readFile(target);
      res.writeHead(200, { 'Content-Type': type, 'Content-Length': body.length, 'Accept-Ranges': 'bytes' });
      res.end(body);
    } catch (error) { res.writeHead(500); res.end(String(error && error.stack || error)); }
  });
  await new Promise((ok, no) => { server.once('error', no); server.listen(0, '127.0.0.1', ok); });
  const { port } = server.address();
  return { baseURL: `http://127.0.0.1:${port}/`, close: () => new Promise((ok) => server.close(ok)) };
}

// A minimal PNG decoder (8-bit, all five filter types) -- ported from
// check-landing-invariants.mjs's averagePixelPNG, generalized to return
// every pixel instead of one whole-image average, because I-fidelity needs
// a mean per grid cell, not a mean per image.
function decodePNG(buf) {
  if (buf.readUInt32BE(0) !== 0x89504e47) throw new Error('decodePNG: not a PNG');
  let offset = 8, width, height, bitDepth, colorType;
  const idatChunks = [];
  while (offset < buf.length) {
    const len = buf.readUInt32BE(offset);
    const type = buf.toString('ascii', offset + 4, offset + 8);
    const data = buf.subarray(offset + 8, offset + 8 + len);
    if (type === 'IHDR') { width = data.readUInt32BE(0); height = data.readUInt32BE(4); bitDepth = data.readUInt8(8); colorType = data.readUInt8(9); }
    else if (type === 'IDAT') idatChunks.push(data);
    else if (type === 'IEND') break;
    offset += 12 + len;
  }
  if (bitDepth !== 8) throw new Error(`decodePNG: unsupported bit depth ${bitDepth}`);
  const channels = { 0: 1, 2: 3, 3: 1, 4: 2, 6: 4 }[colorType];
  if (colorType === 3) throw new Error('decodePNG: palette PNGs unsupported (none of this script\'s sources produce one)');
  const raw = inflateSync(Buffer.concat(idatChunks));
  const stride = width * channels;
  const px = Buffer.alloc(height * stride);
  let rawOff = 0;
  for (let y = 0; y < height; y++) {
    const filter = raw[rawOff++];
    for (let x = 0; x < stride; x++) {
      const rawByte = raw[rawOff++];
      const a = x >= channels ? px[y * stride + x - channels] : 0;
      const up = y > 0 ? px[(y - 1) * stride + x] : 0;
      const c = (y > 0 && x >= channels) ? px[(y - 1) * stride + x - channels] : 0;
      let val;
      if (filter === 0) val = rawByte;
      else if (filter === 1) val = (rawByte + a) & 0xff;
      else if (filter === 2) val = (rawByte + up) & 0xff;
      else if (filter === 3) val = (rawByte + ((a + up) >> 1)) & 0xff;
      else if (filter === 4) { const p = a + up - c, pa = Math.abs(p - a), pb = Math.abs(p - up), pc = Math.abs(p - c); val = (rawByte + (pa <= pb && pa <= pc ? a : pb <= pc ? up : c)) & 0xff; }
      else throw new Error(`decodePNG: bad filter type ${filter}`);
      px[y * stride + x] = val;
    }
  }
  return { width, height, channels, pixels: px };
}

// Mean RGB over a device-pixel rect, gray images replicated into r=g=b so a
// caller never needs to branch on colour type.
function cellMean(img, x0, y0, w, h) {
  const { width, height, channels, pixels } = img;
  let r = 0, g = 0, b = 0, n = 0;
  const x1 = Math.min(width, x0 + w), y1 = Math.min(height, y0 + h);
  for (let y = Math.max(0, y0); y < y1; y++) {
    for (let x = Math.max(0, x0); x < x1; x++) {
      const i = (y * width + x) * channels;
      if (channels === 1 || channels === 2) { r += pixels[i]; g += pixels[i]; b += pixels[i]; }
      else { r += pixels[i]; g += pixels[i + 1]; b += pixels[i + 2]; }
      n++;
    }
  }
  if (!n) return null;
  return [r / n, g / n, b / n];
}

function dataUrlToBuffer(dataUrl) {
  const comma = dataUrl.indexOf(',');
  return Buffer.from(dataUrl.slice(comma + 1), 'base64');
}

// Measured 2026-09-21 (WATERCOLOR_FIDELITY_MEASURE=1 run, both engines, DPR
// 1 and 2, every expected-ok fixture member, 10 CSS-px cells): the ceiling
// is text-box's rotated clip-path edge -- 44.9 (chromium dpr1), 37.3
// (chromium dpr2), 47.2 (webkit dpr1), 37.9 (webkit dpr2) -- because that
// member's pixels are resampled twice (paintBox rasterizes it unrotated,
// then compositeOnto rotates the already-rasterized bitmap a second time),
// compounding edge antialiasing along a long diagonal in a way an
// axis-aligned image's rounded corner never does. Next highest is
// video-el's ~20-22, a real decode-path difference: drawImage(videoEl)
// and the engine's own on-screen video compositor do not run the same
// YUV-to-RGB conversion. Every other member measures under 15. Tolerance
// is set above the measured ceiling with headroom, per engine (WebKit's
// foreignObject rasterization already runs measurably hotter elsewhere in
// this codebase -- see check-landing-invariants.mjs -- so it gets its own
// constant here too, not one shared fudge).
const TOLERANCE = { chromium: 58, webkit: 62 };
const CELL = 10; // CSS px; fine enough that a missing member fails almost every one of its cells, not just its edges

async function runOnce(page, { ablate } = {}) {
  await page.waitForFunction(() => window.__fixtureReady === true || window.__fixtureError, null, { timeout: 20000 });
  const setupError = await page.evaluate(() => window.__fixtureError || null);
  if (setupError) throw new Error(`fixture setup failed: ${setupError}`);
  const result = await page.evaluate(async (ablateKind) => {
    if (ablateKind) delete window.WatercolorCapture.painters[ablateKind];
    const stage = document.getElementById('stage');
    const stageRect = stage.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const members = window.__fixtureManifest.map((m) => {
      const el = document.querySelector(`[data-capture-id="${m.id}"]`);
      const cs = getComputedStyle(el);
      // offsetLeft/offsetTop/offsetWidth/offsetHeight give the pre-transform
      // layout box (what drawPlates already relies on for rotation), but the
      // outer <svg> element does not implement the HTMLElement offset*
      // properties in this engine -- they read back undefined, not 0, which
      // silently became NaN sizes downstream. getBoundingClientRect(), minus
      // the stage's own origin, is the fallback for exactly that element;
      // nothing in this fixture transforms an <svg> member, so the
      // pre/post-transform distinction does not matter for it.
      const hasOffset = typeof el.offsetWidth === 'number';
      const box = hasOffset
        ? { x: el.offsetLeft, y: el.offsetTop, w: el.offsetWidth, h: el.offsetHeight }
        : (() => { const r = el.getBoundingClientRect(); return { x: r.left - stageRect.left, y: r.top - stageRect.top, w: r.width, h: r.height }; })();
      return {
        id: m.id,
        el,
        kind: m.kind,
        ...box,
        rotation: parseFloat(cs.getPropertyValue('--rot')) || 0,
        borderRadius: parseFloat(cs.borderRadius) || 0,
        zIndex: parseFloat(cs.zIndex) || 0,
      };
    });
    const captureResult = await window.WatercolorCapture.capture(members, { dpr });
    const dest = document.createElement('canvas');
    dest.width = Math.round(stageRect.width * dpr);
    dest.height = Math.round(stageRect.height * dpr);
    window.WatercolorCapture.compositeOnto(dest, captureResult, dpr);
    return {
      composite: dest.toDataURL('image/png'),
      results: captureResult.results,
      members: members.map((m) => ({ id: m.id, x: m.x, y: m.y, w: m.w, h: m.h })),
      stageRect: { width: stageRect.width, height: stageRect.height },
      dpr,
    };
  }, ablate || null);

  const compositeImg = decodePNG(dataUrlToBuffer(result.composite));
  const liveBuf = await page.locator('#stage').screenshot();
  const liveImg = decodePNG(liveBuf);
  return { ...result, compositeImg, liveImg };
}

// Which member (if any) a CSS-px point in stage-local coordinates falls
// inside -- gap/padding cells belong to no member and are skipped, since
// I-fidelity judges captured members against their own live pixels, not the
// stage's background between them.
function memberAt(members, cx, cy) {
  for (const m of members) if (cx >= m.x && cx < m.x + m.w && cy >= m.y && cy < m.y + m.h) return m.id;
  return null;
}

function judge(run, engineName) {
  const { results, members, stageRect, dpr, compositeImg, liveImg } = run;
  const byId = Object.fromEntries(results.map((r) => [r.id, r]));
  const tolerance = TOLERANCE[engineName];
  const failingCells = {}; // id -> count
  const maxDiff = {}; // id -> max
  for (let cy = 0; cy < stageRect.height; cy += CELL) {
    for (let cx = 0; cx < stageRect.width; cx += CELL) {
      const id = memberAt(members, cx + CELL / 2, cy + CELL / 2);
      if (!id) continue;
      const report = byId[id];
      if (!report.ok) continue; // declared failures are judged separately, below
      const x0 = Math.round(cx * dpr), y0 = Math.round(cy * dpr), w = Math.round(CELL * dpr), h = Math.round(CELL * dpr);
      const a = cellMean(compositeImg, x0, y0, w, h), b = cellMean(liveImg, x0, y0, w, h);
      if (!a || !b) continue;
      const diff = Math.max(Math.abs(a[0] - b[0]), Math.abs(a[1] - b[1]), Math.abs(a[2] - b[2]));
      maxDiff[id] = Math.max(maxDiff[id] || 0, diff);
      if (diff > tolerance) failingCells[id] = (failingCells[id] || 0) + 1;
    }
  }
  return { byId, failingCells, maxDiff, tolerance };
}

// Unlike every other check-landing-*.mjs, this one does not accept a URL
// override on argv[2]: the other scripts' override means "run this same
// check against an already-served copy of the compiled SITE" (a deployed
// URL, or check-landing-all.mjs's own shared server for the build) -- there
// is no compiled-site analogue for a repo-local module fixture, and
// check-landing-all.mjs passes its build server's URL to every script in
// its list regardless, which would resolve scripts/fixtures/... against
// the wrong root entirely. Always self-serves the worktree root instead.
const { baseURL, close } = await serveWorktree(WORKTREE_ROOT);
const fixtureURL = new URL('scripts/fixtures/watercolor-capture/index.html', baseURL).href;
const engines = await loadPlaywright();
const measure = !!process.env.WATERCOLOR_FIDELITY_MEASURE;
const ablate = process.env.WATERCOLOR_ABLATE || null;

const browsers = {};
try {
  browsers.chromium = await launchChromium(engines.chromium);
  browsers.webkit = await engines.webkit.launch();
  for (const [engineName, browser] of Object.entries(browsers)) {
    for (const dpr of [1, 2]) {
      const context = await browser.newContext({ viewport: { width: 900, height: 700 }, deviceScaleFactor: dpr });
      const page = await context.newPage();
      await page.goto(fixtureURL);
      const run = await runOnce(page, { ablate });
      const manifest = await page.evaluate(() => window.__fixtureManifest);
      const { byId, failingCells, maxDiff, tolerance } = judge(run, engineName);

      if (measure) {
        const okIds = manifest.filter((m) => m.expect === 'ok').map((m) => m.id);
        const okMax = Math.max(0, ...okIds.map((id) => maxDiff[id] || 0));
        console.log(`${engineName} dpr=${dpr}: measured max per-cell diff across expected-ok members = ${okMax.toFixed(2)}`);
        for (const id of okIds) console.log(`  ${id}: ${(maxDiff[id] || 0).toFixed(2)} (ok=${byId[id]?.ok})`);
        await context.close();
        continue;
      }

      for (const m of manifest) {
        const report = byId[m.id];
        assert(report, `I-fidelity ${engineName} dpr=${dpr}: no capture result for member ${m.id}`);
        if (m.expect === 'fail') {
          assert(!report.ok, `I-fidelity ${engineName} dpr=${dpr}: ${m.id} was expected to be a declared failure (no faithful capture is possible) but the registry reported ok -- a false "ok" on this member is worse than the failure it is supposed to replace`);
          continue;
        }
        assert(report.ok, `I-fidelity ${engineName} dpr=${dpr}: ${m.id} expected ok but the registry declared failure: ${report.reason}`);
        const bad = failingCells[m.id] || 0;
        assert(bad === 0, `I-fidelity ${engineName} dpr=${dpr}: ${m.id} has ${bad} cell(s) exceeding tolerance ${tolerance} (max diff ${(maxDiff[m.id] || 0).toFixed(1)}) -- printed pixels do not match the live element`);
      }
      console.log(`${engineName} dpr=${dpr}: I-fidelity green, ${manifest.length} members (${manifest.filter((m) => m.expect === 'fail').length} declared-failure by design)`);
      await context.close();
    }
  }
} finally {
  await Promise.all(Object.values(browsers).map((b) => b.close()));
  await close();
}
