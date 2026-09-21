#!/usr/bin/env node
// I-continuity (unit5-physics-spec.md section 4; dissolve-module-design.md
// section 6's Continuity bullet): while a wash morphs one composition into
// another, pigment coverage must never fall through a floor wherever both
// carry ink, total pigment mass must move smoothly between the two
// endpoints, a cell's shown hue must travel from A's colour to B's without
// passing through paper, and A-only ink must lift while B-only ink
// deposits. All four are read off two synthetic compositions of known
// colour (fixtures/watercolor-morph/), never the landing page's own scenes
// -- the fixture is what gives every tolerance a known-correct answer.
//
// The physics engine under test (V/HEAD/WATER/MEAN/SHOW/makeSim, "the
// morph" section of site/landing.js) is not duplicated here: this script
// extracts that exact span from the current landing.js at run time. A
// fixture with its own second copy of the shaders could drift from what
// actually ships and pass while the shipped code is broken -- the one
// failure mode a physics fixture exists to rule out. The single test-only
// change this script makes to that extracted copy -- probe() widened from
// one texel to a rectangle, so a whole grid comes back in one readback
// instead of one per texel -- never touches the file on disk; landing.js
// itself carries only the edits unit5-physics-spec.md section 1 lists.
//
// WATERCOLOR_MORPH_FAULT=coverage|mass|hue|liftonly re-breaks one already-
// landed fix in the extracted copy, reproducing the spec's own named fault
// for that clause, for the red/green evidence every new assertion needs.
// Unset, this runs against whatever site/landing.js currently contains.
//
// Per-cell math: cov = 1 - exp(-mean(d+s)) and mass = sum(d+s) over the
// cell's texels (section 4's own definitions, "d+s" read as the six-channel
// sum: three suspended + three deposited). "Shown colour" is
// clamp(exp(-(mean d + mean s)), 0, 1) per channel -- the same transmittance
// step SHOW's own `T = clamp(exp(-A), 0, 1)` applies to the same
// accumulating deposit-plus-suspended quantity, without SHOW's tint/paper-
// grain/target-print modulation, which this fixture holds neutral (a white
// --bg, opaque cells) and which is not what changed in this unit. cov(0),
// cov(1), mass(0), mass(1) and each cell's reference hue/chroma are all
// closed-form, computed directly from the fixture's own known RGB values
// via the same absorb() formula PIG/SHOW use (mix(1, clamp(rgb/tint,0,1),
// a); -log(max(*, 0.02))) -- the "known colour" endpoints section 4 names,
// not a second simulated run to p=0 or p=1. This keeps every floor and
// bound in the table fault-independent except where the corresponding sim
// reading is the thing under test, which is what lets each clause's fault
// flip only that clause.
import { resolveBaseURL, loadPlaywright } from './landing-harness.mjs';
import { readFile } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { resolve as resolvePath } from 'node:path';

const HERE = fileURLToPath(new URL('.', import.meta.url));
const WORKTREE_ROOT = resolvePath(HERE, '..');
const LANDING_JS = resolvePath(WORKTREE_ROOT, 'site', 'landing.js');
const assert = (cond, msg) => { if (!cond) throw new Error(msg); };

// ---- extract "the morph" section of landing.js, patch it for testing ----

const START = 'const V = `#version 300 es';
const END = '\nconst sim = makeSim(';

const FAULTS = {
  // Clause 1 (coverage floor): undo 1a entirely -- the JS uAds/uTakeFloor
  // split collapses back to the single clock-gated line, and PIG's uptake
  // drops the l-based take() gate.
  coverage(engine) {
    const jsOld = /gl\.uniform1f\(pig\.u\.uLift, 0\.075\); gl\.uniform1f\(pig\.u\.uAds, 0\.18\);\n\s*gl\.uniform1f\(pig\.u\.uTakeFloor, take\);/;
    assert(jsOld.test(engine), 'FAULT coverage: 1a\'s JS uAds/uTakeFloor lines not found -- has 1a landed yet?');
    engine = engine.replace(jsOld, 'gl.uniform1f(pig.u.uLift, 0.075); gl.uniform1f(pig.u.uAds, 0.18 * take);');
    const glslOld = /float take = max\(uTakeFloor, smoothstep\(uTakeL0, uTakeL1, l\)\);\n\s*vec3 ad = uAds \* take \* wet \* s \* max\(cap \* 1\.6 - d, vec3\(0\.0\)\);/;
    assert(glslOld.test(engine), 'FAULT coverage: 1a\'s PIG take-gated ad line not found');
    engine = engine.replace(glslOld, 'vec3 ad = uAds * wet * s * max(cap * 1.6 - d, vec3(0.0));');
    return engine;
  },
  // Clause 4 (A-only lifts, B-only deposits): keep 1a's l-based gate but
  // drop its floor, gating on l alone -- the fault the spec calls the
  // likeliest real mistake.
  liftonly(engine) {
    const old = /float take = max\(uTakeFloor, smoothstep\(uTakeL0, uTakeL1, l\)\);/;
    assert(old.test(engine), 'FAULT liftonly: 1a\'s max(uTakeFloor, ...) line not found -- has 1a landed yet?');
    return engine.replace(old, 'float take = smoothstep(uTakeL0, uTakeL1, l);');
  },
  // Clause 3 (hue path): neutralize 1b's mixGate by pinning uMixHold to 1 at
  // link time, its default-setting line beside uPaperScale.
  hue(engine) {
    const old = /if \(u\.uMixHold\) gl\.uniform1f\(u\.uMixHold, 0\.25\);/;
    assert(old.test(engine), 'FAULT hue: uMixHold default-setting line not found -- has 1b landed yet?');
    return engine.replace(old, 'if (u.uMixHold) gl.uniform1f(u.uMixHold, 1.0);');
  },
  // Clause 2 (monotonic mass): undo 1c -- the cure goes back to clamping l
  // everywhere and substituting the target print outright at the cure knee.
  mass(engine) {
    const glslOld = /l = max\(l, uCure \* foot\(vUv\)\);/;
    assert(glslOld.test(engine), 'FAULT mass: 1c\'s PIG foot()-masked cure line not found -- has 1c landed yet?');
    engine = engine.replace(glslOld, 'l = max(l, uCure);');
    const showOld = /vec3 dep = dd\.rgb \* g, tgt = absorb\(texture\(uTgt, at\)\);\n\s*vec3 ink = min\(mix\(dep, tgt, uCure \* foot\(uv\)\) \+ 0\.85 \* sc \* g, vec3\(4\.0\)\);/;
    assert(showOld.test(engine), 'FAULT mass: 1c\'s SHOW foot()-masked mix line not found -- has 1c landed yet?');
    engine = engine.replace(showOld, 'vec3 ink = min(mix(dd.rgb * g, absorb(texture(uTgt, at)), uCure) + 0.85 * sc * g, vec3(4.0));');
    return engine;
  },
};

export function extractEngine(landingSrc, faultName) {
  const startIdx = landingSrc.indexOf(START);
  assert(startIdx >= 0, 'extractEngine: start anchor not found in landing.js -- "the morph" section moved or was renamed');
  const endIdx = landingSrc.indexOf(END, startIdx);
  assert(endIdx >= 0, 'extractEngine: end anchor not found in landing.js after the start anchor');
  let engine = landingSrc.slice(startIdx, endIdx);

  // step() closes over T_SPLASH/T_TAKE/T_DRY, module consts declared
  // further down landing.js (valid there because step() is not called
  // until well after that line has run) -- reparsed from the live
  // declaration so this script cannot silently drift from the timing
  // constants unit 5 leaves unchanged (spec section 1d).
  const tMatch = landingSrc.match(/T_SPLASH\s*=\s*([\d.]+)[\s\S]{0,10}T_TAKE\s*=\s*([\d.]+)[\s\S]{0,10}T_DRY\s*=\s*([\d.]+)[\s\S]{0,10}T_CURE\s*=\s*([\d.]+)[\s\S]{0,10}T_WET\s*=\s*([\d.]+)[\s\S]{0,10}T_TOTAL\s*=\s*([\d.]+)/);
  assert(tMatch, 'extractEngine: T_SPLASH..T_TOTAL declaration not found or reshaped');
  const [T_SPLASH, T_TAKE, T_DRY, T_CURE, T_WET, T_TOTAL] = tMatch.slice(1).map(Number);

  // step() also closes over clamp01/smooth (drying/take are computed inside
  // step() itself, from t -- not passed in), so those travel too, verbatim.
  const clamp01Match = landingSrc.match(/const clamp01 = .*?;\n/);
  const smoothMatch = landingSrc.match(/const smooth = .*?;\n/);
  assert(clamp01Match && smoothMatch, 'extractEngine: clamp01/smooth declarations not found or reshaped');

  if (faultName) {
    assert(FAULTS[faultName], `extractEngine: unknown WATERCOLOR_MORPH_FAULT "${faultName}" -- expected one of ${Object.keys(FAULTS).join(', ')}`);
    engine = FAULTS[faultName](engine);
  }

  // Test-only widening: probe(x, y) already reads both pigment attachments
  // for one texel; the fixture needs the same read over a whole rectangle
  // in one call, not twelve thousand round trips, so this widens it here,
  // in the extracted copy only. See the module comment for why production
  // landing.js never carries this.
  const probeRe = /probe\(x, y\) \{[\s\S]*?return out;\n {4}\},/;
  assert(probeRe.test(engine), 'extractEngine: probe() body not found or reshaped -- widen patch needs updating');
  engine = engine.replace(probeRe, `probe(x, y, w = 1, h = 1) {
      const out = [];
      for (const [f, n] of [[wF[wi], 1], [pF[pi], 2]]) { gl.bindFramebuffer(gl.FRAMEBUFFER, f);
        for (let i = 0; i < n; i++) { gl.readBuffer(gl.COLOR_ATTACHMENT0 + i); const px = new Float32Array(w * h * 4); gl.readPixels(x, y, w, h, gl.RGBA, gl.FLOAT, px); out.push(px); } }
      return out;
    },`);

  const consts = clamp01Match[0] + smoothMatch[0] + `const T_SPLASH=${T_SPLASH}, T_TAKE=${T_TAKE}, T_DRY=${T_DRY};\n`;
  // Playwright's addInitScript runs page-supplied content inside its own
  // function wrapper (verified empirically -- a bare top-level `function`
  // declaration there is not reachable from the page's own later scripts),
  // unlike a real classic <script> tag. makeSim is still an ordinary local
  // inside that wrapper, so publishing it once by name is enough; every
  // shader/timing constant it closes over travels with it.
  const bridge = '\nwindow.makeSim = makeSim;\n';
  return { source: consts + engine + bridge, T_CURE, T_TOTAL };
}

// ---- Node-side pigment math (the module comment above states the formulas) ----

function absorbChannels(rgb255) {
  return rgb255.map((v) => -Math.log(Math.max(Math.min(v / 255, 1), 0.02)));
}
function covRef(rgb255) {
  const ch = absorbChannels(rgb255);
  return 1 - Math.exp(-(ch[0] + ch[1] + ch[2]));
}
function shownRef(rgb255) {
  return absorbChannels(rgb255).map((v) => 1 - Math.exp(-v));
}
function hueChroma([r, g, b]) {
  const max = Math.max(r, g, b), min = Math.min(r, g, b), chroma = max - min;
  if (chroma < 1e-6) return { hue: 0, chroma: 0 };
  let hue;
  if (max === r) hue = ((g - b) / chroma) % 6;
  else if (max === g) hue = (b - r) / chroma + 2;
  else hue = (r - g) / chroma + 4;
  hue *= 60; if (hue < 0) hue += 360;
  return { hue, chroma };
}
// Signed shortest arc from hue a to hue b, in degrees, in (-180, 180].
function hueDelta(a, b) { return ((b - a + 540) % 360) - 180; }
function angleDist(a, b) { const d = Math.abs(((a - b + 540) % 360) - 180); return d; }

// Reduces a W*H*4 RGBA texel buffer's [x0, x1) column range (all rows) to
// {cov, mass, shown}: cov and mass per section 4's own definitions, shown
// as this script's transmittance approximation (module comment above).
function reduceRegion(sArr, dArr, W, H, x0, x1) {
  let sumTotal = 0, sR = 0, sG = 0, sB = 0, dR = 0, dG = 0, dB = 0, n = 0;
  for (let y = 0; y < H; y++) {
    const rowBase = y * W * 4;
    for (let x = x0; x < x1; x++) {
      const idx = rowBase + x * 4;
      const s0 = sArr[idx], s1 = sArr[idx + 1], s2 = sArr[idx + 2];
      const d0 = dArr[idx], d1 = dArr[idx + 1], d2 = dArr[idx + 2];
      sumTotal += s0 + s1 + s2 + d0 + d1 + d2;
      sR += s0; sG += s1; sB += s2; dR += d0; dG += d1; dB += d2;
      n++;
    }
  }
  const mean = sumTotal / n;
  const shown = [1 - Math.exp(-(sR / n + dR / n)), 1 - Math.exp(-(sG / n + dG / n)), 1 - Math.exp(-(sB / n + dB / n))];
  return { cov: 1 - Math.exp(-mean), mass: sumTotal, shown };
}

// ---- drive the fixture ----

async function runFixture(page, { T_CURE, T_TOTAL }) {
  await page.waitForFunction(() => window.__fixtureReady === true, null, { timeout: 15000 });
  const setupError = await page.evaluate(() => window.__fixtureError || null);
  assert(!setupError, `fixture setup failed: ${setupError}`);
  const meta = await page.evaluate(() => {
    const fx = window.__morphFixture;
    return { W: fx.W, H: fx.H, CELL: fx.CELL, cells: fx.cells };
  });
  const checkpoints = [];
  for (let i = 1; i <= 19; i++) checkpoints.push(+(i * 0.05).toFixed(2)); // 0.05 .. 0.95
  const DT = 1 / 120;
  const perCheckpoint = await page.evaluate(async ({ checkpoints, DT, T_CURE, T_TOTAL }) => {
    const smooth = (a, b, t) => { const x = Math.min(1, Math.max(0, (t - a) / (b - a))); return x * x * (3 - 2 * x); };
    const fx = window.__morphFixture;
    let t = 0;
    const out = [];
    for (const p of checkpoints) {
      const goal = p * T_TOTAL;
      while (t < goal - 1e-9) { fx.step(t, smooth(T_CURE, T_TOTAL, t)); t += DT; }
      const [, sArr, dArr] = fx.probe(0, 0, fx.W, fx.H);
      out.push({ p, s: sArr, d: dArr });
    }
    return out;
  }, { checkpoints, DT, T_CURE, T_TOTAL });
  return { meta, perCheckpoint };
}

function judge({ meta, perCheckpoint }, engineName, label) {
  const { W, H, CELL, cells } = meta;
  const texelsPerCell = CELL * H;
  const problems = [];
  const note = (msg) => problems.push(msg);

  // Clause 2, monotonic pigment mass -- whole-grid, so reduced once per
  // checkpoint over the full [0, W) column range.
  const massRefA = texelsPerCell * cells.reduce((sum, c) => sum + absorbChannels(c.a).reduce((s, v) => s + v, 0), 0);
  const massRefB = texelsPerCell * cells.reduce((sum, c) => sum + absorbChannels(c.b).reduce((s, v) => s + v, 0), 0);
  const massLo = 0.8 * Math.min(massRefA, massRefB), massHi = 1.25 * Math.max(massRefA, massRefB);
  const massSpan = Math.abs(massRefB - massRefA);
  const massSeries = perCheckpoint.map(({ p, s, d }) => ({ p, mass: reduceRegion(s, d, W, H, 0, W).mass }));
  for (const { p, mass } of massSeries) {
    if (!(mass >= massLo && mass <= massHi)) note(`${label} ${engineName}: clause 2 (mass) at p=${p} mass=${mass.toFixed(3)} outside [${massLo.toFixed(3)}, ${massHi.toFixed(3)}]`);
  }
  for (let i = 1; i < massSeries.length; i++) {
    const step = Math.abs(massSeries[i].mass - massSeries[i - 1].mass);
    if (step > 0.15 * massSpan) note(`${label} ${engineName}: clause 2 (mass) step p=${massSeries[i - 1].p}->${massSeries[i].p} delta=${step.toFixed(3)} exceeds 15% of |mass(1)-mass(0)|=${massSpan.toFixed(3)}`);
  }

  // Clauses 1, 3, 4 -- per cell.
  for (const cell of cells) {
    const x0 = cell.col * CELL, x1 = x0 + CELL;
    const series = perCheckpoint.map(({ p, s, d }) => ({ p, ...reduceRegion(s, d, W, H, x0, x1) }));

    if (cell.kind === 'both') {
      const covRefA = covRef(cell.a), covRefB = covRef(cell.b);
      const covFloor = 0.6 * Math.min(covRefA, covRefB);
      const shownA = shownRef(cell.a), shownB = shownRef(cell.b);
      const { hue: hueA, chroma: chromaA } = hueChroma(shownA);
      const { hue: hueB, chroma: chromaB } = hueChroma(shownB);
      const chromaFloor = 0.35 * Math.min(chromaA, chromaB);
      const arc = hueDelta(hueA, hueB);
      for (const { p, cov, shown } of series) {
        if (!(cov >= covFloor)) note(`${label} ${engineName}: clause 1 (coverage floor) cell col=${cell.col} p=${p} cov=${cov.toFixed(4)} below floor ${covFloor.toFixed(4)}`);
        const { hue, chroma } = hueChroma(shown);
        if (!(chroma >= chromaFloor)) note(`${label} ${engineName}: clause 3 (chroma floor) cell col=${cell.col} p=${p} chroma=${chroma.toFixed(4)} below floor ${chromaFloor.toFixed(4)}`);
        const expectedHue = (hueA + arc * p + 360) % 360;
        const dist = angleDist(hue, expectedHue);
        if (!(dist <= 25)) note(`${label} ${engineName}: clause 3 (hue path) cell col=${cell.col} p=${p} hue=${hue.toFixed(1)} expected~${expectedHue.toFixed(1)} off by ${dist.toFixed(1)} deg (>25)`);
      }
    } else if (cell.kind === 'aOnly') {
      const covRefA = covRef(cell.a);
      const covEnd = series[series.length - 1].cov;
      if (!(covEnd <= 0.02 * covRefA)) note(`${label} ${engineName}: clause 4 (A-only lifts) cell col=${cell.col} cov(end)=${covEnd.toFixed(4)} exceeds 2% of cov(0)=${covRefA.toFixed(4)}`);
    } else if (cell.kind === 'bOnly') {
      const covRefB = covRef(cell.b);
      const covEnd = series[series.length - 1].cov;
      if (!(covEnd >= 0.9 * covRefB)) note(`${label} ${engineName}: clause 4 (B-only deposits) cell col=${cell.col} cov(end)=${covEnd.toFixed(4)} under 90% of cov_B_reference=${covRefB.toFixed(4)}`);
    }
  }
  return problems;
}

// ---- run ----
//
// Guarded on the entry-module check so extractEngine() can be imported
// elsewhere (the filmstrip capture this unit's report needs reuses it
// rather than a second copy of the same text-slicing) without launching a
// browser as an import side effect.

async function main() {
  const faultName = process.env.WATERCOLOR_MORPH_FAULT || null;
  const landingSrc = await readFile(LANDING_JS, 'utf8');
  const { source: engineSource, T_CURE, T_TOTAL } = extractEngine(landingSrc, faultName);

  const { baseURL, close } = await resolveBaseURL(null, { root: WORKTREE_ROOT });
  const fixtureURL = new URL('scripts/fixtures/watercolor-morph/index.html', baseURL).href;
  const engines = await loadPlaywright();

  const allProblems = [];
  try {
    for (const [engineName, engine] of [['chromium', engines.chromium], ['webkit', engines.webkit]]) {
      const browser = await engine.launch();
      try {
        const page = await browser.newPage();
        const pageErrors = [];
        page.on('pageerror', (e) => pageErrors.push(e.message));
        await page.addInitScript({ content: engineSource });
        await page.goto(fixtureURL);
        const run = await runFixture(page, { T_CURE, T_TOTAL });
        const problems = judge(run, engineName, faultName ? `FAULT=${faultName}` : 'unfaulted');
        allProblems.push(...problems);
        assert(pageErrors.length === 0, `${engineName}: page errors during the run: ${pageErrors.join('; ')}`);
        console.log(`${engineName}${faultName ? ` (fault=${faultName})` : ''}: ${problems.length === 0 ? 'I-continuity green, 12 cells x 19 samples' : `${problems.length} violation(s)`}`);
        for (const p of problems.slice(0, 8)) console.log(`  ${p}`);
        if (problems.length > 8) console.log(`  ... and ${problems.length - 8} more`);
        await page.close();
      } finally { await browser.close(); }
    }
  } finally { await close(); }

  if (allProblems.length > 0) {
    console.error(`I-continuity RED: ${allProblems.length} violation(s) across both engines${faultName ? ` under FAULT=${faultName}` : ''}`);
    process.exitCode = 1;
    return;
  }
  console.log(`I-continuity green in both engines${faultName ? ` (fault=${faultName} -- unexpected, this fault should have failed)` : ''}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await main();
