// The whole harness surface in one place: a read side, filled in as each
// piece of state comes into existence below, and a write side the harness
// itself sets (faults, honoured at each fault's own call site so an outside write
// reaches every caller, internal or not).
const landing = window.__landing = { title: { traceOn: false, trace: [] }, faults: { captureHang: false } };
// The composition (the box width is --ed-w, in the stylesheet).
const GEOM = { cellW: 660, cellH: 620, pad: 80 };
// The real homepages' opening text, edited as the vault's index.md.
const BLAKE_FRONTMATTER = `---
uid: 8f508658
description: "I must Create a System, or be enslav'd by another Man's. I will not Reason & Compare: my business is to Create."
children: false
---

`;
const SEED = '';
const TYPED = window.__LANG === 'zh'
  ? '生在曹洞臨濟有，穿過臨濟曹洞有。\n洞曹臨濟兩俱非，羸羸然若喪家之狗。\n還識得此人麼？\n羅漢道底。'
  : `I must Create a System, or be enslav'd by another Man's.${'  '}
I will not Reason & Compare: my business is to Create.`;
const FINAL = SEED + TYPED;
const FULL_SOURCE = window.__LANG === 'zh' ? FINAL : BLAKE_FRONTMATTER + FINAL;
const BASE_TEX_W = GEOM.cellW + 2 * GEOM.pad, BASE_TEX_H = GEOM.cellH + 2 * GEOM.pad;
const MAX_PRINT_EDGE = 2048;
let printRect = { x: -GEOM.pad, y: -GEOM.pad, w: BASE_TEX_W, h: BASE_TEX_H };
let pendingPrintRect = null, printGeneration = 0;
let printExpanded = false;
const printW = () => printRect.w, printH = () => printRect.h;
function applyPrintRect(next) {
  printRect = next; printGeneration++;
  if (typeof canvas !== 'undefined' && canvas) {
    canvas.style.left = next.x + 'px'; canvas.style.top = next.y + 'px'; canvas.style.right = 'auto'; canvas.style.bottom = 'auto';
    canvas.style.width = next.w + 'px'; canvas.style.height = next.h + 'px';
  }
}
function setPrintRect(next) {
  if (next.x === printRect.x && next.y === printRect.y && next.w === printRect.w && next.h === printRect.h) return;
  if (typeof running === 'function' && running()) { pendingPrintRect = next; return; }
  applyPrintRect(next); pendingPrintRect = null; sheets.fill(null);
}
function flushPrintRect() { if (pendingPrintRect && !running()) { const next = pendingPrintRect; pendingPrintRect = null; applyPrintRect(next); sheets.fill(null); } }
// The scenes, in the order the copy reaches them: the editor, the site, the
// three windows. `JOINS[i]` is the join between scene i and i+1 — every one of
// them re-wets the sheet today, but a join is not a wash by definition, and a
// later boundary can carry a different mechanism without touching the trigger.
const PHASE = ['write', 'live', 'ships', 'deploy', 'share'];
const LIVE = PHASE.indexOf('live'), SHIPS = PHASE.indexOf('ships'), DEPLOY = PHASE.indexOf('deploy'), SHARE = PHASE.indexOf('share');
// The box's leftward spill in the widened shape, as the stylesheet's own
// calc(100% + 32px): scene 4 measures against that shape whatever is on screen.
const SPILL = 32;
// Scene 3's own reserved margin: how far past the preview's own edge the
// layout leaves room for, on either side, for whatever scene 3 needs to show
// or drag past that edge — sized once as a sibling's peek, kept now as the
// original card layout, so the composition's
// own width (VIS_W, PAGE_MAX below) never has to move for scene 3's redesign.
// The two background layers no longer fade on their own clock: they arrive
// and leave under a transition's print (WatercolorMorph's membership). The
// wash's own constants are further down, at `const DT =`.
const PEEK = 150;
// Scene 4's control. PUB_SCALE/PUB_MS grow it, alone, before anything else
// happens. PUB_BEAT is the pause after growth before the
// targets start popping (below). Going back, the control drops last, after
// PUB_OUT_DELAY — FAN_OUT_MS, which the join out of scene 4 waits on before
// the sheet may be wetted again, gives the targets' own (much shorter) retreat
// room inside that budget rather than timing against it directly.
const PUB_SCALE = 2.6, PUB_MS = 420, PUB_BEAT = 120, PUB_OUT_DELAY = 450;
const FAN_OUT_MS = PUB_OUT_DELAY + PUB_MS;
// Scene 5's close. The loop, its poster and the headline are the page's own
// arguments, so another cut of the footage or another line can be put in front
// of the director without an edit. SCRIM is how much paper lies over the footage
// so the line reads off it. XF_SPAN is the share of the distance scene 4 owns —
// from its own resting position to scene 5's — that the crossfade is scrubbed
// over: the last tenth of it. A pacing complaint in this scene is a change to
// these numbers.
const arg = (k, d) => new URLSearchParams(location.search).get(k) || d;
const LOOP_SRC = arg('loopVideo', 'scene5-loop/out/v5-makers.mp4');
const LOOP_POSTER = arg('loopPoster', 'scene5-loop/out/v5-makers-poster.jpg');
const HEADLINE = arg('headline', '');
const SCRIM = 0.72, XF_SPAN = 0.1;
// The one window radius every sheet on the stage wears (--win-r in the
// stylesheet); mirrored here for the same reason GEOM mirrors --cell-w — a
// print built on canvas has no custom property to read it from.
const WIN_R = 12;
// Only the three creative artifacts move. Their initial positions are
// measured against the available viewport when Scene 3 opens.
const S3_ORDER = ['sk', 'nb', 'video'];
// the composition is three layers wide still: the cell, its 32px spill, two peeks
const VIS_W = 660 + 32 + 2 * PEEK;
// The width the copy column is never scaled below: its own full measure, not the
// narrowest it can be set in. A third scene's heading is a long line, and the
// composition gives ground to it rather than breaking it in three.
const COPY_MIN = 440;
// The whole composition, capped at what it needs and no more, so a wide screen
// does not pull the sheets and the copy apart — the cap movie5 wrote as 1440,
// now that three sheets and a full measure decide the number.
const PAGE_MAX = 56 + VIS_W + 96 + COPY_MIN;
// Text geometry chooses a boundary and progress; the visual track renders it.
// The first three boundaries share liquid pigment. The third also carries a
// solid Publish control. The last mobile boundary turns the full logo group
// into gray film pigment while the full-screen movie takes over.
const JOINS = [
  { name: 'wash', wash: true },
  { name: 'slide', wash: true },
  { name: 'fan', wash: true },
  { name: 'crossfade', run: (to) => fade(to) },
];
const joinAt = (a, b) => JOINS[Math.min(a, b)];


for (const [k, v] of Object.entries({ '--peek': PEEK + 'px',
  '--vis-w': VIS_W + 'px', '--page-max': PAGE_MAX + 'px',
  '--pub-k': String(PUB_SCALE), '--pub-ms': PUB_MS + 'ms', '--pub-out-delay': PUB_OUT_DELAY + 'ms', '--scrim': String(SCRIM) }))
  document.documentElement.style.setProperty(k, v);

const $ = (id) => document.getElementById(id);
const stage = $('stage'), box = $('box'), canvas = $('gl'), cell = $('cell');
const edFrame = $('ed'), shFrame = $('sh'), vdFrame = $('vd');
const five = $('five'), loopVid = $('loop');
const page = document.querySelector('.page');
// one frame per scene; scenes 2 and 3 are both moss's shell, around a different
// page, and scene 4 is that same frame cut down to the publish control. Scene 5
// has no frame and is never printed: no wash reaches it.
const FRAMES = [edFrame, shFrame, vdFrame, vdFrame];
const scenesEl = [...document.querySelectorAll('.scene')];
// Assign the heavy iframe sources only after the deferred runtime has loaded.
// The HTML remains readable and request-free when scripts are delayed or
// blocked, while the normal boot path keeps the locale-specific harvested UI.
function loadLandingFrames() {
  if (window.__LANG === 'zh') {
    edFrame.src = 'ui/editor.html?measure=400&root=' + encodeURIComponent('八大山人') + '&file=' + encodeURIComponent('八大山人.md') + '&date=1659-12-01&home=1';
    shFrame.src = 'ui/shell.html?preview=' + encodeURIComponent('/zhuda/homepage/') + '&title=' + encodeURIComponent('八大山人') + '&target=zhudasnotebook.com&live=' + encodeURIComponent('https://www.zhudasnotebook.com/');
  } else {
    edFrame.src = 'ui/editor.html?measure=400&root=' + encodeURIComponent('William Blake') + '&file=' + encodeURIComponent('William Blake.md') + '&date=1790-06-01&home=1';
    shFrame.src = 'ui/shell.html?preview=' + encodeURIComponent('/blake/homepage/') + '&title=' + encodeURIComponent('William Blake') + '&target=blakesnotebook.com&live=' + encodeURIComponent('https://www.blakesnotebook.com/');
  }
  vdFrame.src = vdFrame.dataset.src;
}
loadLandingFrames();
const clamp01 = (v) => Math.min(1, Math.max(0, v));
const clampTo = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const smooth = (a, b, t) => { const x = clamp01((t - a) / (b - a)); return x * x * (3 - 2 * x); };
const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;

// Scene 1's plates (director's call, 2026-09-18): every image the real home
// page uses — blakesnotebook.com's own 9, zhudasnotebook.com's own 7 (two of
// its <img>s repeat a collection cover, so 7 distinct files) — not a crop of
// the article scene 2 shows. The plate directories are 640px, quality-70
// derivatives of those live images, sized for loose sheets that render at
// 160–260 CSS px without making the hidden preview frames use lower-resolution
// article assets.
const PLATE_SET = window.__LANG === 'zh'
  ? {"dir": "zhuda/plates", "images": ["376d25daf9aa.jpg", "3926a51d36d5.jpg", "9e4725e1101e.jpg", "e265c76d268f.jpg", "e4772754b3b1.jpg", "e65a0ba027e5.jpg", "eab0bc5785fa.jpg"]}
  : {"dir": "blake/plates", "images": ["3b119ebffab3.jpg", "b8dcbfe35cbd.jpg", "38211f928ad5.jpg", "171a382c9cc2.jpg", "771aaf08cedb.jpg", "f34b78575d36.jpg", "50e62b346c5f.jpg", "5ac4f7fb652f.jpg", "0bb22b56b263.jpg"]};
// Natural width/height for every plate file, both sets: scatterPlates needs
// each plate's real aspect before its <img> has loaded (the box is sized and
// placed up front, the pixels fade in after), so this can't be read from the
// DOM. The English set had none until 2026-09-20 — PLATE_ASPECT's ternary
// only ever filled in zh — so every English plate defaulted to a square box
// (`|| 1` below) regardless of its real shape, and object-fit: contain then
// painted the box's own background into the leftover margin on both sides
// of whichever axis the real image was narrower on.
const PLATE_ASPECT = window.__LANG === 'zh'
  ? {'376d25daf9aa.jpg': 280 / 640, '3926a51d36d5.jpg': 640 / 622, '9e4725e1101e.jpg': 640 / 472, 'e265c76d268f.jpg': 426 / 640, 'e4772754b3b1.jpg': 337 / 640, 'e65a0ba027e5.jpg': 426 / 640, 'eab0bc5785fa.jpg': 318 / 640}
  : {'3b119ebffab3.jpg': 475 / 640, 'b8dcbfe35cbd.jpg': 640 / 490, '38211f928ad5.jpg': 534 / 640, '171a382c9cc2.jpg': 531 / 640, '771aaf08cedb.jpg': 640 / 456, 'f34b78575d36.jpg': 640 / 326, '50e62b346c5f.jpg': 492 / 640, '5ac4f7fb652f.jpg': 640 / 237, '0bb22b56b263.jpg': 468 / 640};
// Where a plate may live, in #cell's own coordinates. The viewport-aware
// bounds let a reader drag beyond the resting sheet while the print rectangle
// grows to include the visible domain; the state textures remain bounded.
function plateScatterBounds() {
  const edW = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ed-w')) || 520;
  const margin = 14;
  const r = cell.getBoundingClientRect(), scale = r.width / GEOM.cellW || 1;
  return { xMin: Math.max(-360, (margin - r.left) / scale), xMax: GEOM.cellW - edW + 60, yMin: -GEOM.pad + margin, yMax: GEOM.cellH + GEOM.pad - margin };
}
function plateBounds() {
  const r = cell.getBoundingClientRect(), s = Math.max(SCALE, 0.5), margin = 80, limit = 1800;
  const visible = { xMin: -r.left / s, xMax: (innerWidth - r.left) / s, yMin: -r.top / s, yMax: (innerHeight - r.top) / s };
  return { xMin: Math.max(-limit, visible.xMin - margin), xMax: Math.min(limit, Math.max(GEOM.cellW + GEOM.pad, visible.xMax + margin)), yMin: Math.max(-limit, visible.yMin - margin), yMax: Math.min(limit, Math.max(GEOM.cellH + GEOM.pad, visible.yMax + margin)) };
}
function currentPrintRect() {
  let xMin = -GEOM.pad, yMin = -GEOM.pad, xMax = GEOM.cellW + GEOM.pad, yMax = GEOM.cellH + GEOM.pad;
  for (const pl of document.querySelectorAll('.plate')) { xMin = Math.min(xMin, pl.offsetLeft - GEOM.pad); yMin = Math.min(yMin, pl.offsetTop - GEOM.pad); xMax = Math.max(xMax, pl.offsetLeft + pl.offsetWidth + GEOM.pad); yMax = Math.max(yMax, pl.offsetTop + pl.offsetHeight + GEOM.pad); }
  if (typeof CARDS !== 'undefined') for (const c of Object.values(CARDS)) {
    if (!c.pose) continue;
    xMin = Math.min(xMin, c.pose.x - GEOM.pad); yMin = Math.min(yMin, c.pose.y - GEOM.pad);
    xMax = Math.max(xMax, c.pose.x + c.width * c.pose.s + GEOM.pad); yMax = Math.max(yMax, c.pose.y + c.height * c.pose.s + GEOM.pad);
  }
  xMin = Math.max(-MAX_PRINT_EDGE, xMin); yMin = Math.max(-MAX_PRINT_EDGE, yMin);
  xMax = Math.min(MAX_PRINT_EDGE, xMax); yMax = Math.min(MAX_PRINT_EDGE, yMax);
  return { x: xMin, y: yMin, w: xMax - xMin, h: yMax - yMin };
}
// Random per load (director's call #1); the only requirement is that it reads
// as a scatter and not a grid, and that nothing clips off-viewport (the
// initial scatter uses its own resting bounds, while dragged plates use the
// viewport-aware bounds above, and the print domain follows them).
// margin sits well inside the page's real left gutter at every viewport this
// composition scales to) or piles unreadably (the greedy least-overlap pick
// below). Each plate tries a handful of random rects and keeps the one that
// overlaps existing plates least, rather than a single blind draw.
function scatterPlates(plates) {
  const b = plateScatterBounds();
  const rand = (lo, hi) => lo + Math.random() * (hi - lo);
  const rects = [];
  for (const pl of plates) {
    const aspect = PLATE_ASPECT[pl.dataset.lqip] || 1;
    // The 96px floor is on the SHORT side, so it has to raise `long` (both
    // sides together) rather than clamp the short side alone: clamping only
    // the short side is what used to paint a black bar on the very-wide and
    // very-tall plates (e.g. blake's 640x237) whenever the random draw put
    // long near 190 -- the box kept long's width but forced height up to 96,
    // which is no longer this image's own ratio.
    const longAspect = Math.max(aspect, 1 / aspect);
    const long = Math.round(Math.max(rand(190, 280), 96 * longAspect));
    const w = aspect >= 1 ? long : Math.round(long * aspect);
    const h = aspect >= 1 ? Math.round(long / aspect) : long;
    const xHi = Math.max(b.xMin, b.xMax - w), yHi = Math.max(b.yMin, b.yMax - h);
    let bx = b.xMin, by = b.yMin, best = Infinity;
    for (let t = 0; t < 40 && best > 0; t++) {
      const x = rand(b.xMin, xHi), y = rand(b.yMin, yHi);
      let score = 0;
      for (const r of rects) score += Math.max(0, Math.min(x + w, r.x + r.w) - Math.max(x, r.x)) * Math.max(0, Math.min(y + h, r.y + r.h) - Math.max(y, r.y));
      if (score < best) { best = score; bx = x; by = y; }
    }
    rects.push({ x: bx, y: by, w, h });
    pl.style.left = bx + 'px'; pl.style.top = by + 'px'; pl.style.width = w + 'px'; pl.style.height = h + 'px';
    pl.style.setProperty('--r', (Math.round(rand(-7, 7) * 10) / 10) + 'deg');
  }
}
function buildPlates() {
  const els = PLATE_SET.images.map((key) => {
    const pl = document.createElement('div');
    pl.className = 'plate'; pl.id = 'p-' + key; pl.dataset.lqip = key;
    const im = document.createElement('img');
    im.alt = ''; im.decoding = 'async'; im.dataset.src = PLATE_SET.dir + '/' + key;
    pl.appendChild(im);
    return pl;
  });
  cell.prepend(...els);
  scatterPlates(els);
  return els;
}
const plateEls = buildPlates();

// Each local homepage image fades in after decoding.
plateEls.forEach((pl) => { const im = pl.querySelector('img'); im.addEventListener('load', () => im.classList.add('in')); im.src = im.dataset.src; });

// Fit: the cell is a fixed composition; smaller windows scale it as one.
let SCALE = 1, baseScale = 1;
const mobileLayout = () => innerWidth <= 900;
const opening = document.getElementById('intro');
const openingTitle = opening.querySelector('h1');
// introWatercolor: a cached 2D-canvas noise mask, restored verbatim in
// spirit from the pre-refactor build (git show landing-live-20260919:
// site/index.html) as the title's DEFAULT renderer -- a pure function of
// progress with no set-up, correct on frame one, unlike the WebGL wash
// below (a second context, shader compiles, a raster) which needs real
// time to arm. 24 discrete steps, each mask cached once on first use: most
// frames re-hit an already-drawn step and cost one integer compare.
let introMaskStep = -1;
const introMaskCache = new Map();
function introWatercolor(progress) {
  if (reduce || progress > .94) {
    introMaskStep = -1;
    openingTitle.style.webkitMaskImage = openingTitle.style.maskImage = 'none';
    return;
  }
  const step = Math.round((1 - progress) * 24);
  if (step === introMaskStep) return;
  introMaskStep = step;
  let data = introMaskCache.get(step);
  if (!data) {
    const c = document.createElement('canvas'); c.width = 192; c.height = 64;
    const x = c.getContext('2d'), im = x.createImageData(c.width, c.height);
    const hash = (ix, iy) => { let v = Math.imul(ix + 37, 374761393) ^ Math.imul(iy + 71, 668265263) ^ 0x5f3759df; v = Math.imul(v ^ v >>> 13, 1274126177); return ((v ^ v >>> 16) >>> 0) / 4294967295; };
    const noise = (px, py) => { const ix = Math.floor(px), iy = Math.floor(py), fx = px - ix, fy = py - iy, sx = fx * fx * (3 - 2 * fx), sy = fy * fy * (3 - 2 * fy); return (hash(ix, iy) * (1 - sx) + hash(ix + 1, iy) * sx) * (1 - sy) + (hash(ix, iy + 1) * (1 - sx) + hash(ix + 1, iy + 1) * sx) * sy; };
    const fbm = (px, py) => .57 * noise(px / 22, py / 22) + .29 * noise(px / 10 + 19, py / 10 + 7) + .14 * noise(px / 4 + 43, py / 4 + 29);
    const threshold = .18 + step / 24 * .72;
    for (let py = 0; py < c.height; py++) for (let px = 0; px < c.width; px++) {
      const value = fbm(px, py) + px / c.width * .18;
      const t = Math.max(0, Math.min(1, (value - (threshold - .06)) / .12));
      const a = Math.round((t * t * (3 - 2 * t)) * 255);
      im.data[(py * c.width + px) * 4 + 3] = a;
    }
    x.putImageData(im, 0, 0); data = c.toDataURL(); introMaskCache.set(step, data);
  }
  const url = `url(${data})`;
  openingTitle.style.webkitMaskImage = openingTitle.style.maskImage = url;
  openingTitle.style.webkitMaskSize = openingTitle.style.maskSize = '100% 100%';
}
// The title's own pigment sim is set up further down (after makeSim/raster/
// advanceWash exist) and is an ENHANCEMENT, not a replacement that must be
// waited for: titleDissolve starts as introWatercolor (above) and stays
// there, correct from frame one, until setupTitleDissolve arms the wash AND
// driveTitleDissolve below sees titleP land exactly on a boundary (1, fully
// solid, or 0, fully gone) -- the one moment the two renderers agree pixel
// for pixel, so the swap is never visible. washDissolve is set once the sim
// exists; simArmed gates the swap so an in-flight scroll is never handed a
// mid-dissolve pop.
let titleDissolve = introWatercolor;
let washDissolve = null;
// Set alongside washDissolve: snaps the wash's own clock straight to
// whichever end it is being handed off at, instead of leaving it at its
// post-reset 0 (== "fully solid") regardless of which boundary this is.
// Handing off at titleP===0 without this pops just as hard as the bug
// being fixed: driven from a fresh clock, washDissolve's own goal (T_TOTAL,
// "fully gone") is real distance away, so it would show several frames of
// the wash animating from solid to gone while introWatercolor's mask had
// already been sitting at gone -- primeWash makes the wash's first frame
// match whichever boundary it is, exactly, the same way a fresh clock
// already happens to match the titleP===1 boundary on its own.
let primeWash = null;
let simArmed = false;
let pendingTitleP = 1;
let titleSteps = 0;
// Set by washDissolve on every call, read by driveTitleDissolve's trace push
// below -- diagnostic only (goal/clockT/steps), so a probe can see what the
// sim's own clock was asked to do this call without a second, desynced
// sampler. null until the wash has taken its first call.
let lastWashDiag = null;
// True once setupTitleDissolve has decided the title's own fate (armed,
// fell back, or does not apply here) — a harness signal so a test can wait
// for that decision instead of guessing how long it takes.
let titleReady = false;
// Which mechanism owns the title: 'wash' once the real sim is driving it,
// 'fade' for the plain-opacity fallback (no WebGL2, or a lost context), and
// 'plain' where none of this applies at all (mobile and reduced motion,
// where driveTitleDissolve/introWatercolor are never reached or always a
// no-op, or before setup has decided). Still 'plain' while introWatercolor
// is the active desktop renderer, same as before this restore -- the mask
// is introWatercolor's own name for itself (h1's maskImage), not a mode a
// harness needs to select on; only the wash needs that (below). A harness
// signal so a test can require the real wash rather than silently passing
// on the fallback it exists to catch.
// The watercolor title is switched off (2026-09-21). With it armed and the
// page idle at the top, the first scroll tick could drop the visible ink by
// 0.98 in one frame: the sudden disappearance the owner reported. Until the
// first-tick catch-up is fixed, introWatercolor's mask, a pure function of
// scroll that cannot pop, draws every dissolve. Flip this to re-enable.
const TITLE_WASH = true;
let titleMode = 'plain';
function driveTitleDissolve(titleP) {
  pendingTitleP = titleP;
  if (simArmed && titleDissolve !== washDissolve && (titleP === 1 || titleP === 0)) {
    primeWash(titleP);
    titleDissolve = washDissolve;
    titleMode = 'wash';
  }
  titleDissolve(titleP);
  // Opt-in only (landing.title.traceOn, off by default: no array growth,
  // no cost, on every other call): a per-frame check reading titleP/ink off
  // its own separate requestAnimationFrame chain is not synchronized with
  // this one, so under real CPU contention (a concurrent browser job, a
  // loaded machine) it can miss several of this function's own calls
  // between two of its samples and read the accumulated jump as if it were
  // one frame's worth -- found live, chasing a false "pop" that traced back
  // to the sampler's own gap, not this function's. Recorded here instead,
  // synchronously with every real call, has no such gap to miss.
  if (landing.title.traceOn) {
    const cs = getComputedStyle(openingTitle);
    const canvas = document.getElementById('gl-title');
    const canvasVisible = !!canvas && getComputedStyle(canvas).display !== 'none';
    landing.title.trace.push({
      titleP, ink: landing.title.ink(),
      h1mask: (cs.maskImage || cs.webkitMaskImage || 'none') !== 'none',
      canvasVisible,
      // Diagnostic fields: the sim's own goal/clock/step-count this call,
      // from washDissolve itself (goal/clockT/steps, a probe's own reading,
      // not used by the check below), and renderedInk -- what the check
      // actually grades for POP, since ink() (remaining()) races ahead of
      // what is painted (see TITLE_LOAD's own comment).
      goal: lastWashDiag ? lastWashDiag.goal : null,
      clockT: lastWashDiag ? lastWashDiag.clockT : null,
      steps: lastWashDiag ? lastWashDiag.steps : null,
      renderedInk: landing.title.renderedInk ? landing.title.renderedInk() : null,
    });
  }
}
// Overwritten once the title's own sim is armed AND handed the h1 (below);
// correct on its own before that, and for mobile/reduced motion, where it
// is never overwritten at all: introMaskStep < 0 is introWatercolor's own
// "no mask, solid" state (nothing dissolving, so the h1's own opacity is
// the whole answer), and otherwise its 24-step quantization of how much of
// the mask's own alpha remains is a closer reading of the visible glyph
// than the h1's opacity, which introWatercolor never touches.
landing.title.ink = () => introMaskStep < 0 ? Number(getComputedStyle(openingTitle).opacity) : 1 - introMaskStep / 24;
function updateOpening() {
  // On phones the opening and first scene are ordinary document flow. Their
  // geometry is established once in fit(); scrolling only moves that flow.
  if (mobileLayout()) return;
  const visible = Math.max(0, opening.getBoundingClientRect().bottom);
  const introP = Math.min(1, visible / Math.max(1, opening.offsetHeight));
  const first = document.querySelector('#c1 .scene-text').getBoundingClientRect();
  const firstRest = Math.max(1, scrollY + (first.top + first.bottom) / 2 - innerHeight * .5);
  const titleP = clamp01(1 - scrollY / firstRest);
  // The opening's scale and lift end with its title, so scene 1 arrives at the
  // same stage pose scene 2 morphs from. The copy can still occupy the opening
  // beat; it must not leave the live paper smaller or lower than its print.
  const stageIntroP = introP * titleP;
  const introScale = .4 + .1 * clamp01((innerHeight - 720) / 480);
  SCALE = baseScale * (1 - (1 - introScale) * stageIntroP);
  document.documentElement.style.setProperty('--s', String(SCALE));
  // Published for a test to read the exact value this frame drove the
  // title with, rather than recomputing it independently and risking a
  // one-frame mismatch against whichever scrollY this frame actually used
  // (found live: an out-of-process sampler on its own rAF chain can read a
  // scrollY a tick newer than this one, one frame ahead of what the mask
  // reflects -- imperceptible to a reader, but a false "no mask" reading
  // to a per-frame check). Restored from the pre-refactor build (git show
  // landing-live-20260919:site/index.html), which published this too.
  document.documentElement.style.setProperty('--intro-title-p', String(titleP));
  driveTitleDissolve(titleP);
  // CSS pins the visual from the document opening. Only the intro moves it;
  // once titleP is zero there is no scroll-position compensation to chase.
  const pageStart = parseFloat(document.documentElement.style.getPropertyValue('--page-start')) || 0;
  document.documentElement.style.setProperty('--opening-lift', (titleP * (Math.max(0, pageStart - scrollY) - visible * .33)) + 'px');
}
function fit() {
  const sw = (Math.min(innerWidth, PAGE_MAX) - 56 - 96 - COPY_MIN) / VIS_W;
  const mobileScale = Math.min((innerWidth - 32) / 820, 366 / 750);
  // Mobile browser chrome changes innerHeight while the reader scrolls. Size
  // the visual from width so that an address-bar resize cannot move the band.
  const visualHeight = mobileLayout() ? Math.min(390, Math.max(260, mobileScale * 750 + 24)) : Math.min(innerHeight * .48, 390);
  baseScale = mobileLayout() ? mobileScale : Math.max(.5, Math.min(1, sw, (innerHeight - 96) / GEOM.cellH));
  const root = document.documentElement.style;
  root.setProperty('--layout-s', String(baseScale));
  root.setProperty('--page-start', (document.querySelector('.page').getBoundingClientRect().top + scrollY) + 'px');
  if (mobileLayout()) { SCALE = baseScale; root.setProperty('--s', String(SCALE)); }
  root.setProperty('--intro-h', opening.offsetHeight + 'px');
  root.setProperty('--mobile-vis-h', visualHeight + 'px');
  const firstCopyHeight = document.querySelector('#c1 .scene-text').offsetHeight;
  root.setProperty('--first-copy-h', (firstCopyHeight + 28) + 'px');
  if (mobileLayout()) {
    // Scene 1's scattered plates extend about 70 composition pixels above #stage.
    // Keep that paint inside #vis so its sticky top is the visible top, then
    // place the whole visual band one fixed reading gap after the copy.
    root.setProperty('--mobile-stage-overhang', (70 * baseScale) + 'px');
    root.setProperty('--mobile-dwell', Math.min(340, Math.max(220, visualHeight * .9)) + 'px');
    root.setProperty('--mobile-col-pad', (firstCopyHeight + 32) + 'px');
  }
  document.getElementById('vis').inert = mobileLayout();
  updateOpening();
  if (printExpanded) setPrintRect(currentPrintRect());
}
addEventListener('resize', fit); fit();
  document.fonts.ready.then(fit);
scatterPlates(plateEls);

// ── The morph: the sheet is flooded, the print dissolves, the next print takes ─
// No brush. Water arrives over the whole printed area at once, the way a sheet
// is flooded or misted (Curtis et al. 1997 shallow water: depth, velocity,
// fibre saturation; paper relief; rim current). The old print's ink is
// re-wetted and goes entirely into suspension, where it diffuses, rides the
// film's currents and is stirred at large scale (an eddy diffusivity, taken
// from the coarse mip levels of the suspended field), so the film becomes one
// muddy pigment liquid: what a real wash does when several colours are
// dissolved together. The sheet then takes pigment back selectively. The next
// print is the sheet's affinity map, the way a lithographic plate takes ink
// only on its image or a mordanted cloth takes dye only where it is sized;
// uptake follows Langmuir kinetics, proportional to what is in the liquid and
// to the remaining capacity, per colour channel. As the film dries the
// unfixed liquid leaves with it (the wash-off of dyeing), a little strands at
// the rim (Deegan's ring), and the fixed pigment cures onto the print exactly.
// The canvas multiplies into the page: bare page paints nothing.
const V = `#version 300 es
in vec2 q; out vec2 vUv; void main(){ vUv = q; gl_Position = vec4(q * 2.0 - 1.0, 0.0, 1.0); }`;
const HEAD = `#version 300 es
precision highp float; precision highp sampler2D;
in vec2 vUv; uniform vec2 uSize; uniform sampler2D uPaper; uniform vec3 uTint;
// The prints at three resolutions, each a plain texture: full, the sim grid,
// and a quarter of it for the footprint. They are downscaled on the 2D canvas
// rather than by generateMipmap, so no driver's mip chain is in the picture.
uniform sampler2D uSrc, uTgt, uSrcLo, uTgtLo, uSrcFt, uTgtFt;
// uPaper is sampled in texel coordinates, so its 256x256 tileable period is
// a fixed number of texels regardless of the instance's own grid size or
// its CSS-px-per-texel: a grid with few, large texels sees the same period
// as a grid with many, small ones, so the grain reads at a different size
// on screen. uPaperScale (default 1, set once per program like uLoad) lets
// an instance retune that without a second paper texture.
uniform float uPaperScale;
vec4 P(vec2 t){ t *= uPaperScale; return mix(texture(uPaper, t / 256.0), texture(uPaper, t / 977.0 + 0.37), 0.45); }
float wetOf(float h){ return smoothstep(0.0004, 0.004, h); }
// ink density of a printed pixel: what it takes out of the paper's light;
// a soft-edged or shadowed pixel is its colour laid over the page at its alpha
vec3 absorb(vec4 c){ vec3 r = mix(vec3(1.0), clamp(c.rgb / uTint, 0.0, 1.0), c.a); return -log(max(r, vec3(0.02))); }
// the wetted sheet: the prints' footprint blurred (coarse mip) and broken by
// the fibres, so the wet edge is ragged like a wet-in-wet wash, not a frame
// the sheet under the scene: the union of the two prints' silhouettes (not their
// soft shadows), blurred a little and broken by the fibres; and the next
// print's own silhouette, which is where the film gathers as it dries
float sil(float a, vec2 uv){ vec4 pap = P(uv * uSize); return smoothstep(0.5, 0.9, a + (pap.g - 0.5) * 0.35 + (pap.b - 0.5) * 0.15); }
float foot(vec2 uv){ return sil(max(texture(uSrcFt, uv).a, texture(uTgtFt, uv).a), uv); }
float footT(vec2 uv){ return sil(texture(uTgtFt, uv).a, uv); }`;

// Water: (depth, u, v, saturation).
const WATER = HEAD + `
uniform sampler2D uW;
uniform float uSplash, uMist, uCure, uEvap, uDrying; uniform vec2 uTilt;
out vec4 o;
vec4 W(vec2 t){ return texture(uW, t / uSize); }
float wetAt(vec2 t){ return wetOf(W(t).r); }
void main(){
  vec2 t = gl_FragCoord.xy;
  vec4 w0 = W(t); float s = w0.a;
  vec2 back = t - w0.gb; vec4 wb = W(back);
  float h = wb.r; vec2 vel = wb.gb;
  float hl = (W(t+vec2(1,0)).r + W(t-vec2(1,0)).r + W(t+vec2(0,1)).r + W(t-vec2(0,1)).r) * 0.25;
  h = mix(h, hl, 0.25);
  vec4 pap = P(t);
  // the splash: a handful of big drops land on the printed area and their
  // impact pushes the water outward (the momentum is what makes the film move
  // and the dissolution fast), with a light mist so every part wets
  vec2 px = 1.0 / uSize;
  float f = max(foot(vUv), max(max(foot(vUv + vec2(5.0, 0.0) * px), foot(vUv - vec2(5.0, 0.0) * px)), max(foot(vUv + vec2(0.0, 5.0) * px), foot(vUv - vec2(0.0, 5.0) * px))));
  const vec2 drops[8] = vec2[8](vec2(0.30, 0.62), vec2(0.62, 0.42), vec2(0.46, 0.80), vec2(0.72, 0.72), vec2(0.24, 0.28), vec2(0.55, 0.15), vec2(0.88, 0.14), vec2(0.86, 0.86));
  // R was a fixed texel count (110.0), tuned for the main wash's own grid
  // (410x390 desktop): on a wide, short title grid that same texel count
  // reaches most of the box regardless of drop position, flooding it into
  // one blob. Scaled by the instance's own uSize (already how drops[i]*uSize
  // places each centre) it reproduces exactly 110.0 for the main instance —
  // min(uSize)=390 there, 110/390*390=110 — and shrinks with a smaller grid.
  float dropR = (110.0 / 390.0) * min(uSize.x, uSize.y);
  for (int i = 0; i < 8; i++) {
    vec2 c = drops[i] * uSize; vec2 rd = t - c; float rl = max(length(rd), 1e-3);
    float R = dropR * (0.8 + 0.4 * float(i % 3) * 0.5);
    float fall = smoothstep(R, R * 0.25, rl);
    // a drop lands where it falls: water past the print's edge dilutes the
    // liquid and gives the wet region a drop's outline, not the print's
    h += fall * uSplash * (0.1 + 0.9 * f);
    vel += (rd / rl) * fall * uSplash * 1.6 * (0.3 + 0.7 * f);
  }
  h += uMist * f * (0.5 + 1.0 * pap.b) * (1.2 - pap.r);
  // forces: downhill along the free surface and the paper's relief; the page lies flat
  float hasW = smoothstep(0.0002, 0.002, h);
  float gx = (W(t+vec2(1,0)).r + P(t+vec2(1,0)).r * 0.45) - (W(t-vec2(1,0)).r + P(t-vec2(1,0)).r * 0.45);
  float gy = (W(t+vec2(0,1)).r + P(t+vec2(0,1)).r * 0.45) - (W(t-vec2(0,1)).r + P(t-vec2(0,1)).r * 0.45);
  vel += (-0.45 * vec2(gx, gy) * 0.5) * hasW;
  // the scroll tilts the sheet: the film drifts the way the page is being
  // pushed, at once. A drift, not a force: the tilt sets a target speed the
  // film relaxes toward, so a long scroll cannot wind it up to the cap and
  // slide the whole film down the sheet (which is what put a lattice of
  // advection aliasing over the plates on a real GPU)
  vel += (uTilt - vel) * 0.15 * hasW;
  // evaporation at the wet-dry boundary drives the flow outward: the rim
  vec2 sob = vec2(wetAt(t+vec2(1.7,0.)) - wetAt(t-vec2(1.7,0.)), wetAt(t+vec2(0.,1.7)) - wetAt(t-vec2(0.,1.7))) * 0.5;
  float em = length(sob);
  // while the film stands its edge is pinned and the rim current runs outward
  // (Deegan); as it dries the contact line depins and recedes, and the film is
  // drawn inward toward the sheet, carrying its pigment with it
  if (em > 1e-4) vel += (sob / em) * em * mix(-0.22, 0.9, uDrying) * hasW;
  // the sheet wets, the page around it does not: as the film dries the contact
  // line recedes down the wettability gradient onto the sheet (Chaudhury &
  // Whitesides 1992), which is what contains the liquid to the scene
  // the page around the sheet is sized and does not wet, so the film is always
  // drawn back onto the sheet; as it dries it gathers onto the next print
  vec2 gf = vec2(foot((t + vec2(4.0, 0.0)) * px) - foot((t - vec2(4.0, 0.0)) * px), foot((t + vec2(0.0, 4.0)) * px) - foot((t - vec2(0.0, 4.0)) * px));
  vec2 gt = vec2(footT((t + vec2(4.0, 0.0)) * px) - footT((t - vec2(4.0, 0.0)) * px), footT((t + vec2(0.0, 4.0)) * px) - footT((t - vec2(0.0, 4.0)) * px));
  vel += (gf * 1.2 + gt * 3.0 * uDrying) * hasW;
  vel *= mix(0.82, 0.96, clamp(h * 6.0, 0.0, 1.0));
  float spd = length(vel); if (spd > 2.4) vel *= 2.4 / spd;
  // into the fibres, and along them
  float da = min(h, 0.006 * pap.g * (1.0 - s)); h -= da; s += da * 1.3;
  float sl = (W(t+vec2(1,0)).a + W(t-vec2(1,0)).a + W(t+vec2(0,1)).a + W(t-vec2(0,1)).a) * 0.25;
  s += 0.10 * (sl - s) * (0.7 + 0.6 * pap.b);
  // evaporation, fastest at the rim, and quick off the sheet
  // pinned rims evaporate fastest; a receding one dries like the rest of the film
  h -= uEvap * (1.0 + 2.5 * em * 8.0 * (1.0 - 0.8 * uDrying) + 6.0 * (1.0 - f));
  s = clamp(s * (1.0 - 0.006) * (1.0 - uCure), 0.0, 1.0);
  h = max(h, 0.0) * (1.0 - uCure);
  o = vec4(min(h, 1.6), vel, s);
}`;

// Pigment: one liquid (suspended ink density, rgb), the fixed deposit (rgb),
// and the sheet's record of how much of the old print has dissolved (l).
const PIG = HEAD + `
uniform sampler2D uW, uS, uD;
uniform sampler2D uNear, uWhole;
uniform float uTime, uCure, uLift, uAds, uMix, uMixG, uDrying, uStir, uRelift, uLoad, uTakeFloor, uTakeL0, uTakeL1, uMixHold, uHomog;
uniform vec3 uMean;
layout(location=0) out vec4 oS; layout(location=1) out vec4 oD;
vec4 S(vec2 t){ return texture(uS, t / uSize); }
vec2 curlN(vec2 p){
  vec2 q = p / 46.0 + vec2(0.0, uTime * 0.45); float e = 1.6;
  float n0 = texture(uPaper, q / 7.0).r, nx = texture(uPaper, (q + vec2(e / 46.0, 0.0)) / 7.0).r, ny = texture(uPaper, (q + vec2(0.0, e / 46.0)) / 7.0).r;
  return vec2(ny - n0, n0 - nx) * (46.0 / e) * 0.05;
}
void main(){
  vec2 t = gl_FragCoord.xy; vec2 px = 1.0 / uSize;
  vec4 w = texture(uW, vUv); float h = w.r; vec2 vel = w.gb; float wet = wetOf(h);
  // the liquid rides the flow plus unresolved convection
  vec2 velP = vel + curlN(t) * 1.6 * smoothstep(0.025, 0.28, h);
  vec3 s = S(t - velP).rgb;
  // What the sheet has taken is not yet bound: while the film is wet and moving,
  // the settled pigment is dragged along with it (a fraction of the flow the
  // liquid follows), and it binds as the sheet cures. So a print does not appear
  // in place at full sharpness: it comes in smeared with the current and
  // gathers into its strokes as the film stops, the mirror of the dissolve.
  // The sheet's record of what has dissolved (l) belongs to the place, not the pigment.
  float mob = wet * (1.0 - uCure) * 0.6;
  vec4 dd = texture(uD, vUv); float l = dd.a;
  vec3 d = texture(uD, (t - velP * mob) / uSize).rgb;
  vec4 pap = P(t);
  // molecular diffusion while wet, and the stirring of the film at large
  // scale: the liquid relaxes toward its neighbourhood's mean and, more
  // slowly, toward the whole film's mean, so it becomes one colour
  // what mixes is concentration (pigment per volume), so a thin rim holds
  // little pigment and the veil fades to the water's edge instead of outlining it
  float h0 = 0.02; float hc = max(h, h0);
  vec4 wl = texture(uW, vUv + vec2(px.x, 0.0)), wr = texture(uW, vUv - vec2(px.x, 0.0)), wu = texture(uW, vUv + vec2(0.0, px.y)), wd = texture(uW, vUv - vec2(0.0, px.y));
  // exchange between neighbours is driven by the concentration difference and
  // limited by the thinner of the two films, so it conserves pigment and a
  // thin rim neither gains nor gives much
  // Mixing is the work of the flow, not a constant: an eddy diffusivity goes as
  // the speed of the film (Prandtl's mixing length, K = l|u|). While the film
  // stands still after the splash the pigment only creeps, and the drops'
  // blooms and the fronts between the colours stay; the drying current then
  // stirs the film (the resolved rim current, and the convection cells that
  // evaporation drives in a drying film, below the grid) and it goes to one
  // colour in a moment, just as the sheet begins to take. Long unmixed,
  // briefly mixed: what a wet-in-wet wash does.
  float agit = 0.12 + 0.88 * clamp(length(vel) * 2.0 + uStir * 0.5 + uDrying, 0.0, 1.0);
  float diffF = clamp(0.45 * (0.35 + h * 45.0), 0.0, 0.45) * wet * agit;
  vec3 c = s / hc;
  vec3 ex = vec3(0.0);
  ex += (S(t+vec2(1,0)).rgb / max(wl.r, h0) - c) * min(wl.r, hc);
  ex += (S(t-vec2(1,0)).rgb / max(wr.r, h0) - c) * min(wr.r, hc);
  ex += (S(t+vec2(0,1)).rgb / max(wu.r, h0) - c) * min(wu.r, hc);
  ex += (S(t-vec2(0,1)).rgb / max(wd.r, h0) - c) * min(wd.r, hc);
  s += diffF * 0.25 * ex;
  // the neighbourhood and whole-film means come from the explicit box
  // downsample below, not from a mip chain: the sim state is never mipmapped
  vec4 nr = texture(uNear, vUv), wh = texture(uWhole, vec2(0.5));
  float hn = nr.a, hw = wh.a;
  vec3 nearc = nr.rgb / max(hn, h0), wholec = wh.rgb / max(hw, h0);
  // The whole-film mean is held back while this texel is still releasing A's
  // ink: a film that is still gaining fresh local pigment has not yet had
  // time to homogenize, and letting it do so is what turned the middle of
  // every morph into one average colour instead of A's composition becoming
  // B's.
  float mixGate = mix(uMixHold, 1.0, smoothstep(uTakeL1, 1.0, l));
  s += uMix * agit * wet * (nearc - c) * min(hn, hc); s += uMixG * mixGate * wet * (wholec - c) * min(hw, hc);
  s = max(s, vec3(0.0));
  // the water re-wets the old print: its ink dissolves into the film, all of it
  vec3 Ao = absorb(texture(uSrcLo, vUv));
  // dissolution is faster where the film moves: flow thins the boundary layer
  float dl = wet * uLift * (1.0 + 2.0 * min(length(vel), 1.5) + uStir) * (1.0 - l) * (0.7 + 0.6 * pap.g);
  // uLoad is concentration released per unit dissolved, not the dissolving
  // rate itself: 1.0 reproduces today's paint-strength ink exactly (the
  // main wash never sets it), while a print with much less inked area than
  // a paint wash (a title's bare strokes against a scene's own photos) can
  // ask for a stronger dose from the same dl without dissolving any faster.
  dl = min(dl, 1.0 - l); l += dl; s += Ao * dl * uLoad;
  // the sheet takes pigment where the next print is: uptake proportional to
  // what is in the liquid and to the capacity still free, per channel
  vec3 cap = absorb(texture(uTgtLo, vUv));
  // the image areas hold more than the print shows while wet (over-inked);
  // the cure brings the excess to the print's own density
  // a wash poured again to go back: the fresh water takes the deposit it had
  // laid down back into suspension, so it can settle where the other print is
  vec3 rl = d * wet * uRelift * (1.0 - uDrying) * (0.6 + 0.8 * pap.g);
  d -= rl; s += rl;
  // The sheet takes pigment where the film has already lifted some: uptake
  // follows the dissolved fraction of THIS texel, so ink that has just left A
  // is available to settle toward B instead of waiting out a clock. The floor
  // is the old clock gate, kept because l never rises where A had no ink --
  // a region B alone occupies would otherwise never deposit at all.
  float take = max(uTakeFloor, smoothstep(uTakeL0, uTakeL1, l));
  vec3 ad = uAds * take * wet * s * max(cap * 1.6 - d, vec3(0.0));
  ad = min(ad, s);
  s -= ad; d += ad;
  // nothing leaves: where the film has gone the pigment in it settles onto the
  // sheet, all of it; the receding front has already carried most of it inward
  float settle = (0.004 + 0.25 * uDrying) * pow(1.0 - wet, 3.0) * clamp(1.0 + (0.5 - pap.r) * 2.0, 0.05, 2.5);
  vec3 st = s * settle; s -= st; d += st;
  // drying: the sheet sharpens onto the print
  // The cure only sharpens the record where the wash actually went: outside
  // the footprint, forcing l to 1 here erased ink the film carried past the
  // silhouette instead of leaving it be.
  l = max(l, uCure * foot(vUv));
  // A recorded dissolve ends in the stirred film's long-time limit: every
  // texel relaxes toward the print's own mean load spread over the wetted
  // sheet (uMean, set per print), the print's undissolved remnant and any
  // deposit joining the liquid at the same rate, so the last step leaves one
  // uniform wash holding exactly the print's pigment. 0 on every other step.
  s += (Ao * (1.0 - l) * uLoad + d) * uHomog; d *= 1.0 - uHomog; l = mix(l, 1.0, uHomog);
  s = mix(s, uMean * foot(vUv), uHomog);
  // Depth rides in the liquid's spare channel, so a stored frame of the film
  // is two textures and still carries the water the display refracts through.
  oS = vec4(min(s * (1.0 - uCure), 8.0), h); oD = vec4(min(d, 8.0), l);
}`;

// Means: each texel the box average of a uBlock-square of (liquid rgb, depth)
// read from uS and uW; run again over its own output for the whole-film mean.
const MEAN = `#version 300 es
precision highp float; precision highp sampler2D;
in vec2 vUv; uniform sampler2D uA, uB; uniform vec2 uInSize; uniform int uBlock; uniform bool uJoin;
out vec4 o;
void main(){
  ivec2 o0 = ivec2(gl_FragCoord.xy) * uBlock; vec4 acc = vec4(0.0); float n = 0.0;
  for (int y = 0; y < 64; y++) { if (y >= uBlock) break; for (int x = 0; x < 64; x++) { if (x >= uBlock) break;
    ivec2 q = o0 + ivec2(x, y); if (q.x >= int(uInSize.x) || q.y >= int(uInSize.y)) continue;
    vec4 a = texelFetch(uA, q, 0); acc += uJoin ? vec4(a.rgb, texelFetch(uB, q, 0).r) : a; n += 1.0; } }
  o = acc / max(n, 1.0);
}`;

// ?diag=N shows one state layer raw and opaque in place of the wash, to see
// which carries an artifact (the Mac's GPU path has shown faults the Linux
// harness cannot); compiled into the display shader only when asked for.
// ?hold=T freezes the wash once its clock passes T seconds, so a state can be looked at.
const DIAG = parseInt(new URLSearchParams(location.search).get('diag') || '0', 10) || 0;
const HOLD = parseFloat(new URLSearchParams(location.search).get('hold') || '') || 0;
const DIAG_VIEW = {
  4: 'exp(-texture(uS, uv).rgb * 3.0)',                     // liquid
  5: 'exp(-texture(uD, uv).rgb)',                           // deposit
  6: 'vec3(texture(uW, uv).r * 3.0)',                       // water depth
  8: 'vec3(texture(uD, uv).a)',                             // how much of the old print has dissolved
  9: 'vec3(0.5 + texture(uW, uv).gb * 0.5, 0.5)',           // velocity: red +x, green +y, grey still
  10: 'vec3(length(gh * 0.03 * wet) * uSize.x * 0.5)',      // refraction offset
}[DIAG];

// Display: the light the sheet takes out of the page, as a multiply layer.
const SHOW = HEAD + `
uniform sampler2D uW, uS, uD; uniform float uCure, uClearance;
out vec4 o;
void main(){
  vec2 uv = vUv; vec2 px = 1.0 / uSize;
  vec4 w = texture(uW, uv); float h = w.r; float wet = wetOf(h);
  // the print seen through standing water bends with the surface
  vec2 gh = vec2(texture(uW, uv + vec2(px.x, 0.0)).r - texture(uW, uv - vec2(px.x, 0.0)).r, texture(uW, uv + vec2(0.0, px.y)).r - texture(uW, uv - vec2(0.0, px.y)).r);
  vec2 at = uv + gh * 0.03 * wet;
  ${DIAG_VIEW ? `o = vec4(${DIAG_VIEW}, 1.0); return;` : ''}
  vec4 dd = texture(uD, uv); float l = dd.a;
  vec3 s = texture(uS, uv).rgb;
  vec2 t = uv * uSize; vec4 pap = P(t);
  float gmod = 1.0 + 0.4 * ((pap.r - 0.5) * 1.4 + (pap.b - 0.5) * 0.6);
  vec3 A = absorb(texture(uSrc, at)) * (1.0 - l);
  // ink is already an optical thickness; the fixed deposit cures onto the print
  float g = max(gmod, 0.05);
  // the suspended pigment shows with its hue stretched about its density: the
  // same darkness, more colour; the deposit keeps the print's own colours
  float sl = dot(s, vec3(1.0 / 3.0)); vec3 sc = max(sl + (s - sl) * 2.2, 0.0);
  // The cure settles the deposit onto the print -- correcting an over- or
  // under-inked film to the print's own density -- but only where the film
  // actually went. Outside the wash's own footprint it must not paint ink
  // out of nothing, which is what made B arrive as a picture rather than as
  // a deposit, and must not erase ink the film carried past the silhouette.
  vec3 dep = dd.rgb * g, tgt = absorb(texture(uTgt, at));
  vec3 ink = min(mix(dep, tgt, uCure * foot(uv)) + 0.85 * sc * g, vec3(4.0));
  A += ink * (1.0 + 0.2 * wet);
  // The darkening P*T over the known page colour P, written as a premultiplied
  // pixel so the canvas is truly clear where nothing is inked or wet: alpha is
  // the deepest channel's absorption and the colour carries the rest.
  // Clear the pigment along the paper grain, without a coloured overlay.
  A *= mix(.22, 1.0, smoothstep(uClearance - .12, uClearance + .12, pap.g));
  vec3 T = clamp(exp(-A), 0.0, 1.0); float a = 1.0 - min(T.r, min(T.g, T.b));
  o = vec4(uTint * (T - (1.0 - a)), a);
}`;

// A scene transition's display (WatercolorMorph): one frame of the film is a
// print plus two stored state textures, (liquid rgb, depth) and (deposit rgb,
// dissolved fraction), and what is shown is two such frames mixed by uW1 --
// neighbouring frames of one recorded dissolve, or the two scenes' well-mixed
// washes. The optical thickness is summed at its true amount, with no hue
// stretch and no wet boost: absorbance is the pigment's own quantity (Beer-
// Lambert), so a wash that holds a print's pigment shows that print's amount
// and colour ratio, and mixing two frames mixes the pigment, not the pixels.
const SHOWK = HEAD + `
uniform sampler2D uS0, uD0, uS1, uD1; uniform float uW1, uWetGainMax; uniform vec3 uK0, uK1;
out vec4 o;
float depth(vec2 uv){ return mix(texture(uS0, uv).a, texture(uS1, uv).a, uW1); }
void main(){
  vec2 uv = vUv; vec2 px = 1.0 / uSize;
  float wet = wetOf(depth(uv));
  vec2 gh = vec2(depth(uv + vec2(px.x, 0.0)) - depth(uv - vec2(px.x, 0.0)), depth(uv + vec2(0.0, px.y)) - depth(uv - vec2(0.0, px.y)));
  vec2 at = uv + gh * 0.03 * wet;
  vec4 pap = P(uv * uSize);
  float g = max(1.0 + 0.4 * ((pap.r - 0.5) * 1.4 + (pap.b - 0.5) * 0.6), 0.05);
  vec4 d0 = texture(uD0, uv), d1 = texture(uD1, uv);
  vec3 A0 = absorb(texture(uSrc, at)) * (1.0 - d0.a) + (d0.rgb + texture(uS0, uv).rgb) * g * uK0;
  vec3 A1 = absorb(texture(uTgt, at)) * (1.0 - d1.a) + (d1.rgb + texture(uS1, uv).rgb) * g * uK1;
  vec3 Am = mix(A0, A1, uW1);
  // Owner (item C): a colourful wet-on-wet bleed is the most stunning part
  // of a dissolve, and it is exactly what a well-mixed wash's own flat mean
  // has none of left. Stretched about the mean the same way SHOW already
  // stretches its own suspended-pigment layer (uWetGainMax is the caller's
  // own cap, kept under that shader's 2.2 -- MOBILE_WET_GAIN_MAX's own
  // comment has the measured value and why it moved past the owner's "up
  // to 1.4" example), scaled by wet itself so a dry pixel (wet=0, s=0 at
  // each dissolve's own step 0) is untouched -- p=0 and p=1 of a leg are
  // always dry, so this never moves either scene's own arrival or rest frame.
  float sat = 1.0 + uWetGainMax * wet;
  float mean = dot(Am, vec3(1.0 / 3.0));
  Am = max(mean + (Am - mean) * sat, 0.0);
  vec3 T = clamp(exp(-min(Am, vec3(4.0))), 0.0, 1.0); float a = 1.0 - min(T.r, min(T.g, T.b));
  o = vec4(uTint * (T - (1.0 - a)), a);
}`;

// A stored frame's pigment, split for the mass fixer: what the film carries
// (liquid and deposit, grained as SHOWK shows them) and what is still in the
// print, per texel at the grid's resolution, for MEAN to reduce.
const FIX = HEAD + `
uniform sampler2D uS, uD;
layout(location=0) out vec4 oL; layout(location=1) out vec4 oR;
void main(){
  vec4 pap = P(gl_FragCoord.xy); vec4 d = texture(uD, vUv);
  float g = max(1.0 + 0.4 * ((pap.r - 0.5) * 1.4 + (pap.b - 0.5) * 0.6), 0.05);
  oL = vec4((d.rgb + texture(uS, vUv).rgb) * g, 1.0); oR = vec4(absorb(texture(uSrcLo, vUv)) * (1.0 - d.a), 1.0);
}`;

// Paper noise pixels: a pure function of nothing (a0=90210 is a fixed seed,
// not a parameter), so both makeSim() instances -- the main wash and the
// title's own small wash -- compute the exact same 256x256 RGBA buffer.
// Memoized here rather than redone identically per instance: this synchronous
// noise generation is where most of the title sim's own arming stall went
// (measured; see setupTitleDissolve's own report), and the main wash's
// instance already runs first, so the title's second call is a cache hit.
let paperPixelsCache = null;
function computePaperPixels() {
  if (paperPixelsCache) return paperPixelsCache;
  const N = 256; let a0 = 90210;
  const rnd = () => { a0 = (a0 + 0x6d2b79f5) | 0; let t = Math.imul(a0 ^ (a0 >>> 15), 1 | a0); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
  const lattice = (n) => { const g = new Float32Array(n * n); for (let i = 0; i < g.length; i++) g[i] = rnd(); return g; };
  const sm = (t) => t * t * (3 - 2 * t);
  const value = (g, n, x, y) => { const gx = x * n, gy = y * n, x0 = Math.floor(gx) % n, y0 = Math.floor(gy) % n, x1 = (x0 + 1) % n, y1 = (y0 + 1) % n, fx = sm(gx - Math.floor(gx)), fy = sm(gy - Math.floor(gy));
    const a = g[y0 * n + x0], b = g[y0 * n + x1], c = g[y1 * n + x0], d = g[y1 * n + x1]; return (a + (b - a) * fx) * (1 - fy) + (c + (d - c) * fx) * fy; };
  const octs = [4, 8, 16, 32, 64, 128];
  const fbm = (lats, x, y, o0) => { let s = 0, amp = 1, tot = 0; for (let o = o0; o < octs.length; o++) { s += amp * value(lats[o], octs[o], x, y); tot += amp; amp *= 0.55; } return s / tot; };
  const L = [0, 1, 2, 3].map(() => octs.map(lattice));
  const ch = [0, 1, 2, 3].map(() => new Float32Array(N * N));
  for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) { const i = y * N + x, u = x / N, v = y / N;
    ch[0][i] = fbm(L[0], u, v, 2) * 0.7 + 0.3 * rnd(); ch[1][i] = fbm(L[1], u, v, 1); ch[2][i] = fbm(L[2], u, v, 0); ch[3][i] = fbm(L[3], u, v, 3); }
  const px = new Uint8Array(N * N * 4);
  for (let k = 0; k < 4; k++) { let lo = 1, hi = 0; for (const t of ch[k]) { lo = Math.min(lo, t); hi = Math.max(hi, t); } for (let i = 0; i < N * N; i++) px[i * 4 + k] = ((ch[k][i] - lo) / (hi - lo)) * 255; }
  paperPixelsCache = px;
  return px;
}
// The title's own second WebGL2 context compiles the same four programs
// makeSim's main-wash instance already compiled once, synchronously
// (gl.compileShader followed immediately by a COMPILE_STATUS read, which
// blocks until the driver finishes). KHR_parallel_shader_compile, where a
// GPU exposes it, lets a driver compile in the background instead: this
// issues the same four compiles on a throwaway context, without ever
// reading COMPILE_STATUS/LINK_STATUS itself, and polls
// COMPLETION_STATUS_KHR (rAF-paced, so it costs nothing while waiting)
// before discarding that context -- most drivers cache a compiled binary by
// source, so makeSim's own later, synchronous compile of the identical
// source for the title's real context is then a cache hit rather than a
// second full compile. A no-op, not a slower path, where the extension
// does not exist: nothing here blocks on anything the browser would not
// have blocked on anyway.
async function warmShaderCache() {
  const canvas = document.createElement('canvas');
  const gl = canvas.getContext('webgl2');
  const ext = gl && gl.getExtension('KHR_parallel_shader_compile');
  if (!gl || !ext) { gl?.getExtension('WEBGL_lose_context')?.loseContext(); return; }
  const programs = [WATER, PIG, SHOW, MEAN].map((fs) => {
    const p = gl.createProgram();
    const vs = gl.createShader(gl.VERTEX_SHADER); gl.shaderSource(vs, V); gl.compileShader(vs);
    const fss = gl.createShader(gl.FRAGMENT_SHADER); gl.shaderSource(fss, fs); gl.compileShader(fss);
    gl.attachShader(p, vs); gl.attachShader(p, fss); gl.linkProgram(p);
    return p;
  });
  await new Promise((resolve) => {
    const poll = () => {
      if (programs.every((p) => gl.getProgramParameter(p, ext.COMPLETION_STATUS_KHR))) resolve();
      else requestAnimationFrame(poll);
    };
    poll();
  });
  gl.getExtension('WEBGL_lose_context')?.loseContext();
}
// One WebGL2 pigment engine per canvas: the page's main wash and the intro
// title's own small wash are two instances of the same factory, sharing every
// shader, constant and step. `rect` is a getter because the main instance's
// print rectangle grows at runtime (dragged plates, scene 3 cards) while the
// title's stays fixed to its own box.
function makeSim({ canvas, texW, texH, rect, load = 1, splashAmp = 0.05, mistAmp = 0.03, mistHold = 0.15, blockSize = 16, paperScale = 1 }) {
  const gl = canvas.getContext('webgl2', { alpha: true, antialias: false, premultipliedAlpha: true, preserveDrawingBuffer: true });
  if (!gl || !gl.getExtension('EXT_color_buffer_float')) return null;
  const compile = (type, src) => { const s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s); if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s)); return s; };
  const W = texW, H = texH;
  const tint = (() => { const c = getComputedStyle(document.documentElement).getPropertyValue('--bg').trim(); const n = parseInt(c.slice(1), 16); return [(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255]; })();
  // absorb() per channel, tabulated by byte alpha then byte value.
  const ABSORB = tint.map((t) => Float32Array.from({ length: 65536 }, (_, i) => -Math.log(Math.max(0.02, 1 + (Math.min(1, (i & 255) / 255 / t) - 1) * (i >> 8) / 255))));
  // the texture units and constants every pass shares are set once, at link;
  // per step only the framebuffers, the state textures and the prints change
  const UNITS = { uW: 0, uS: 1, uD: 2, uNear: 3, uWhole: 4, uS0: 0, uD0: 1, uS1: 2, uD1: 3, uPaper: 8, uSrc: 9, uTgt: 10, uSrcLo: 11, uTgtLo: 12, uSrcFt: 13, uTgtFt: 14 };
  const prog = (fs) => { const p = gl.createProgram(); gl.attachShader(p, compile(gl.VERTEX_SHADER, V)); gl.attachShader(p, compile(gl.FRAGMENT_SHADER, fs)); gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
    const u = {}; const n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS); for (let i = 0; i < n; i++) { const nm = gl.getActiveUniform(p, i).name; u[nm] = gl.getUniformLocation(p, nm); }
    gl.useProgram(p); for (const k in UNITS) if (u[k]) gl.uniform1i(u[k], UNITS[k]);
    if (u.uSize) gl.uniform2f(u.uSize, W, H); if (u.uTint) gl.uniform3f(u.uTint, tint[0], tint[1], tint[2]);
    if (u.uPaperScale) gl.uniform1f(u.uPaperScale, paperScale);
    // uTakeFloor defaults to 0 (pure l-gating) so a caller that never sets it
    // per step keeps that behaviour; the main and title instances set it
    // every step (below) to the old clock value, as a floor. uTakeL0/L1 are
    // dimensionless fractions of the dissolved fraction l, not lengths.
    if (u.uTakeFloor) gl.uniform1f(u.uTakeFloor, 0);
    if (u.uTakeL0) gl.uniform1f(u.uTakeL0, 0.15);
    if (u.uTakeL1) gl.uniform1f(u.uTakeL1, 0.55);
    if (u.uMixHold) gl.uniform1f(u.uMixHold, 0.25);
    if (u.uHomog) gl.uniform1f(u.uHomog, 0);
    return { p, u }; };
  const buf = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
  gl.disable(gl.BLEND);
  // a shader one GPU's compiler rejects must not take the page down with it:
  // the wash falls back to the cut, and the log says which program and why
  let water, pig, show, mean;
  try { water = prog(WATER); pig = prog(PIG); show = prog(SHOW); mean = prog(MEAN); }
  catch (e) { console.error('wash sim unavailable, cutting instead:', e.message); return null; }
  let nStep = 0;
  const tex = (w, h, wrap = gl.CLAMP_TO_EDGE, float = true) => { const t = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, t);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, wrap); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, wrap);
    if (float) gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA16F, w, h, 0, gl.RGBA, gl.HALF_FLOAT, null); return t; };
  const fbo = (texes) => { const f = gl.createFramebuffer(); gl.bindFramebuffer(gl.FRAMEBUFFER, f);
    texes.forEach((t, i) => gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0 + i, gl.TEXTURE_2D, t, 0));
    gl.drawBuffers(texes.map((_, i) => gl.COLOR_ATTACHMENT0 + i)); return f; };
  const wT = [tex(W, H), tex(W, H)], wF = wT.map((t) => fbo([t]));
  const pT = [0, 1].map(() => [tex(W, H), tex(W, H)]), pF = pT.map((ts) => fbo(ts));
  let wi = 0, pi = 0;
  const BLOCK = blockSize, NW = Math.ceil(W / BLOCK), NH = Math.ceil(H / BLOCK);
  const nearT = tex(NW, NH), nearF = fbo([nearT]), wholeT = tex(1, 1), wholeF = fbo([wholeT]);
  const means = (S, Wt) => {
    gl.useProgram(mean.p);
    gl.viewport(0, 0, NW, NH); gl.bindFramebuffer(gl.FRAMEBUFFER, nearF);
    bind(0, S); bind(1, Wt); gl.uniform1i(mean.u.uA, 0); gl.uniform1i(mean.u.uB, 1); gl.uniform2f(mean.u.uInSize, W, H); gl.uniform1i(mean.u.uBlock, BLOCK); gl.uniform1i(mean.u.uJoin, 1);
    draw();
    gl.viewport(0, 0, 1, 1); gl.bindFramebuffer(gl.FRAMEBUFFER, wholeF);
    bind(0, nearT); gl.uniform1i(mean.u.uA, 0); gl.uniform2f(mean.u.uInSize, NW, NH); gl.uniform1i(mean.u.uBlock, 64); gl.uniform1i(mean.u.uJoin, 0);
    draw();
  };

  // Paper: r relief, g absorbency, b fibre, a pore — tileable value noise,
  // seeded, computed once for both sim instances (computePaperPixels, above).
  const paper = tex(256, 256, gl.REPEAT, false);
  gl.bindTexture(gl.TEXTURE_2D, paper);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 256, 256, 0, gl.RGBA, gl.UNSIGNED_BYTE, computePaperPixels());

  // The two prints, straight alpha, flipped so uv (0,0) is the bottom-left as in the sim.
  const prints = { src: null, tgt: null };
  const scaled = (im, w, h) => { const c = document.createElement('canvas'); c.width = w; c.height = h; const g = c.getContext('2d'); g.imageSmoothingQuality = 'high';
    // two halvings at a time keep the box filter honest on every browser
    let cur = im; while (cur.width > w * 2) { const m = document.createElement('canvas'); m.width = Math.ceil(cur.width / 2); m.height = Math.ceil(cur.height / 2); const mg = m.getContext('2d'); mg.imageSmoothingQuality = 'high'; mg.drawImage(cur, 0, 0, m.width, m.height); cur = m; }
    g.drawImage(cur, 0, 0, w, h); return c; };
  const upload = (t, im) => { gl.bindTexture(gl.TEXTURE_2D, t); gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true); gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, im); gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false); };
  const setPrint = (key, im) => { const p = prints[key] || { full: tex(0, 0, gl.CLAMP_TO_EDGE, false), lo: tex(0, 0, gl.CLAMP_TO_EDGE, false), ft: tex(0, 0, gl.CLAMP_TO_EDGE, false) };
    upload(p.full, im); upload(p.lo, scaled(im, W, H)); upload(p.ft, scaled(im, Math.round(W / 4), Math.round(H / 4))); prints[key] = p; };

  const bind = (unit, t) => { gl.activeTexture(gl.TEXTURE0 + unit); gl.bindTexture(gl.TEXTURE_2D, t); };
  const draw = () => gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  const bindPrints = (S, T) => { bind(8, paper); bind(9, S.full); bind(10, T.full); bind(11, S.lo); bind(12, T.lo); bind(13, S.ft); bind(14, T.ft); };
  // a pass with the prints bound in the wash's direction
  const common = (pr, fwd) => {
    const [S, T] = fwd ? [prints.src, prints.tgt] : [prints.tgt, prints.src];
    gl.useProgram(pr.p); bindPrints(S, T);
  };
  // Empty paper as a print, and the film at rest as a stored frame: one
  // transparent texel, which absorb() reads as no ink and the state reads as
  // no liquid, no deposit, nothing dissolved.
  const blank = tex(1, 1, gl.CLAMP_TO_EDGE, false);
  gl.bindTexture(gl.TEXTURE_2D, blank); gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array(4));
  const blankPrint = { full: blank, lo: blank, ft: blank };
  // The one film is shared by every dissolve and the closing wash. `film`
  // says which dissolve it holds and at which step, so a dissolve can carry
  // on from where the film already is; anything else that moves the film
  // (the closing wash's step/reset, another dissolve) makes the next user
  // restore a checkpoint instead.
  let film = null, showk = null, fixer = null, held = [];
  // The mass fixer: a stored frame's liquid is scaled, per channel, so the
  // frame holds exactly its print's pigment. The film's transport is not
  // conservative (semi-Lagrangian advection, and concentration relaxing
  // across an uneven depth), and mid-dissolve it lost up to a fifth of a
  // print (measured) before the stirring limit put it back; a global mass
  // fixer is the standard answer for such a scheme. The frame's liquid and
  // its undissolved remnant are drawn at grid resolution (FIX), box-averaged
  // with MEAN, and the block means read back and weighted by the texels each
  // block really holds: the grid's last row and column of blocks are partial,
  // and an unweighted mean of block means is off by as much as 8% (phone).
  const FIX_EVERY = 6;   // divides every checkpoint spacing in use
  const fixScale = (rec) => {
    if (!fixer) {
      const L = tex(W, H), R = tex(W, H), nearT2 = tex(NW, NH);
      fixer = { p: prog(FIX), L, R, F: fbo([L, R]), nearF: fbo([nearT2]) };
    }
    gl.viewport(0, 0, W, H); gl.useProgram(fixer.p.p); bindPrints(rec.pr, blankPrint); gl.bindFramebuffer(gl.FRAMEBUFFER, fixer.F);
    bind(1, pT[pi][0]); bind(2, pT[pi][1]); draw();
    const reduce = (t) => {
      gl.useProgram(mean.p);
      gl.viewport(0, 0, NW, NH); gl.bindFramebuffer(gl.FRAMEBUFFER, fixer.nearF);
      bind(0, t); gl.uniform1i(mean.u.uA, 0); gl.uniform2f(mean.u.uInSize, W, H); gl.uniform1i(mean.u.uBlock, BLOCK); gl.uniform1i(mean.u.uJoin, 0);
      draw();
      const px = new Float32Array(NW * NH * 4); gl.readPixels(0, 0, NW, NH, gl.RGBA, gl.FLOAT, px);
      const sum = [0, 0, 0];
      for (let by = 0; by < NH; by++) for (let bx = 0; bx < NW; bx++) {
        const n = Math.min(BLOCK, W - bx * BLOCK) * Math.min(BLOCK, H - by * BLOCK), i = (by * NW + bx) * 4;
        for (let c = 0; c < 3; c++) sum[c] += px[i + c] * n;
      }
      return sum.map((v) => v / (W * H));
    };
    const liquid = reduce(fixer.L), remnant = reduce(fixer.R);
    // Up to 32, not 2: a print that is one small disc (scene 4's control)
    // loses most of its ink mid-dissolve and needed 3.2 on desktop and 14 on
    // the phone's coarser grid (measured) to hold it.
    return rec.target.map((m, c) => liquid[c] > 1e-5 ? Math.min(32, Math.max(0.5, (m - remnant[c]) / liquid[c])) : 1);
  };
  const clearState = () => {
    gl.viewport(0, 0, W, H); gl.clearColor(0, 0, 0, 0);
    for (const f of [...wF, ...pF, nearF, wholeF]) { gl.bindFramebuffer(gl.FRAMEBUFFER, f); gl.clear(gl.COLOR_BUFFER_BIT); }
  };
  const fitCanvas = () => {
    const r = rect();
    const dpr = Math.min(2, devicePixelRatio || 1);
    const cw = Math.round(r.w * dpr), chh = Math.round(r.h * dpr);
    if (canvas.width !== cw || canvas.height !== chh) { canvas.width = cw; canvas.height = chh; }
    canvas.style.left = r.x + 'px'; canvas.style.top = r.y + 'px'; canvas.style.right = 'auto'; canvas.style.bottom = 'auto';
    canvas.style.width = r.w + 'px'; canvas.style.height = r.h + 'px';
    gl.bindFramebuffer(gl.FRAMEBUFFER, null); gl.viewport(0, 0, cw, chh);
  };
  // One step of a print dissolving on its own, toward nothing: no uptake (the
  // target is paper), no drying, the sheet kept wet, the film stirred harder
  // as it goes, and from a third of the way the stirring's limit (uHomog)
  // pulling it to one uniform wash that the last step reaches exactly. The
  // pull closes the remaining gap evenly over the last steps (1 / steps
  // left) rather than all in the last one: every step of the film is now a
  // frame someone sees, and a last-step snap was the largest jump in a slow
  // scroll (deltaE 13 to 20 in the worst cell, measured).
  const stepDissolve = (rec, n) => {
    const t = n * DT, u = (n + 1) / rec.N;
    gl.viewport(0, 0, W, H);
    gl.useProgram(water.p); bindPrints(rec.pr, blankPrint); gl.bindFramebuffer(gl.FRAMEBUFFER, wF[1 - wi]);
    bind(0, wT[wi]);
    gl.uniform1f(water.u.uSplash, t < T_SPLASH ? splashAmp : 0); gl.uniform1f(water.u.uMist, u < 0.6 ? mistAmp : 0); gl.uniform1f(water.u.uCure, 0);
    gl.uniform1f(water.u.uEvap, 0.0008); gl.uniform1f(water.u.uDrying, 0); gl.uniform2f(water.u.uTilt, 0, 0);
    draw(); wi = 1 - wi;
    // on the record's own count, so a print's dissolve is the same every time it is recorded
    if (n % 3 === 0) means(pT[pi][0], wT[wi]);
    gl.viewport(0, 0, W, H);
    gl.useProgram(pig.p); bindPrints(rec.pr, blankPrint); gl.bindFramebuffer(gl.FRAMEBUFFER, pF[1 - pi]);
    bind(0, wT[wi]); bind(1, pT[pi][0]); bind(2, pT[pi][1]); bind(3, nearT); bind(4, wholeT);
    gl.uniform1f(pig.u.uTime, t); gl.uniform1f(pig.u.uCure, 0);
    gl.uniform1f(pig.u.uLift, 0.075); gl.uniform1f(pig.u.uAds, 0); gl.uniform1f(pig.u.uTakeFloor, 0);
    gl.uniform1f(pig.u.uMix, 0.06); gl.uniform1f(pig.u.uMixG, 0.005 + 0.05 * smooth(0.2, 0.8, u));
    gl.uniform1f(pig.u.uDrying, 0); gl.uniform1f(pig.u.uStir, smooth(0.05, 0.4, u)); gl.uniform1f(pig.u.uRelift, 0);
    gl.uniform1f(pig.u.uLoad, load);
    gl.uniform1f(pig.u.uHomog, Math.max(0.035 * smooth(0.3, 0.95, u), 1 / (rec.N - n)));
    gl.uniform3f(pig.u.uMean, rec.mean[0], rec.mean[1], rec.mean[2]);
    draw(); pi = 1 - pi;
  };

  // Reversal keyframe ring: GPU-only, no readback. Four slots at p in {0,
  // .25, .5, .75}, allocated on first capture so an instance that never
  // reverses never pays for it.
  const KF_PS = [0, 0.25, 0.5, 0.75];
  let kf = null;
  const kfEnsure = () => { if (kf) return; kf = KF_PS.map(() => {
    const wTx = tex(W, H), pT0x = tex(W, H), pT1x = tex(W, H);
    return { t: -1, wF: fbo([wTx]), pF0: fbo([pT0x]), pF1: fbo([pT1x]) };
  }); };
  // dstBuffers, given, targets one attachment of pF[pi] (which otherwise
  // keeps both live for PIG's own MRT output); every other destination here
  // is single-attachment already and needs no override.
  const blit = (srcFbo, srcAttachment, dstFbo, dstBuffers) => {
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, srcFbo); gl.readBuffer(srcAttachment);
    gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, dstFbo); if (dstBuffers) gl.drawBuffers(dstBuffers);
    gl.blitFramebuffer(0, 0, W, H, 0, 0, W, H, gl.COLOR_BUFFER_BIT, gl.NEAREST);
  };
  // A checkpoint is the film's whole state at one step of a dissolve: the
  // water (depth, velocity, saturation) as well as the pigment and deposit,
  // since stepping on from anything less would not be the same film. The
  // coarse means need no copy: a checkpoint falls on a step where
  // stepDissolve recomputes them before they are read.
  const saveCheckpoint = (rec, n) => {
    const c = { W: tex(W, H), S: tex(W, H), D: tex(W, H) };
    c.fW = fbo([c.W]); c.fS = fbo([c.S]); c.fD = fbo([c.D]);
    blit(wF[wi], gl.COLOR_ATTACHMENT0, c.fW); blit(pF[pi], gl.COLOR_ATTACHMENT0, c.fS); blit(pF[pi], gl.COLOR_ATTACHMENT1, c.fD);
    rec.ck.set(n, c);
  };
  const loadCheckpoint = (c) => {
    blit(c.fW, gl.COLOR_ATTACHMENT0, wF[wi]);
    blit(c.fS, gl.COLOR_ATTACHMENT0, pF[pi], [gl.COLOR_ATTACHMENT0, gl.NONE]);
    blit(c.fD, gl.COLOR_ATTACHMENT0, pF[pi], [gl.NONE, gl.COLOR_ATTACHMENT1]);
    gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, pF[pi]); gl.drawBuffers([gl.COLOR_ATTACHMENT0, gl.COLOR_ATTACHMENT1]);
  };
  // Puts the film at step s of a dissolve and returns the steps that took:
  // from the film itself when it already holds this dissolve between s's
  // checkpoint and s, else from that checkpoint. Replay is the real
  // simulation with fixed inputs, so step s is the same state whichever way
  // it was reached, going forward or coming back.
  const seek = (rec, s) => {
    const base = s - s % rec.every;
    if (!(film && film.rec === rec && film.n >= base && film.n <= s)) {
      if (base) loadCheckpoint(rec.ck.get(base)); else clearState();
      film = { rec, n: base };
    }
    let count = 0;
    while (film.n < s) { stepDissolve(rec, film.n); film.n++; count++; }
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    return count;
  };
  // A frame to show: the print at rest, a checkpoint, or the film itself.
  const frameTextures = ({ rec, s }) => {
    if (!s) return [blank, blank, [1, 1, 1]];
    const k = Array.from(rec.k.subarray(s * 3, s * 3 + 3)), c = rec.ck.get(s);
    if (c) return [c.S, c.D, k];
    if (film?.rec === rec && film.n === s) return [pT[pi][0], pT[pi][1], k];
    throw new Error(`dissolve step ${s} is neither stored nor on the film`);
  };
  return {
    setPrints(src, tgt) { setPrint('src', src); setPrint('tgt', tgt); held = [src, tgt];
      // A new pair invalidates every stored keyframe: it belongs to the
      // wash between these two specific prints, and setPrints always starts
      // a new one.
      if (kf) for (const k of kf) k.t = -1;
    },
    // Blits the live state into ring slot `slot`, recording it at `t`.
    captureKeyframe(slot, t) {
      kfEnsure();
      const k = kf[slot];
      blit(wF[wi], gl.COLOR_ATTACHMENT0, k.wF);
      blit(pF[pi], gl.COLOR_ATTACHMENT0, k.pF0);
      blit(pF[pi], gl.COLOR_ATTACHMENT1, k.pF1);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      k.t = t;
    },
    // Called after every forward step; captures whichever of KF_PS the sim
    // has newly reached. Safe to call unconditionally -- a slot already at
    // or past its own threshold is left alone.
    maybeCaptureKeyframe(t) {
      kfEnsure();
      for (let i = 0; i < KF_PS.length; i++) { const at = KF_PS[i] * T_TOTAL; if (t >= at && kf[i].t < at) this.captureKeyframe(i, t); }
    },
    // Restores the newest keyframe at or before goal, if any qualifies, and
    // returns the t it was captured at; null (nothing restored) tells the
    // caller to fall back to reset().
    restoreNearestKeyframe(goal) {
      if (!kf) return null;
      let best = null;
      for (const k of kf) if (k.t >= 0 && k.t <= goal && (!best || k.t > best.t)) best = k;
      if (!best) return null;
      film = null;
      blit(best.wF, gl.COLOR_ATTACHMENT0, wF[wi]);
      blit(best.pF0, gl.COLOR_ATTACHMENT0, pF[pi], [gl.COLOR_ATTACHMENT0, gl.NONE]);
      blit(best.pF1, gl.COLOR_ATTACHMENT0, pF[pi], [gl.NONE, gl.COLOR_ATTACHMENT1]);
      gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, pF[pi]); gl.drawBuffers([gl.COLOR_ATTACHMENT0, gl.COLOR_ATTACHMENT1]); // PIG's own MRT output needs both live again
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      return best.t;
    },
    reset() { film = null; clearState(); },
    holds: (src, tgt) => held[0] === src && held[1] === tgt,
    // A print's own dissolve, from the print at rest to its well-mixed wash,
    // `steps` steps long, kept as a checkpoint every `every` steps (a
    // multiple of 3, the means' period) and the mass fixer's scale for every
    // step; nothing runs until advanceRecord. The grid-resolution print the
    // film dissolves from is averaged in absorbance, not in colour: the log
    // of an averaged colour understates the ink of any detail finer than a
    // texel (by a sixth on the phone's grid, measured), and the film would
    // lose that much the moment it dissolved. The mean load is the print's ink over the share of the grid
    // the wash covers as the shaders draw it -- the footprint sil() makes,
    // fibre noise included, times the display's grain -- so the uniform wash
    // the record ends in shows exactly the print's pigment, channel by channel.
    recordDissolve(im, steps, every) {
      const ft = scaled(im, Math.round(W / 4), Math.round(H / 4)), fw = ft.width, fh = ft.height;
      const full = im.getContext('2d').getImageData(0, 0, im.width, im.height).data, iw = im.width, ih = im.height;
      const acc = new Float64Array(W * H * 3), cnt = new Float64Array(W * H), total = [0, 0, 0];
      for (let y = 0; y < ih; y++) {
        const gy = Math.min(H - 1, Math.floor((ih - 1 - y) * H / ih));   // flipped: the grid's row 0 is the print's bottom
        for (let x = 0; x < iw; x++) {
          const i = (y * iw + x) * 4, a = full[i + 3] << 8, j = gy * W + Math.min(W - 1, Math.floor(x * W / iw));
          cnt[j]++;
          if (a) for (let c = 0; c < 3; c++) { const A = ABSORB[c][a | full[i + c]]; acc[j * 3 + c] += A; total[c] += A; }
        }
      }
      // The same thickness written back as a colour absorb() reads straight back.
      const lo = new Uint8Array(W * H * 4);
      for (let j = 0; j < W * H; j++) {
        for (let c = 0; c < 3; c++) lo[j * 4 + c] = Math.round(255 * tint[c] * Math.exp(-acc[j * 3 + c] / Math.max(1, cnt[j])));
        lo[j * 4 + 3] = 255;
      }
      const pp = computePaperPixels(), ftp = ft.getContext('2d').getImageData(0, 0, fw, fh).data;
      const texel = (ch, x, y) => pp[(((y % 256) + 256) % 256 * 256 + ((x % 256) + 256) % 256) * 4 + ch] / 255;
      const bilinear = (fx, fy, get) => { const x0 = Math.floor(fx), y0 = Math.floor(fy), tx = fx - x0, ty = fy - y0;
        return (get(x0, y0) * (1 - tx) + get(x0 + 1, y0) * tx) * (1 - ty) + (get(x0, y0 + 1) * (1 - tx) + get(x0 + 1, y0 + 1) * tx) * ty; };
      const alphaAt = (x, y) => ftp[((fh - 1 - Math.min(fh - 1, Math.max(0, y))) * fw + Math.min(fw - 1, Math.max(0, x))) * 4 + 3] / 255;
      let share = 0;
      for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
        const tx = (x + 0.5) * paperScale, ty = (y + 0.5) * paperScale;
        const pap = (ch) => texel(ch, Math.floor(tx), Math.floor(ty)) * 0.55 + bilinear((tx / 977 + 0.37) * 256 - 0.5, (ty / 977 + 0.37) * 256 - 0.5, (u, v) => texel(ch, u, v)) * 0.45;
        const pr = pap(0), pg = pap(1), pb = pap(2);
        const f = smooth(0.5, 0.9, bilinear((x + 0.5) / W * fw - 0.5, (y + 0.5) / H * fh - 0.5, alphaAt) + (pg - 0.5) * 0.35 + (pb - 0.5) * 0.15);
        share += f * Math.max(1 + 0.4 * ((pr - 0.5) * 1.4 + (pb - 0.5) * 0.6), 0.05);
      }
      share = Math.max(1e-3, share / (W * H));
      const pr = { lo: tex(0, 0, gl.CLAMP_TO_EDGE, false), ft: tex(0, 0, gl.CLAMP_TO_EDGE, false) }; pr.full = pr.lo;
      gl.bindTexture(gl.TEXTURE_2D, pr.lo); gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, W, H, 0, gl.RGBA, gl.UNSIGNED_BYTE, lo);
      upload(pr.ft, ft);
      const target = total.map((m) => m / (iw * ih));
      return { pr, target, mean: target.map((m) => m / share), N: steps, every, n: 0, ck: new Map(), k: new Float32Array((steps + 1) * 3).fill(1) };
    },
    // Records up to `budget` steps of a dissolve (in whole checkpoint
    // intervals, so a record interrupted between two calls resumes from its
    // own last checkpoint with nothing lost); returns the steps run. Complete
    // once n === N.
    advanceRecord(rec, budget) {
      if (rec.n >= rec.N || budget <= 0) return 0;
      let count = seek(rec, rec.n);
      while (rec.n < rec.N && count < budget) {
        // The fixer reads back from the GPU, which stalls it (2 ms a step on
        // an Apple GPU, measured, against 0.4 for the step), so it is read
        // every FIX_EVERY steps and the scale between is interpolated: it
        // varies as slowly as the losses it corrects, except early in a small
        // print's dissolve, which a checkpoint's spacing was too coarse for.
        for (let i = 0; i < rec.every; i++) {
          stepDissolve(rec, rec.n); rec.n++; film.n = rec.n; count++;
          if (rec.n % FIX_EVERY) continue;
          const n0 = rec.n - FIX_EVERY, k0 = rec.k.slice(n0 * 3, n0 * 3 + 3), k1 = fixScale(rec);
          for (let j = 1; j <= FIX_EVERY; j++) for (let c = 0; c < 3; c++) rec.k[(n0 + j) * 3 + c] = k0[c] + (k1[c] - k0[c]) * j / FIX_EVERY;
        }
        saveCheckpoint(rec, rec.n);
      }
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      return count;
    },
    seek,
    disposeRecord(rec) {
      if (film?.rec === rec) film = null;
      gl.deleteTexture(rec.pr.lo); gl.deleteTexture(rec.pr.ft);
      for (const c of rec.ck.values()) for (const k of ['W', 'S', 'D']) { gl.deleteTexture(c[k]); gl.deleteFramebuffer(c['f' + k]); }
      rec.ck.clear();
    },
    // Shows two frames mixed by w. A frame is { slot: 'src' | 'tgt', rec, s }:
    // the print held in that slot (setPrints) at step s of its dissolve, s = 0
    // being the print at rest; a step between checkpoints must be the one the
    // film was last seeked to.
    // wetGainMax: the SHOW shader's own colour-in-the-bleed gain, 0 for the
    // desktop shape (identical to before this uniform existed) and set by
    // watercolor-morph.js's own per-layout bounds for mobile.
    present(f0, f1, w, wetGainMax = 0) {
      if (!showk) showk = prog(SHOWK);
      const [s0, d0, k0] = frameTextures(f0), [s1, d1, k1] = frameTextures(f1);
      fitCanvas();
      gl.useProgram(showk.p);
      bind(8, paper); bind(9, prints[f0.slot].full); bind(10, prints[f1.slot].full);
      bind(0, s0); bind(1, d0); bind(2, s1); bind(3, d1);
      gl.uniform1f(showk.u.uW1, w);
      gl.uniform1f(showk.u.uWetGainMax, wetGainMax);
      gl.uniform3f(showk.u.uK0, k0[0], k0[1], k0[2]); gl.uniform3f(showk.u.uK1, k1[0], k1[1], k1[2]);
      draw();
    },
    // One fixed step at time t (seconds). The schedule: flood, dissolve and
    // stir, take up, dry and cure.
    step(fwd, t, cure, stir = 0, tilt = 0, relift = 0) {
      film = null;
      const splash = t < T_SPLASH ? splashAmp : 0, mist = t < T_SPLASH + mistHold ? mistAmp : 0;
      // a hot-air blast: evaporation many times the rate of standing air, which
      // thins the film, drives the rim current and settles the pigment quickly
      const drying = smooth(T_DRY, T_DRY + 0.4, t);
      // no longer the gate: PIG takes up wherever a texel's own dissolved
      // fraction says to, so this clock is now only the floor under that --
      // the guarantee that uptake starts at least this early even where A
      // never had ink for `l` to track.
      const take = smooth(T_TAKE, T_TAKE + 0.45, t);
      gl.viewport(0, 0, W, H);
      common(water, fwd); gl.bindFramebuffer(gl.FRAMEBUFFER, wF[1 - wi]);
      bind(0, wT[wi]);
      gl.uniform1f(water.u.uSplash, splash); gl.uniform1f(water.u.uMist, mist); gl.uniform1f(water.u.uCure, cure);
      gl.uniform1f(water.u.uEvap, 0.0008 + 0.016 * drying); gl.uniform1f(water.u.uDrying, drying); gl.uniform2f(water.u.uTilt, 0, tilt);
      draw(); wi = 1 - wi;
      // the coarse levels of liquid and pigment give the neighbourhood and
      // whole-film concentrations; the stirring is slow, so every third step is enough
      if (nStep++ % 3 === 0) means(pT[pi][0], wT[wi]);
      gl.viewport(0, 0, W, H);
      common(pig, fwd); gl.bindFramebuffer(gl.FRAMEBUFFER, pF[1 - pi]);
      bind(0, wT[wi]); bind(1, pT[pi][0]); bind(2, pT[pi][1]); bind(3, nearT); bind(4, wholeT);
      gl.uniform1f(pig.u.uTime, t); gl.uniform1f(pig.u.uCure, cure);
      gl.uniform1f(pig.u.uLift, 0.075); gl.uniform1f(pig.u.uAds, 0.18);
      gl.uniform1f(pig.u.uTakeFloor, take);
      // the whole film is stirred by the drying current (and by the hand that scrolls), not before
      gl.uniform1f(pig.u.uMix, 0.06); gl.uniform1f(pig.u.uMixG, 0.005 + 0.025 * Math.max(drying, Math.min(1, stir)));
      gl.uniform1f(pig.u.uDrying, drying); gl.uniform1f(pig.u.uStir, stir); gl.uniform1f(pig.u.uRelift, relift);
      gl.uniform1f(pig.u.uLoad, load); gl.uniform1f(pig.u.uHomog, 0);
      draw(); pi = 1 - pi;
    },
    // read one texel of the state, for the harness: [h,u,v,sat], s.rgb, [d.rgb,l]
    probe(x, y) {
      const out = [];
      for (const [f, n] of [[wF[wi], 1], [pF[pi], 2]]) { gl.bindFramebuffer(gl.FRAMEBUFFER, f);
        for (let i = 0; i < n; i++) { gl.readBuffer(gl.COLOR_ATTACHMENT0 + i); const px = new Float32Array(4); gl.readPixels(x, y, 1, 1, gl.RGBA, gl.FLOAT, px); out.push([...px].map((v) => +v.toFixed(4))); } }
      return out;
    },
    // How much of the old print has not yet dissolved, weighted by where it
    // had ink (mask/maskTotal, at this instance's own W x H, GL's row order).
    // l only ever grows within a forward wash, so this is monotone by
    // construction where a rendered-pixel read is not: a bloom can raise
    // total on-screen coverage while it is lifting the print off its own
    // letterforms, and a real GPU's step-to-step rate is not bit-identical
    // across engines, so a display-shader proxy can wobble at a shared
    // sample point even though the wash is strictly further along.
    remaining(mask, maskTotal) {
      gl.bindFramebuffer(gl.FRAMEBUFFER, pF[pi]);
      gl.readBuffer(gl.COLOR_ATTACHMENT1);
      const buf = new Float32Array(W * H * 4);
      gl.readPixels(0, 0, W, H, gl.RGBA, gl.FLOAT, buf);
      let dot = 0;
      for (let i = 0; i < W * H; i++) dot += mask[i] * (1 - buf[i * 4 + 3]);
      return dot / maskTotal;
    },
    // the display pass onto the canvas
    draw(fwd, cure, clearance = -.2) {
      fitCanvas();
      common(show, fwd);
      bind(0, wT[wi]); bind(1, pT[pi][0]); bind(2, pT[pi][1]);
      gl.uniform1f(show.u.uCure, cure);
      gl.uniform1f(show.u.uClearance, clearance);
      draw();
    },

  };
}
// Phones display the wash at less than half its design size. The page's own
// canvas and print rectangle are today's exact values; only the factory above
// is new.
const sim = makeSim({ canvas, texW: Math.round(BASE_TEX_W / (mobileLayout() ? 4 : 2)), texH: Math.round(BASE_TEX_H / (mobileLayout() ? 4 : 2)), rect: () => printRect });
// Every scene transition, both layouts (site/watercolor-morph.js). Each frame
// blends the two recorded checkpoints around its exact scroll position in
// pigment space. CHECKPOINT_EVERY therefore bounds storage and interpolation
// distance without requiring a second replay state. A checkpoint
// is three RGBA16F textures of the grid, 3 x 410 x 390 x 8 B = 3.8 MB on
// desktop, so ten a print make 38 MB, a leg's two prints 77 MB and the three
// the page keeps 115 MB; the phone's grid is a quarter of that area, 29 MB
// for three.
// DISSOLVE_STEPS is a dissolve's length, 1.5 sim-seconds at DT.
const DISSOLVE_STEPS = 180, CHECKPOINT_EVERY = 18;
// Owner (item C): "can the morph stay longer in the state of not-yet-well-
// mixed wash... a colourful wet-on-wet wash that is bleeding is the most
// stunning, and we want to keep that." Mobile-only bounds (desktop keeps
// watercolor-morph.js's own DEFAULT_BOUNDS): the middle phase stays broad
// (0.4-0.6) so dissolution and consolidation read as one continuous wet
// field, while MOBILE_STEP_EASE softens the outer phase's step curve without
// pinning a fast touch scroll to its low steps. MOBILE_WET_GAIN_MAX
// is the SHOW shader's own saturation gain cap at full wetness (present()
// in makeSim, SHOWK) for the same reason: colour where the bleed is still
// wet, never on a dry pixel (p=0/1 of a leg are always dry, so the live
// scenes' own arrival and rest colours are untouched).
//
// Measured (legs 0->1, 1->2; scratchpad/mobile-round measure-itemC.mjs):
// structured share (fraction of sampled p with luminance std >= 12 over a
// 48x36 downsample of #gl, alpha-weighted) 0.29 -> 0.73 to 0.78 and 0.22 ->
// 0.68 to 0.71 across repeated runs; chroma at p=0.25 relative to each
// leg's own settled start 48-62 -> 68-108 and 62-75 -> 105-115. ease=2 and
// wetGainMax=0.9
// (gain up to 1.9, short of SHOW's own 2.2) are both past the brief's own
// "for example" numbers: a smaller gain (tried at 0.4, 0.55, 0.7) left leg
// 0->1's chroma under its own start's on some runs, on a machine measured
// at 100+ concurrent chromium processes from other agents during this
// session, which the repeated-run spread above is itself evidence of --
// 0.9 was the value that held a positive margin across every repeat tried.
const MOBILE_STEP_EASE = 2, MOBILE_WET_GAIN_MAX = 0.9;
const MOBILE_MORPH_BOUNDS = { A_END: 0.4, B_START: 0.6, ease: MOBILE_STEP_EASE, wetGainMax: MOBILE_WET_GAIN_MAX };
const morph = sim && WatercolorMorph.create(sim, { steps: DISSOLVE_STEPS, every: CHECKPOINT_EVERY, mobile: MOBILE_MORPH_BOUNDS });
// Wherever the cursor is, a scroll gesture moves the page and nothing else. The
// editor and the preview are live documents with their own scroll containers,
// so each frame has its own scrolling switched off and the browser chains the
// gesture to the page on its own; hover, cursor and clicks are untouched.
// It used to cancel the wheel inside each frame and call `scrollBy` for it,
// which both chained and scrolled: measured 2026-09-14, a wheel over the
// composition moved the page twice as far as the same wheel over the margin,
// and a touch drag over it moved nothing at all. Cancelling the reader's own
// gesture is the one thing this page may never do.
{
  const inert = (doc) => {
    if (!doc || doc.__inert) return; doc.__inert = true;
    // the preview itself stays a real page: it scrolls under the wheel, and
    // hands the wheel up to this page only when it has no further to go
    const fe = doc.defaultView && doc.defaultView.frameElement;
    if (window.__LANG === 'zh' && (fe?.id === 'ed' || fe?.id === 'sh')) {
      const hant = window.__LANDING_LOCALE === 'zh-hant';
      const strings = hant ? {
        'New page':'新增頁面', 'Toggle full width':'切換全寬', 'Edit page':'編輯頁面',
        'Go back':'返回', 'Go forward':'前進', 'Toggle desktop/mobile preview':'切換桌面／行動版預覽',
        'Settings':'設定', 'Website':'網站', 'Add channel':'新增管道', 'Publish site':'發布網站',
        'Open editor':'開啟編輯器', 'Import from URL':'從 URL 匯入',
        'Publish':'發布', 'Publish site':'發布網站', 'Cancel':'取消', 'Confirm':'確認', 'Later':'稍後',
        'Close':'關閉', 'Visit site':'造訪網站', 'Your site is live':'你的網站已上線', 'Settings':'設定',
        'This Page':'此頁面', 'Child Pages':'子頁面', 'Live preview':'即時預覽', 'Source':'原始碼',
        'Writing mode':'寫作模式', 'Untitled':'未命名', 'New Page':'新增頁面', 'New Folder':'新增資料夾',
      } : {
        'New page':'新建页面', 'Toggle full width':'切换全宽', 'Edit page':'编辑页面',
        'Go back':'后退', 'Go forward':'前进', 'Toggle desktop/mobile preview':'切换桌面／移动版预览',
        'Settings':'设置', 'Website':'网站', 'Add channel':'添加渠道', 'Publish site':'发布网站',
        'Open editor':'打开编辑器', 'Import from URL':'从 URL 导入',
        'Publish':'发布', 'Publish site':'发布网站', 'Cancel':'取消', 'Confirm':'确认', 'Later':'稍后',
        'Close':'关闭', 'Visit site':'访问网站', 'Your site is live':'你的网站已上线', 'Settings':'设置',
        'This Page':'此页面', 'Child Pages':'子页面', 'Live preview':'实时预览', 'Source':'源代码',
        'Writing mode':'写作模式', 'Untitled':'未命名', 'New Page':'新建页面', 'New Folder':'新建文件夹',
      };
      const translateTree = (root) => {
        if (!root) return;
        if (root.nodeType === 3) { const value = strings[root.nodeValue.trim()]; if (value) root.nodeValue = root.nodeValue.replace(root.nodeValue.trim(), value); return; }
        const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT);
        const nodes = []; while (walker.nextNode()) nodes.push(walker.currentNode);
        nodes.forEach((node) => { const value = strings[node.nodeValue.trim()]; if (value) node.nodeValue = node.nodeValue.replace(node.nodeValue.trim(), value); });
      };
      translateTree(doc.body);
      for (const el of doc.querySelectorAll('[data-tooltip]')) {
        const value = strings[el.getAttribute('data-tooltip')];
        if (value) el.setAttribute('data-tooltip', value);
      }
      for (const el of doc.querySelectorAll('[aria-label]')) {
        const value = strings[el.getAttribute('aria-label')];
        if (value) el.setAttribute('aria-label', value);
      }
      const translateAttrs = (el) => {
        if (!el || el.nodeType !== 1) return;
        for (const attr of ['data-tooltip', 'aria-label', 'placeholder', 'title']) {
          const value = strings[el.getAttribute(attr)];
          if (value) el.setAttribute(attr, value);
        }
      };
      new MutationObserver((records) => records.forEach((record) => { translateAttrs(record.target); if (record.type === 'childList') record.addedNodes.forEach(translateTree); })).observe(doc, { subtree: true, childList: true, attributes: true, attributeFilter: ['data-tooltip', 'aria-label', 'placeholder'] });

      doc.documentElement.lang = hant ? 'zh-Hant' : 'zh-Hans';
    }
    if (fe && fe.id === 'ed') {
      const style = doc.createElement('style');
      style.textContent = '.landing-playback .insert-bar, .landing-playback .editor-band { opacity: .42; pointer-events: none; transition: none !important; } .landing-playback .insert-bar *, .landing-playback .editor-band * { transition: none !important; }';
      doc.head.appendChild(style);
    }
    if (fe && fe.id === 'sh') {
      const preview = doc.getElementById('moss-preview-iframe');
      const suppressPreviewLinks = () => {
        const previewDoc = preview && preview.contentDocument;
        if (!previewDoc || previewDoc.__landingLinksInert) return;
        previewDoc.__landingLinksInert = true;
        previewDoc.addEventListener('click', (event) => {
          if (event.target.closest?.('a')) { event.preventDefault(); event.stopImmediatePropagation(); }
        }, true);
      };
      if (preview) { preview.addEventListener('load', suppressPreviewLinks); suppressPreviewLinks(); }
    }
    // and it is always the light theme: the scene is a light page, whatever the OS prefers
    // and it shows no scrollbar: a classic scrollbar (a mouse plugged into a
    // Mac shows one) narrows the live page by 15px against its print, which is
    // laid out with none, and the picture shifted as the sheet cured
    if (fe && fe.id === 'moss-preview-iframe') {
      doc.documentElement.dataset.theme = 'light';
      const st = doc.createElement('style');
      st.textContent = 'html, body[data-typesetting="vertical"] { scrollbar-width: none; } html::-webkit-scrollbar, body[data-typesetting="vertical"]::-webkit-scrollbar { display: none; }';
      doc.head.appendChild(st);
      // The harvested vertical page keeps native horizontal trackpad scrolling.
      // A conventional mouse wheel moves its columns, without a visible bar.
      if (doc.body?.dataset.typesetting === 'vertical') doc.addEventListener('wheel', (event) => {
        if (event.ctrlKey || Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return;
        const body = doc.body, before = body.scrollLeft;
        const delta = event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? body.clientWidth : 1);
        body.scrollLeft -= delta;
        event.stopImmediatePropagation();
        if (body.scrollLeft !== before) event.preventDefault();
      }, { passive: false, capture: true });
      return;
    }
    doc.documentElement.style.overflow = 'hidden'; if (doc.body) doc.body.style.overflow = 'hidden';
    doc.querySelectorAll('iframe').forEach((f) => { const go = () => inert(f.contentDocument); f.addEventListener('load', go); if (f.contentDocument && f.contentDocument.readyState === 'complete') go(); });
  };
  for (const id of ['ed', 'sh', 'vd']) { const f = document.getElementById(id); const go = () => inert(f.contentDocument); f.addEventListener('load', go); if (f.contentDocument && f.contentDocument.readyState === 'complete') go(); }
}
// The editor's surface takes the page colour: the wash is a multiply layer and
// cannot paint a surface lighter than the page, so the live editor and its
// print must agree on it.
{ const f = document.getElementById('ed');
  const tint = () => f.contentDocument.documentElement.style.setProperty('--moss-color-bg', getComputedStyle(document.documentElement).getPropertyValue('--bg').trim());
  f.addEventListener('load', tint); if (f.contentDocument && f.contentDocument.readyState === 'complete') tint(); }

// ── Scenes: the pinned visual shows one scene at a time ───────────────────
// shown: the scene on the sheet (0 write, 1 live). target: the scene the copy
// on screen asks for. A change runs the wash to completion; if the target
// flips mid-wash the wash finishes and a second one runs back.
let shown = 0, target = 0, phase = 'idle', steps = 0;
let ed = null, sh = null, vd = null;
// A scene change in flight. A wash announces itself by holding the phase at
// 'morph', but neither of the other two mechanisms does — the crossfade leaves
// the phase where it was, on purpose, and a jump puts the scenes it passes
// through on the page — so the drive says so itself, and nothing may start a
// second one under any of them.
let driving = false;
const running = () => phase === 'morph' || driving;
// The pacing, all of it, so a complaint about how the page feels is a change to
// these numbers and not to the model.
// The wash's clock runs from 0 to T_TOTAL in sim-seconds, and each T_ is a mark
// on it: T_SPLASH the first drops, T_TAKE where the print begins to smear,
// T_DRY and T_CURE where the paper takes and sets, and T_WET where the scroll's
// own work ends — the film at its wettest, the print fully smeared. From T_WET
// to T_TOTAL is the cure, which belongs to time and not to the hand. DT is one
// step of that clock and STEPS_PER_FRAME the most a frame may spend on it;
// V_TAU is the memory of the scroll-speed integrator that agitates and tilts
// the film.
// Scene transitions no longer run on this clock (WatercolorMorph presents
// them by scroll position); the closing wash and the title still do. ARRIVE
// and ARRIVE_SMEAR, in sim-seconds per real second, survive only in
// cureLeft(), the reduced-motion settle's estimate of a wash's remaining time.
const DT = 1 / 120, STEPS_PER_FRAME = 36, V_TAU = 0.4,
  T_SPLASH = 0.2, T_TAKE = 1.2, T_DRY = 1.2, T_CURE = .9, T_WET = 1.6, T_TOTAL = 2.1,
  ARRIVE = 6, ARRIVE_SMEAR = 1;
// The rest. REST_MS is how long the scroll must have been still to count as a
// stop. A wheel is nudges rather than a stream — a reader turning it slowly
// leaves a fifth of a second between them — and the old catch window could
// afford 140ms because it only ever fired near a designed position and in the
// direction of travel. This one acts on any rest anywhere, so the window has to
// be longer than the gap inside a gesture or it cuts one in half: at 140ms a
// steady scroll across one boundary turned into two washes, the second one back
// (harness/jump.mjs, 2026-09-14). The copy is then carried to the scene's
// designed position on a critically damped spring — SETTLE_K is the stiffness
// the prior art starts from (SwiftUI's half-second spring at mass 1) and
// SETTLE_C the damping that is critical at that stiffness, 2*sqrt(K). It is not
// the 17 the same source names beside 160: that is its bounce-0.3 pairing, and
// a bounce is the one thing an arrival may not have. SETTLE_SECS is the
// spring's own duration, which follows from K, and is used only when no wash is
// owed; when one is, the settle borrows the wash's clock and runs for what the
// cure has left, so the copy comes to rest in the same beat the film sets
// rather than on a clock of its own (measured within a third of a second either
// way, against a second and more for a fixed duration). SETTLE_MIN is the floor
// on that, for a cure with almost nothing left to run. It does not fold into
// SETTLE_SECS: the only ratio between them that reproduces 0.12 is a curve fit
// to that number, not a derivation of it, and would read as a reason where
// there isn't one — checked 2026-09-15, kept as two constants.
// DEAD is the dead zone, in fractions of a gap, that still counts a stop as
// arrival rather than as onward travel: once a reader scrolls with intent
// they almost always want the next scene in the direction they were going,
// so a rest carries forward to it, and only a stop still this close to the
// scene behind that travel springs back — a small nudge, not a change of
// mind. This is paging, not proximity: iOS's UIScrollView, Android's
// PagerSnapHelper, and GSAP ScrollTrigger's own `snap: { directional: true }`
// all carry a deliberate scroll onward rather than resting on whichever
// position is numerically nearer (research/2026-09-14/scroll-rest-physics.md,
// §4). It happens to equal SETTLE_MIN above; the two answer unrelated
// questions — one a fraction of a gap's distance, the other a floor on the
// spring's own seconds — and share no derivation, so moving one is not
// expected to move the other.
// watchScrollDesktop's own critically damped pull toward the well ahead
// (research/2026-09-16/continuous-carry.md) — a release hands the reader's own speed to the
// integrator, so the page keeps going like a let-go spring. The same ζ=1 ratio as SETTLE_K/
// SETTLE_C, named separately because this one runs every frame on live real-time dt for the
// page's whole life, not the settle's rate-scaled, wash-synced clock.
// How much of one wheel tick's pixel delta becomes velocity (px/s), added straight into the
// integrator's v the instant the tick arrives — "v += delta * PUSH_GAIN",
// research/2026-09-16/continuous-carry.md §5. Tuned by feel, not derived: too low reads as
// inert, too high as a teleport across scenes.
// How far the live scroll position may drift from watchScrollDesktop's own tracked position
// before that reads as a foreign write — a scrollbar drag, or keyboard/touch scrolling, both
// left outside this system — rather than ordinary rounding from the integrator's own last
// write.
// How long a gap between trackpad wheel ticks still reads as the same continuous gesture in
// contact with the pad, used only where WheelEvent.momentum is unavailable (see the held-state
// comment above the wheel listener) — a real gesture's own ticks land well inside this, a pause
// or the coast after release does not.

const CARD_SETTLE_K = 160;
function springStep(x, v, dt, k) {
  const c = 2 * Math.sqrt(k);
  for (let n = Math.max(1, Math.ceil(dt * 240)), h = dt / n; n-- > 0;) { v += (-k * x - c * v) * h; x += v * h; }
  return [x, v];
}
function sceneClasses(scene) { stage.classList.toggle('live', scene >= 1); stage.dataset.scene = String(scene); }
function editorPlayback(active) {
  const doc = edFrame.contentDocument;
  if (doc) { doc.documentElement.classList.toggle('landing-playback', active); if (active) doc.querySelector('.insert-bar')?.classList.add('dim', 'visible'); }
}
// The grounds' opacity: the shadow of the scene being shown, faded as it
// dissolves and returned as the next one consolidates (pour, by position).
function ground(a, b) { stage.style.setProperty('--g0', a.toFixed(3)); stage.style.setProperty('--g1', b.toFixed(3)); }
// There are two sheet shapes, not four: the editor's, and the widened one that
// scenes 2 and 3 share. So a 2 to 3 wash never touches the ground at all, and
// scene 4 is under neither of them — the sheet itself has gone and what is left
// on the paper is the control, which carries no window shadow.
const GROUND = [[1, 0], [0, 1], [0, 1], [0, 0], [0, 0]];
function groundAt(scene) { ground(...GROUND[scene]); }

// ── Scene 4's control: the shell's own, measured rather than copied ───────
// Where the control is and how big it is are read from the shell, never written
// here: its frame is a fixed cellW+SPILL by cellH whatever the box is doing, so
// the rect the shell reports is already in the box's own coordinates. What the
// stylesheet needs from it is the circle to cut (centre and radius) and the
// distance from there to the middle of the cell, where the grown control lands
// and where every target orbits.
const fanEl = $('fan');
let pubR = 25;
function measurePub() {
  const doc = vdFrame.contentDocument;
  const btn = doc && doc.querySelector('.moss-publish-button');
  const r = btn && btn.getBoundingClientRect();
  if (!r || !r.width) return false;
  const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
  pubR = r.width / 2;
  for (const [k, v] of Object.entries({ '--pub-cx': cx + 'px', '--pub-cy': cy + 'px', '--pub-r': pubR + 'px',
    '--pub-dx': (GEOM.cellW / 2 + SPILL - cx) + 'px', '--pub-dy': (GEOM.cellH / 2 - cy) + 'px' }))
    stage.style.setProperty(k, v);
  return true;
}

// ── Scene 2's Publish cue: a ring that leaves the window ──────────────────
// Replaces the old cue (site owner, 2026-09-20): a small pulsing ring plus a
// "Try publishing" label, both injected into shell.html's own document and
// clipped by #box's overflow:hidden long before they could read as "the
// whole periodical radiates outward". This version lives in a body-level
// fixed layer -- a sibling of #stage, never a descendant -- so it can grow
// past every ancestor's overflow:hidden on its way to and past the window
// edge, and so it is never folded into a print (fold() only walks #stage).
// Event-driven, not polled (R8: nothing runs at rest): synced from scenes()'s
// own dispatch (2026-09-21, dropping an earlier setInterval(...,250) that
// worked but ran forever) plus resize, a reduced-motion change, and the
// button's own click -- never a timer or a rAF loop of its own, so there is
// nothing left running once the reader is actually at rest. `phase` is
// scenes()'s own state, already exactly "which named scene is current",
// true for the whole of scene 2's own idle animation and false the instant a
// wash to elsewhere starts (scenes('morph') runs before that wash's first
// frame) -- a more precise signal than the shown/target/running() this cue
// used to poll, and one this cue only ever reads. "Activated" lives as a
// dataset flag on #pub-cue itself rather than a new top-level binding.
const pubCue = $('pub-cue');
function pubCueButton() {
  const doc = shFrame.contentDocument;
  const btn = doc && doc.querySelector('.moss-publish-button');
  if (!btn) return null;
  if (!btn.dataset.pubCueBound) {
    btn.dataset.pubCueBound = '1';
    btn.addEventListener('click', () => { pubCue.dataset.activated = '1'; syncPubCue(); });
    // "nothing to publish on a cold boot" starts this button disabled;
    // liveLoop's own breathe/growTo cycle enables and disables it again as
    // the mock edits it drives come and go. Not a scene change, a resize, or
    // a click -- a MutationObserver is the event-driven way to notice it
    // rather than a poll.
    new MutationObserver(syncPubCue).observe(btn, { attributes: true, attributeFilter: ['disabled', 'class'] });
  }
  return btn.disabled ? null : btn;
}
function syncPubCue() {
  if (!pubCue) return;
  const btn = !mobileLayout() && !pubCue.dataset.activated && phase === 'live' && !document.hidden ? pubCueButton() : null;
  if (!btn) { pubCue.classList.remove('on'); return; }
  // The button lives in an iframe with its own layout viewport; its own
  // getBoundingClientRect() is in THAT viewport, not this fixed layer's.
  // shFrame's own rect (in this document) versus its unscaled layout size
  // gives the one ratio that maps iframe-local px onto real screen px,
  // whatever #cell's own --s scale currently is.
  const outer = shFrame.getBoundingClientRect();
  const scale = outer.width / (shFrame.offsetWidth || 1);
  const r = btn.getBoundingClientRect();
  const cx = outer.left + (r.left + r.width / 2) * scale, cy = outer.top + (r.top + r.height / 2) * scale;
  const r0 = Math.max(1, (r.width / 2) * scale);
  pubCue.style.setProperty('--pub-ring-cx', cx + 'px');
  pubCue.style.setProperty('--pub-ring-cy', cy + 'px');
  pubCue.style.setProperty('--pub-ring-r0', String(r0));
  // The scale that carries the ring's own radius past every one of the four
  // edges: the farthest one from the button's own screen position, plus one
  // more radius so the ring's stroke itself has fully crossed it too.
  pubCue.style.setProperty('--pub-ring-k', String(Math.max(cx, innerWidth - cx, cy, innerHeight - cy) / r0 + 1));
  pubCue.classList.add('on');
}
if (pubCue) {
  addEventListener('resize', syncPubCue);
  addEventListener('visibilitychange', syncPubCue);
  matchMedia('(prefers-reduced-motion: reduce)').addEventListener('change', syncPubCue);
}

// ── Scene 4's targets: one circle per deploy target, run by d3-force ──────
// The deploy target list itself is never hardcoded here: scene4/logos/ is a
// rights audit in progress, other hands add and re-verify entries in
// targets.json while this scene only reads it. Each entry's own `file` and
// `allowed_on_circle` decide whether a circle shows a real mark or the
// fallback name — see buildCircle — so a target moving from unclear to
// cleared needs no code change on this side either.
let TARGETS = [];
let targetsPromise = null;
const loadTargets = () => targetsPromise || (targetsPromise = fetch('scene4/logos/targets.json').then((r) => r.json())
  .catch((e) => { console.warn('scene 4: targets.json failed to load, no targets to show', e && e.message); return []; }));
// Fired now, well before the reader could plausibly reach scene 4 (it is the
// fourth of five), so by the time startOrbit awaits it the promise is
// normally already settled — a small same-origin JSON file, not something
// worth gating behind the scene entry the way d3-force itself is.
loadTargets();

// Popping, colliding and huddling are all governed by a real d3-force
// simulation (forceRadial at radius 0 — a point attraction toward the
// control, not a ring to keep to — forceCollide against overlap, with the
// control itself in the node list as a fixed body per the brief, and a
// hand-rolled mutual pull between the targets themselves below; nothing here
// is a keyframed ring). The module is fetched lazily on the scene's first
// entry, so nothing pays for the physics engine before the reader has
// scrolled this far, and only d3-force and its own dependencies come down,
// never the rest of d3. ORBIT_D is a target's own width on the page;
// ORBIT_STAGGER and ORBIT_STAGGER_SWING set how far apart pops land —
// alternating short and long like a heartbeat's own two beats around that
// average, never a metronome's even tick, and both ends of the swing still
// under one pop a second; ORBIT_POP_MS one pop's spring; ORBIT_ATTRACT how
// hard a target is drawn toward the control; ORBIT_HUDDLE a little of that
// same pull between the targets themselves, so a settled pack leans into
// itself rather than sitting in one even, lattice-like ring; ORBIT_ALPHA_DRAG
// is how much hotter the simulation runs while the reader holds one target;
// ORBIT_OUT_MS is the targets' own retreat into the control on exit — well
// inside FAN_OUT_MS's budget for the whole scene to have stood down before
// the sheet may re-wet; ORBIT_TANGENT is each new arrival's own clockwise
// velocity, ORBIT_SPEED_MAX bounds collision energy without friction below
// that ceiling, and ORBIT_RETURN_MS is the gentle path home after a drag.
// A pacing complaint here is a change to these numbers.
const ORBIT_D = mobileLayout() ? 60 : 44, ORBIT_STAGGER = 1250, ORBIT_STAGGER_SWING = 150, ORBIT_POP_MS = 480;
const ORBIT_ATTRACT = 0.1, ORBIT_HUDDLE = 0.35, ORBIT_ALPHA_DRAG = 0.4, ORBIT_OUT_MS = 380;
const ORBIT_TANGENT = 0.42, ORBIT_SPEED_MAX = 0.65, ORBIT_RETURN_MS = 1100, ORBIT_ALPHA_IDLE = 0.025;
document.documentElement.style.setProperty('--orbit-d', ORBIT_D + 'px');

const orbitCx = GEOM.cellW / 2, orbitCy = GEOM.cellH / 2;
const buttonR = () => pubR * PUB_SCALE;
// Where a target rests once it has packed against the control: touching its
// edge, not standing off at a fixed ring radius — the "bump directly" the
// brief asks for. Read fresh each time rather than cached, since buttonR()
// itself can change (a resize re-measures the control) and nothing here may
// repeat forceCollide's own known bug of pinning a radius at its first read.
const restR = () => buttonR() + ORBIT_D / 2;
// Target i's own even slice of the ring, starting at 12 o'clock — used only
// for reduced motion's static layout (startOrbit, at restR()). Full motion's
// targets land at a random arrival angle instead (popAll) and pack against
// the control and each other from there, so no fixed slice governs where
// they end up.
const ringAngle = (i, n) => (i / n) * Math.PI * 2 - Math.PI / 2;
// A back-out ease: overshoots past 1 before settling, which is the "spring"
// a target's pop reads as. Position itself needs no such curve — it is the
// simulation's own attraction that carries a target out from the control.
const easeOutBack = (t) => { const c1 = 1.70158, c3 = c1 + 1, p = t - 1; return 1 + c3 * p * p * p + c1 * p * p; };
const popScale = (n, now) => { if (!n.popAt) return 0; const t = clamp01((now - n.popAt) / ORBIT_POP_MS); return t >= 1 ? 1 : easeOutBack(t); };
// A released drag returns without a snap or exaggerated overshoot.
const easeOutCubic = (t) => 1 - (1 - t) ** 3;

let d3fPromise = null, orbitStartPromise = null, orbitStartToken = 0, orbitExitFrame = 0;
function loadVendorScript(src) {
  return new Promise((resolve, reject) => {
    const script = document.createElement('script');
    script.src = src; script.async = false;
    script.onload = resolve;
    script.onerror = () => reject(new Error(`could not load ${src}`));
    document.head.appendChild(script);
  });
}
// Load the checked-in official UMD builds in dependency order. Keeping these
// local makes the interaction work offline and avoids CDN ESM rewrites whose
// bare dependency URLs resolve against this site in some browsers.
const loadD3Force = () => d3fPromise || (d3fPromise = (async () => {
  for (const file of ['d3-dispatch.min.js', 'd3-quadtree.min.js', 'd3-timer.min.js', 'd3-force.min.js']) {
    await loadVendorScript(`vendor/d3-force/${file}`);
  }
  return window.d3;
})());

// The targets' own mutual pull, separate from their shared attraction to the
// control: without it, attraction plus collide alone settle into one evenly
// spaced ring, which reads as a lattice rather than a huddle. A weak pairwise
// draw between neighbours — inverse-distance, and cut off past a few
// diameters so it is a local lean rather than the whole cluster bunching into
// one corner — lets targets lean on each other instead, the "jostle, settle,
// nudge" the director asked for. Collide still has the final word on actual
// overlap; this only decides who leans toward whom while it does.
function makeHuddle() {
  let nodes = [];
  function force(alpha) {
    for (let i = 0; i < nodes.length; i++) {
      const a = nodes[i];
      if (a.fx != null || !a.popAt || a.exiting) continue;
      for (let j = i + 1; j < nodes.length; j++) {
        const b = nodes[j];
        if (b.fx != null || !b.popAt || b.exiting) continue;
        const dx = b.x - a.x, dy = b.y - a.y, dist = Math.hypot(dx, dy) || 1;
        if (dist > ORBIT_D * 3) continue;
        const k = ORBIT_HUDDLE * alpha / dist;
        const fx = dx * k, fy = dy * k;
        a.vx += fx; a.vy += fy; b.vx -= fx; b.vy -= fy;
      }
    }
  }
  force.initialize = (ns) => { nodes = ns; };
  return force;
}
// Collision can concentrate several targets' momentum into one. Preserve
// frictionless motion below this ceiling while preventing a crowded contact
// from catapulting one target out of the composition.
function makeSpeedLimit(max) {
  let nodes = [];
  function force() {
    for (const n of nodes) {
      const speed = Math.hypot(n.vx, n.vy);
      if (speed > max) { n.vx *= max / speed; n.vy *= max / speed; }
    }
  }
  force.initialize = (ns) => { nodes = ns; };
  return force;
}

// pointer coordinates -> the cell's own 660×620 space, whatever --s has
// scaled the composition to on screen
function toCell(x, y) {
  const r = $('cell').getBoundingClientRect();
  return { x: (x - r.left) * (GEOM.cellW / r.width), y: (y - r.top) * (GEOM.cellH / r.height) };
}
let dragging = null;
// A released drag's own gentle path back into the pack. fx/fy stay pinned to
// the tween itself, exactly like a
// synthetic drag, so collide keeps pushing any target already sitting where
// this one is about to land — the same force that resolves two targets
// dropped on each other (the collision test) resolves a bounced-back one
// arriving into a crowded pack. Independent of the simulation's own tick
// cadence for the same reason stopOrbit's retreat is: a fixed, authored curve
// is what this one motion asks for, not more simulation.
function bounceBack(node) {
  const from = { x: node.fx, y: node.fy };
  const th = Math.atan2(from.y - orbitCy, from.x - orbitCx);
  const to = { x: orbitCx + restR() * Math.cos(th), y: orbitCy + restR() * Math.sin(th) };
  const t0 = performance.now(), g = gen;
  const step = () => {
    if (gen !== g || dragging === node || node.exiting) return;
    const t = clamp01((performance.now() - t0) / ORBIT_RETURN_MS), e = easeOutCubic(t);
    node.fx = from.x + (to.x - from.x) * e;
    node.fy = from.y + (to.y - from.y) * e;
    if (orbitNodes) render();
    if (t < 1) requestAnimationFrame(step);
    else {
      node.fx = null; node.fy = null;
      node.vx = -Math.sin(th) * ORBIT_TANGENT;
      node.vy = Math.cos(th) * ORBIT_TANGENT;
    }
  };
  requestAnimationFrame(step);
}
// The standard d3 drag pattern, by hand rather than by importing d3-drag: on
// down, fx/fy pin the target to the pointer and the simulation is asked to
// run hotter so its neighbours give way; on release the pin lifts and
// bounceBack returns the target over a readable interval — reduced motion
// skips that path and lands directly. Pointer capture
// keeps the events coming even once the pointer has left the target's own
// small circle, and touch-action:none on the element (styled above) is what
// stops a touch drag from also scrolling the page.
function bindDrag(el, node) {
  // render() is called directly too, not only left to the simulation's own
  // tick: a drag started before d3-force has finished loading would otherwise
  // move fx/fy with nothing to paint it, since nothing ticks yet.
  const move = (e) => { e.preventDefault(); const p = toCell(e.clientX, e.clientY); node.fx = p.x; node.fy = p.y; if (orbitNodes) render(); };
  const release = (e) => {
    el.removeEventListener('pointermove', move);
    try { el.releasePointerCapture(e.pointerId); } catch {}
    if (dragging === node) dragging = null;
    if (reduce) {
      const th = Math.atan2(node.fy - orbitCy, node.fx - orbitCx);
      node.x = orbitCx + restR() * Math.cos(th); node.y = orbitCy + restR() * Math.sin(th);
      node.fx = null; node.fy = null;
      if (orbitSim) { orbitSim.alphaTarget(0); render(); }
      return;
    }
    if (orbitSim) orbitSim.alpha(Math.max(orbitSim.alpha(), 0.2)).alphaTarget(ORBIT_ALPHA_IDLE).restart();
    bounceBack(node);
  };
  el.addEventListener('pointerdown', (e) => {
    if (node.exiting || e.button > 0) return;
    e.preventDefault();
    try { el.setPointerCapture(e.pointerId); } catch {}   // no capture, no fallout: the listeners below still track this pointer by its move/up
    dragging = node;
    const p = toCell(e.clientX, e.clientY);
    node.fx = p.x; node.fy = p.y;
    if (orbitNodes) render();
    if (orbitSim) orbitSim.alpha(Math.max(orbitSim.alpha(), ORBIT_ALPHA_DRAG)).alphaTarget(ORBIT_ALPHA_DRAG).restart();
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', release, { once: true });
    el.addEventListener('pointercancel', release, { once: true });
  });
}
// Shrinks a fallback label's own type until its wrap fits inside the circle,
// rather than truncating it: a name set in the page's own typeface reads as
// a name and is honest about not being the target's mark, where two letters
// ("CF") used to read as a bad fake logo. Measured against the element's own
// rendered layout — it must already be attached to the document for this to
// mean anything, which is why buildCircle only creates the element and
// ensureOrbitNodes calls this after appending it — since the page's type is
// proportional and no two labels cost the same width at the same size.
const FALLBACK_MAX_PX = 10, FALLBACK_MIN_PX = 7, FALLBACK_STEP_PX = 0.5;
function fitFallback(el) {
  let px = FALLBACK_MAX_PX;
  el.style.fontSize = px + 'px';
  // Two lines' worth at the current size (line-height is the 1.05 ratio set
  // in CSS), not a fraction of the circle — a fixed height budget would let
  // several short, dense lines slip through as "small enough" even though
  // that reads as three or four lines, not two. A name long enough to still
  // overflow this at FALLBACK_MIN_PX is left to wrap past two lines rather
  // than truncated or invented-abbreviated — there is no shorter form of it
  // on record to fall back to.
  while (px > FALLBACK_MIN_PX && el.scrollHeight > px * 1.05 * 2 + 0.5) {
    px -= FALLBACK_STEP_PX;
    el.style.fontSize = px + 'px';
  }
}
// `display_file` records an explicit product decision to show the official
// source asset. It stays separate from the provenance audit's
// `allowed_on_circle`: displaying a mark must not rewrite that evidence into
// a permission claim. The fallback remains in the DOM in case an image fails.
function buildCircle(t, node) {
  const el = document.createElement('div');
  el.className = 'orbit-circle'; el.dataset.id = t.id;
  const planned = t.availability === 'planned';
  const accessibleLabel = `${t.label}${planned ? ' — planned' : ''}`;
  el.setAttribute('role', 'img'); el.setAttribute('aria-label', accessibleLabel);
  el.title = accessibleLabel;
  const fb = document.createElement('span'); fb.className = 'orbit-fallback'; fb.textContent = t.label.toLowerCase();
  const logoFile = t.display_file || (t.allowed_on_circle === true && t.file ? t.file : null);
  if (logoFile) {
    const img = document.createElement('img');
    img.alt = ''; img.draggable = false; img.decoding = 'async';
    img.addEventListener('load', () => el.classList.add('has-logo'));
    img.addEventListener('error', () => img.remove());
    img.src = `scene4/logos/${logoFile}`;
    el.append(img, fb);
  } else el.append(fb);
  bindDrag(el, node);
  return el;
}

// Every position a target's element shows on the page — orbiting, mid-pop, or
// tweening back into the control — reads off orbitNodes here and nowhere
// else, so there is exactly one place that turns "where is it" into pixels.
let orbitSim = null, orbitNodes = null, orbitEls = null, buttonNode = null, orbitCollide = null;
function render(now = performance.now()) {
  for (let i = 0; i < orbitNodes.length; i++) {
    const n = orbitNodes[i], el = orbitEls[i];
    let x = n.x, y = n.y, s, op;
    if (n.exiting) {
      const t = clamp01((now - n.exitAt) / ORBIT_OUT_MS), e = smooth(0, 1, t);
      x = n.exitFrom.x + (orbitCx - n.exitFrom.x) * e;
      y = n.exitFrom.y + (orbitCy - n.exitFrom.y) * e;
      s = n.exitFrom.s * (1 - e); op = 1 - e;
    } else { s = popScale(n, now); op = n.popAt ? 1 : 0; }
    el.style.transform = `translate(${(x - ORBIT_D / 2).toFixed(1)}px, ${(y - ORBIT_D / 2).toFixed(1)}px) scale(${Math.max(0, s).toFixed(3)})`;
    el.style.opacity = op.toFixed(3);
  }
}
// Staggers the release of one target at a time, each at its own random
// angle off the control's edge — collide and attraction carry it the rest of
// the way into the pack, so the spread the reader sees is the simulation's,
// not a layout this function computed. Only the new target receives the
// clockwise tangent; an arrival never shakes the standing pack.
async function popAll(g) {
  for (let i = 0; i < orbitNodes.length; i++) {
    if (gen !== g) return;
    const n = orbitNodes[i], th = Math.random() * Math.PI * 2;
    const r0 = Math.max(1, buttonR() - ORBIT_D * 0.6);
    n.spawnAngle = th; n.spawnR = r0;
    n.x = orbitCx + r0 * Math.cos(th); n.y = orbitCy + r0 * Math.sin(th);
    n.vx = -Math.sin(th) * ORBIT_TANGENT; n.vy = Math.cos(th) * ORBIT_TANGENT;
    n.spawnVx = n.vx; n.spawnVy = n.vy;
    n.fx = null; n.fy = null; n.popAt = performance.now();
    if (orbitCollide) orbitCollide.radius((d) => d === buttonNode ? buttonR() : d.popAt ? ORBIT_D / 2 : 0);
    if (i < orbitNodes.length - 1) await wait(i % 2 === 0 ? ORBIT_STAGGER - ORBIT_STAGGER_SWING : ORBIT_STAGGER + ORBIT_STAGGER_SWING);
  }
}
function ensureOrbitNodes() {
  if (orbitNodes) {
    for (const n of orbitNodes) { n.x = orbitCx; n.y = orbitCy; n.fx = orbitCx; n.fy = orbitCy; n.vx = 0; n.vy = 0; n.popAt = 0; n.exiting = false; }
    return;
  }
  orbitNodes = TARGETS.map(() => ({ x: orbitCx, y: orbitCy, vx: 0, vy: 0, fx: orbitCx, fy: orbitCy, popAt: 0, exiting: false }));
  orbitEls = orbitNodes.map((n, i) => {
    const el = buildCircle(TARGETS[i], n);
    fanEl.appendChild(el);
    const fb = el.querySelector('.orbit-fallback');
    if (fb) fitFallback(fb);   // needs a real layout box, hence after appendChild
    return el;
  });
}
async function startOrbit() {
  if (orbitSim) return;
  if (orbitStartPromise) return orbitStartPromise;
  cancelAnimationFrame(orbitExitFrame); orbitExitFrame = 0;
  const startToken = ++orbitStartToken;
  const run = (async () => {
  const g = gen;
  TARGETS = TARGETS.length ? TARGETS : await loadTargets();
  if (gen !== g || startToken !== orbitStartToken) return;   // the reader left before the target list resolved
  ensureOrbitNodes();
  if (reduce) {
    // The static pack an unwilling-to-move reader gets, standing the instant
    // the scene does rather than once a network fetch resolves: every target
    // already touching the control at its own even slice, nothing popped in
    // and nothing turning. d3-force is still loaded below (so a drag has a
    // simulation to collide and settle against), but nothing about the
    // reader's first look at the scene waits on it.
    let placed = 0, ring = 0;
    while (placed < orbitNodes.length) {
      const radius = restR() + ring * ORBIT_D;
      const capacity = Math.max(1, Math.floor(Math.PI * 2 * radius / (ORBIT_D * 1.05)));
      const count = Math.min(capacity, orbitNodes.length - placed);
      for (let j = 0; j < count; j++) {
        const n = orbitNodes[placed + j], th = ringAngle(j, count) + ring * 0.21;
        n.x = orbitCx + radius * Math.cos(th); n.y = orbitCy + radius * Math.sin(th);
        n.fx = null; n.fy = null; n.popAt = -Infinity;
      }
      placed += count; ring++;
    }
  }
  render();
  let mod;
  try { [mod] = await Promise.all([loadD3Force(), reduce ? null : wait(PUB_MS + PUB_BEAT)]); }
  catch (e) { console.warn('scene 4: d3-force failed to load, targets stay' + (reduce ? ' static' : ' unpopped'), e && e.message); return; }
  if (gen !== g || startToken !== orbitStartToken) return;   // the reader left before this resolved
  const { forceSimulation, forceCollide, forceRadial } = mod;
  buttonNode = buttonNode || { id: 'button', x: orbitCx, y: orbitCy, vx: 0, vy: 0, fx: orbitCx, fy: orbitCy };

  orbitCollide = forceCollide((n) => n === buttonNode ? buttonR() : n.popAt ? ORBIT_D / 2 : 0).iterations(4);
  orbitSim = forceSimulation([buttonNode, ...orbitNodes])
    .velocityDecay(0)
    // A point attraction, not a ring: radius 0 is the standard forceRadial
    // idiom for a plain pull toward (cx, cy), so every target is drawn
    // straight at the control rather than held off at a fixed distance from
    // it — what stops it there is forceCollide below, not this force easing
    // off.
    .force('attract', forceRadial(0, orbitCx, orbitCy).strength(ORBIT_ATTRACT))
    // A fixed radius per node, not one keyed on n.popAt: forceCollide reads
    // its radius accessor once, at initialize, and caches it — a target not
    // yet popped is still pinned exactly to the control's own centre either
    // way, so collide never has reason to move it, and every already-packed
    // target still collides against where an unpopped one is standing. The
    // radius itself is each circle's own, with no added gap, so a settled
    // pack touches rather than keeps a courteous distance.
    // Default collide only relaxes overlap by one pass a tick, which a hub
    // this small and a dozen-plus targets converging on it can outrun,
    // leaving a soft overlap standing rather than a second shell forming
    // cleanly outside the first; a few more iterations a tick is what a
    // crowded packing needs to actually resolve (d3-force's own remedy for
    // this, not a workaround).
    .force('huddle', makeHuddle())
    // Resolve contact after attraction and huddling have applied their
    // velocities, so neither pull can re-introduce overlap in the same tick.
    .force('collide', orbitCollide)
    .force('speed-limit', makeSpeedLimit(ORBIT_SPEED_MAX))
    .alphaTarget(ORBIT_ALPHA_IDLE)
    .on('tick', render);
  if (reduce) orbitSim.alpha(0);
  else await popAll(g);
  })();
  orbitStartPromise = run;
  try { await run; } finally { if (orbitStartPromise === run) orbitStartPromise = null; }
}
// The reverse of a pop: each target tweens, by its own current position and
// pop-scale, straight back to the control over ORBIT_OUT_MS — a plain
// interpolation rather than more simulation, since this is the one part of
// the scene a fixed duration is asked of it (FAN_OUT_MS, above, is timed
// against it). The simulation itself is stopped first, which also cancels
// whatever the cluster's alpha was still cooling down from — leaving the
// scene ends the physics outright rather than waiting for it to settle.
function stopOrbit() {
  orbitStartToken++;
  orbitStartPromise = null;
  if (!orbitNodes) return;
  if (dragging) { dragging.fx = null; dragging.fy = null; dragging = null; }
  if (orbitSim) { orbitSim.stop(); orbitSim = null; }
  const g = gen, now0 = performance.now();
  for (const n of orbitNodes) { n.exiting = true; n.exitAt = now0; n.exitFrom = { x: n.x, y: n.y, s: popScale(n, now0) }; }
  const step = () => {
    const now = performance.now();
    render(now);
    const active = orbitNodes.some((n) => now - n.exitAt < ORBIT_OUT_MS);
    if (active && gen === g) { orbitExitFrame = requestAnimationFrame(step); return; }
    for (const n of orbitNodes) n.exiting = false;
  };
  orbitExitFrame = requestAnimationFrame(step);
}
// The targets are scene 4's and no other scene's; leaving either way sends
// them back into the control, and the simulation stops with them.
const setFan = (on) => { stage.classList.toggle('fanned', on); if (on) startOrbit(); else stopOrbit(); };
// ── Scene 5: the loop, and whether it is running ──────────────────────────
// The footage is asked for once, at the first crossing toward it, and never
// under reduced motion — a <video> with no source fetches nothing, so the
// poster is the whole scene there and no request for the loop is made. Whether
// the loop runs is then the reader's: the control below is a real button, and a
// hand that stops it keeps it stopped through every crossing after.
loopVid.poster = LOOP_POSTER;
const closingPoster = new Image(); closingPoster.src = LOOP_POSTER;
if (HEADLINE) $('five-headline').textContent = HEADLINE;
// loopOn starts unknown rather than false, so the first call after the source
// is mounted always says something to the element. It has to: the tag carries
// `autoplay`, and left alone the browser would start decoding the moment the
// source arrives — at the first crossing toward the scene, while the scrub has
// not begun and nothing has asked for it. An explicit pause there is what
// clears the autoplay flag and leaves the running state the script's to decide.
let xf = 0, loopMounted = false, loopWanted = true, loopOn = null;
function mountLoop() {
  if (loopMounted || reduce) return;
  loopMounted = true;
  loopVid.src = LOOP_SRC;
  syncLoop();
}
// One place decides: the loop runs when it is on the page and the reader has
// not stopped it. Called every frame of the scrub, so it acts only on a change.
function syncLoop() {
  const want = !document.hidden && !reduce && loopMounted && loopWanted && xf > 0;
  if (want === loopOn) return;
  loopOn = want;
  if (want) loopVid.play().catch(() => {}); else loopVid.pause();
}
// How far the last join has run, and the only thing it moves.
const setCross = (q) => { xf = q; document.documentElement.style.setProperty('--xf', q.toFixed(3)); syncLoop(); };
// The close takes the pointer and the tab order only where it is settled: during
// the scrub it is paint, and behind the page it is not there at all. The join
// has two sides and the page is the other one: where the close is settled the
// page is faded to nothing, and a control faded to nothing is still a tab stop
// until the element it sits in is inert.
const fiveOn = (on) => {
  const entering = on && !five.classList.contains('on');
  five.classList.toggle('on', on); five.inert = !on; page.inert = on;
  if (entering) requestAnimationFrame(() => {
    restSince = performance.now();
  });
};
// The canvas takes over the scene: the prints it shows are the pixels on
// screen, so the DOM under it can change without a visible frame.
// The sim keeps the boundary's two prints in SCENE order and `fwd` says which
// end the wash runs from, so the pair is keyed by scene and never by direction;
// handing it (from, to) would swap the source and the target on every wash back.
function holdCanvas(src, tgt, scene, fwd, mobileTransition = false) {
  groundAt(scene); sim.setPrints(src, tgt); sim.reset(); sim.draw(fwd, 0);
  // The cover is written every frame from p by the caller; seeding it to 0
  // here made every leg mount a visible uncover, which is what the reader
  // saw between scenes as "it flickers into the previous animation".
  if (mobileTransition) stage.classList.add('mobile-handoff');
  else stage.classList.add('morphing');
}
// The same arming for a scene transition: A's print is presented at p = 0
// before anything live is hidden, so the handover is the same pixels.
// `members` are memberPrint's for each end, a's then b's. carry: only the
// SHIPS<->DEPLOY pair -- the one leg publishBridge covers, whose readable
// content is the Publish control it carries solid across the leg rather
// than anything consolidated from pigment (watercolor-morph.js's own
// module comment). Declared here, the one place that mounts every leg,
// rather than left for a checker to reconstruct from scene indices.
function holdMorph(a, b, from, to, covered, members = [[], []]) {
  groundAt(from);
  const carry = Math.min(from, to) === SHIPS && Math.max(from, to) === DEPLOY;
  const leg = morph.leg(a, b, { from, to }, [...members[0].map((m) => ({ ...m, side: 'a' })), ...members[1].map((m) => ({ ...m, side: 'b' }))], carry);
  leg.render(0, 0);
  stage.classList.add(covered ? 'mobile-handoff' : 'morphing');
  return leg;
}
function releasePigmentCover() {
  morph?.end();
  stage.classList.remove('morphing', 'mobile-handoff');
  stage.style.removeProperty('--wash-cover');
  canvas.style.filter = '';
  cell.style.transform = '';
}
// When the scene last came to a stand, for the work that may only run at a
// rest: a dwell counts from here, not from the frame the target changed.
let settledAt = 0;
function still(scene) {
  settledAt = performance.now();
  // Install the no-transition guard before changing data-scene. Otherwise a
  // publish frame can paint once with the shell's lower-right source pose
  // before the centred end-state transform is applied.
  if (scene === DEPLOY) stage.classList.add('snap');
  sceneClasses(scene);
  groundAt(scene);
  fiveOn(scene === SHARE);
  scenes(PHASE[scene]);
  // Place scene furniture while the wash still covers the live DOM. This
  // keeps the clipped Publish control from flashing once in its source
  // position at the shell's lower-right before its centred transform lands.
  if (scene === DEPLOY) snapStage();
  // A final wash owns the pixels while the reader turns around. Keep its
  // cached source covered until the next frame can set the reverse direction;
  // otherwise the live Publish control flashes in its shell corner.
  if (!(finalWash?.covered && scene === DEPLOY)) releasePigmentCover();
  // a leg that left scene 4 froze its targets where they stood (memberPrint)
  if (scene === DEPLOY) orbitSim?.restart();
  if (mobileLayout() && scene === SHIPS) { sketchVisible(true); videoActive(true); }
  // The phone's mounted leg lost its canvas class above: remount it on the
  // next frame rather than keep presenting into a canvas nothing shows.
  if (mobileLayout()) { mobileWatchKey = ''; mob = { ...mob, from: -1, to: -1 }; }
  if (scene !== DEPLOY && finalWash) { cancelAnimationFrame(finalWash.raf); finalWash = null; canvas.style.filter = ''; fanEl.classList.remove(WatercolorMorph.MEMBER); }
  if (scene < DEPLOY) finalPrints = null;
}

// The signup link is navigation: it skips the scene performance.
window.mossLanding = {
  openSignup() {
    cancelSettle();
    if (running()) setTarget(SHARE);
    else { shown = target = SHARE; still(SHARE); }
    mountLoop(); setCross(1); fiveOn(true);
  },
  // Whether the page has actually arrived at the close, not just been asked
  // to go there: closing.js polls this to know when it may safely focus the
  // signup field, the same rest condition the wash itself already tracks.
  atClosing: () => shown === SHARE && !running(),
};

// ── The last join: the page gives way to the full-bleed close ─────────────
// The scrub is a position, not a clock: where the reader stands inside the last
// XF_SPAN of the distance scene 4 owns. So it reverses for free — a turnaround
// needs nothing said about it — and the join settles on whichever end the
// reader carried it to. Only opacity changes per frame.
// The incoming text owns contact. No dissolve begins in the gap before it.
// earlyBy shortens the far end of the span, so consolidation (q===1) lands
// earlyBy px sooner in scroll than the text fully clearing the band -- the
// scene1->2 leg passes MOBILE_LEG1_EARLY_BY (owner: consolidate "a little
// bit before" scene 2's text settles; 40px is about a line of this
// viewport's own body copy, enough margin to read as early without the
// target still looking unfinished when it happens). Every other leg passes
// 0, unchanged. Named and exposed on landing.mobileEarlyBy rather than left
// as an inline literal so a check can read what this leg actually does
// instead of re-typing the number (check-landing-text-track.mjs).
const MOBILE_LEG1_EARLY_BY = 40;
landing.mobileEarlyBy = (scene) => scene === 1 ? MOBILE_LEG1_EARLY_BY : 0;
function mobileInkProgress(text, earlyBy = 0) {
  const band = mobileVisualBand();
  const rect = text.getBoundingClientRect();
  return clamp01((band.bottom - rect.top) / (band.height + rect.height - earlyBy));
}
// Owner (2026-09-24): "morph from scene 1 to scene 2 should start a bit
// later, once text of scene 2 touches the animation" -- the #col-pin gate
// below read close to 1 while #c2's own text was still hundreds of px below
// the visual, so scene 1 was already dissolving with nowhere for the reader
// to see it land. Touch-gated instead, the same shape mobileInkProgress
// already gives every later leg, except this opening leg starts one line
// before contact so the slower dissolve is already legible as the copy arrives.
// That span is deliberately short and independent of mobileInkProgress's
// own (much longer) span for the *next* leg, 1->2, which is also gated by
// #c2 -- reusing that leg's own MOBILE_LEG1_EARLY_BY-adjusted ramp here
// would make this leg's own completion land at the exact scroll position
// leg 1->2's gate also reads 1, i.e. two legs finishing on the same frame,
// which collapses the plateau between them to nothing. A short span keeps
// leg 1->2's own gate (mobileInkProgress(#c2, 40), still counting from the
// same touch point) close to its own start when this leg hands off --
// the longer ramp keeps the handoff in the dissolve's own early phase so the
// scene 2 arrival reads as a continuation of the wet field, not a cut.
const MOBILE_LEG0_EARLY_BY = 72, MOBILE_LEG0_RAMP_SPAN = 160;
function mobileEntranceProgress() {
  const band = mobileVisualBand();
  const rect = scenesEl[LIVE].firstElementChild.getBoundingClientRect();
  return clamp01((band.bottom + MOBILE_LEG0_EARLY_BY - rect.top) / MOBILE_LEG0_RAMP_SPAN);
}
// Ends at closingRestY() (the reader's actual last pixel of scroll), not
// mobileInkProgress's own text-height span, which saturated at 1 hundreds
// of px early and left a reversal stuck reading "already at 1" until the
// whole gap was retraced (owner report 2026-09-22, "after scene 5 I cannot
// scroll back").
function mobileClosingProgress() {
  const rect = scenesEl[DEPLOY].firstElementChild.getBoundingClientRect();
  const startY = scrollY + rect.top - mobileVisualBand().bottom;
  return clamp01((scrollY - startY) / Math.max(1, closingRestY() - startY));
}
function mobileVisualBand() {
  const visual = document.getElementById('vis');
  const top = parseFloat(getComputedStyle(visual).top);
  const height = visual.offsetHeight;
  return { top, bottom: top + height, height };
}
// Mobile keeps its own path (unit 4, review-phases-2-4.md Job 2 item 5):
// mobileClosingProgress() through the same smooth() ease as every other
// mobile join, not unified with desktop's crossfade band here. Desktop
// derives from progressAt() -- the one number the crossfade, the CSS
// --xf custom property and a scene rest all now read alike (unit 3).
function xfAt() {
  if (mobileLayout()) return smooth(.45, 1, mobileClosingProgress());
  return clamp01(progressAt() - DEPLOY);
}
function nativeScroll() {
  // The browser owns scrollY in every viewport. The presenter only observes
  // it and updates the composition; no wheel event or spring writes a page
  // position.
  return true;
}
// The scene on the far side of this join arrives already standing rather than
// drawing itself under the fade: the fan is scene 4's end state, not a
// performance to replay each time the reader crosses back.
let snapToken = 0;
function snapStage() {
  const token = ++snapToken;
  stage.classList.add('snap');
  requestAnimationFrame(() => requestAnimationFrame(() => { if (token === snapToken) stage.classList.remove('snap'); }));
}
async function fade(to) {
  // Restore the centred composition under pigment, never replay its entrance.
  if (shown === SHARE) {
    snapStage(); sceneClasses(DEPLOY); groundAt(DEPLOY);
    stage.classList.add('fanned');
  }
  mountLoop();
  if (reduce) { setCross(to === SHARE ? 1 : 0); return to; }
  const settled = await new Promise(done => {
    const frame = () => {
      const q = xfAt(); setCross(q); updateFinalDissolve();
      if (q >= 1 && target !== DEPLOY) done(SHARE);
      else if (q <= 0 && target !== SHARE) done(DEPLOY);
      else requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  });
  setCross(settled === SHARE ? 1 : 0); snapStage();
  return settled;
}

const stepToward = (from, t) => t === from ? from : from + Math.sign(t - from);
// How far one mechanism carries the sheet toward `want`. A wash swallows every
// wash boundary in its path — three boundaries is still one wash, from the
// pixels on screen to the print of the scene asked for — while a join that
// brings its own mechanism is always a step of its own, so the crossfade is
// never folded into a wash and no wash is ever asked to reach the scene that
// has no print. `onHand` bounds the answer by the prints there are: a wash
// re-points on the frame the target moves and cannot wait for a capture, so it
// points at the farthest print it has and the next leg carries the rest.
const legToward = (from, want, onHand) => {
  let to = stepToward(from, want);
  if (to === from || !joinAt(from, to).wash) return to;
  while (to !== want && joinAt(to, stepToward(to, want)).wash) to = stepToward(to, want);
  if (onHand) while (to !== from && !sheets[to]) to = stepToward(to, from);
  return to;
};
// The scenes a jump passes over are states, not performances: each is applied
// to the page in turn and none of them gets a wash of its own. It all happens
// inside one frame, under the canvas, so no intermediate scene is ever on
// screen — and none of their interiors is started, which is what keeps a
// notebook that was only passed over from booting a kernel. The fan and the
// interiors belong to the scene the reader arrives at, which `still` sets: the
// one scene that carries a fan sits next to the one join that never washes, so
// it is always a leg's end and never a scene passed over.
const passThrough = (from, to) => { for (let s = stepToward(from, to); s !== to; s = stepToward(s, to)) { sceneClasses(s); groundAt(s); } };
// A scene change: one mechanism at a time, and never two at once. The target is
// read whole off the scroll position, so a jump is not a queue of joins to walk
// — a wash already running is re-pointed at the newest target inside `pour`,
// and this loop only starts another mechanism once the last one has landed
// somewhere the target has since moved away from.
let joinsRun = 0, washesRun = 0;
async function runJoin() {
  if (driving) return;
  driving = true;
  try {
    while (target !== shown) {
      const to = legToward(shown, target);
      joinsRun++;
      // The mechanism is the first boundary's: it is the one whose scene is
      // being left, and the only one with anything standing to take back.
      const join = joinAt(shown, stepToward(shown, to));
      // A join that brings its own mechanism runs it and nothing else: the
      // wash's preamble takes down what the scene being left had standing, and
      // a crossfade needs both scenes left exactly as they are.
      if (join.back && to < shown && !mobileLayout()) shown = await join.back(to);
      else if (join.run) shown = await join.run(to);
      else {
        // A jump can ask for a scene no print was ever taken for: the prints on
        // hand are the neighbours', and two boundaries away is not a neighbour.
        // Stopping to take one costs 14 to 54ms of clone-and-serialize on the
        // frame the reader is still moving in (measured 2026-09-14), so the
        // wash reaches as far as the prints it holds and the rest of the leg is
        // a cut. The warmer has the others by the next rest.
        const reach = washing() && join.wash && sheets[shown] ? legToward(shown, target, true) : shown;
        // With no simulation (reduced motion, or no float render targets) a
        // wash is a cut, and so is a boundary whose print never arrived; a wash
        // that runs may be re-pointed or turned around inside.
        if (reach !== shown) { shown = await pour(reach); wentStale(); }
        else { passThrough(shown, to); shown = to; }
      }
      if (target === SHARE && xfAt() >= 1) { standAtTerminalClose(); break; }
      still(shown);
    }
  } finally { driving = false; flushPrintRect(); }
}
// One mobile pigment clock. The text track supplies a position; this advances
// or rewinds the same liquid surface, and does no GPU draw at an unchanged rest.
function advanceWash(simInstance, state, { goal, fwd, stir = 0, tilt = 0, relift = 0, budget = STEPS_PER_FRAME }, paint) {
  if (state.t > goal + DT) {
    // A keyframe at or before goal replays forward from there instead of a
    // full reset()-and-252-step replay from zero; reset() only when no
    // keyframe qualifies (goal is before the first one, or none has been
    // captured yet for this print pair) -- reset() zeroes the water field
    // too, which visibly re-floods the sheet, so it is never used where a
    // restore will do.
    const kfT = simInstance.restoreNearestKeyframe && simInstance.restoreNearestKeyframe(goal);
    if (kfT != null) state.t = kfT;
    else { simInstance.reset(); state.t = 0; }
    state.drawn = -1;
  }
  let count = 0;
  while (state.t < goal && state.t < T_TOTAL && budget-- > 0) {
    simInstance.step(fwd, state.t, smooth(T_CURE, T_TOTAL, state.t), stir, tilt, relift);
    state.t += DT; count++;
  }
  if (simInstance.maybeCaptureKeyframe) simInstance.maybeCaptureKeyframe(state.t);
  if (state.t >= goal - DT && state.t !== state.drawn) { paint(state.t); state.drawn = state.t; }
  return { caughtUp: state.t >= goal - DT, count };
}
// ── Membership: what a leg's two prints carry of what moves ───────────────
// A cached sheet is right for everything that stands still -- the stage, the
// window, the documents in it -- and is taken at rest. What animates (scene
// 3's sketch, the notebook's zooming field, the video; scene 4's targets when
// it is left) is only right on the frame it was read, so at a leg's start it
// is frozen where it stands and read through the painter registry in the same
// task, and the print is composed with it. Each such element is a member of
// the leg: WatercolorMorph hides it under its own print (or, where it could
// not be read, fades it) until its scene is shown again.
function memberPrint(scene, leaving) {
  const sheet = sheets[scene];
  if (!sheet) return { print: sheet, members: [] };
  if (scene === SHIPS && sheet.recompose) {
    // a reader's own arrangement is frozen, as it stands, with the media
    for (const id of S3_ORDER) stopCard(id);
    sketchVisible(false); videoActive(false);
    const fresh = {}, members = [];
    for (const id of S3_ORDER) {
      // Not read now, the card's cached pixels still stand in; with neither,
      // it is not in the print at all and fades instead of hiding.
      const im = artifactNow(id, sheet.artifacts[id]);
      if (im) fresh[id] = im; else reportCaptureFault(`member ${id}: not read at the leg's start`);
      members.push({ el: CARDS[id].el, captured: !!(im || sheet.artifacts[id]) });
    }
    // The freshest print of scene 3 there is: it replaces the one on hand, so
    // the dissolve recorded for this leg is the one kept warm after it.
    const print = sheet.recompose(fresh);
    setSheet(SHIPS, print);
    return { print, members };
  }
  if (scene === DEPLOY) {
    if (leaving) orbitSim?.stop();
    // Left before any target has popped, scene 4 is its control alone, and a
    // print of nothing would show paper where the control's wash should be.
    const print = deployPrint({ logos: leaving, control: !leaving || !orbitNodes?.some((n) => n.popAt) });
    print.__printGeneration = sheet.__printGeneration;
    return { print, members: leaving ? [{ el: fanEl, captured: true }] : [] };
  }
  return { print: sheet, members: [] };
}
// Scene 3's card `id` as it stands on screen now, on the cached card's own
// canvas size: the sketch and the video read directly, the notebook as its
// cached document with only its live field read afresh and laid over it.
function artifactNow(id, cached) {
  const read = (el, w, h) => { const r = WatercolorCapture.frameNow(el, w, h); return r.ok ? r.canvas : null; };
  try {
    if (id === 'video') { const v = $('s3-video-el'); return read(v, v.videoWidth, v.videoHeight); }
    const doc = $(id).contentDocument;
    if (id === 'sk') { const cv = doc?.querySelector('canvas'); return cv && read(cv, cv.width, cv.height); }
    const field = doc?.getElementById('field');
    if (!field || !cached) return null;
    const live = read(field, field.width, field.height);
    if (!live) return null;
    const out = document.createElement('canvas'); out.width = cached.width; out.height = cached.height;
    const g = out.getContext('2d'), r = field.getBoundingClientRect(), k = cached.width / CARDS.nb.width;
    g.drawImage(cached, 0, 0); g.drawImage(live, r.left * k, r.top * k, r.width * k, r.height * k);
    return out;
  } catch (e) { reportCaptureFault(`member ${id}: ${e.message}`); return null; }
}
function publishBridge(from, to, prints) {
  if (Math.min(from, to) !== SHIPS || Math.max(from, to) !== DEPLOY) return null;
  const button = vdFrame.contentDocument.querySelector('.moss-publish-button');
  if (!button) return null;
  const r = button.getBoundingClientRect(), size = r.width;
  const clone = button.cloneNode(true);
  const originals = [button, ...button.querySelectorAll('*')], copies = [clone, ...clone.querySelectorAll('*')];
  originals.forEach((el, i) => {
    const style = el.ownerDocument.defaultView.getComputedStyle(el);
    for (const property of style) copies[i].style.setProperty(property, style.getPropertyValue(property));
  });
  clone.id = 'publish-bridge'; clone.setAttribute('aria-hidden', 'true'); clone.tabIndex = -1;
  Object.assign(clone.style, { position: 'absolute', right: 'auto', bottom: 'auto', margin: '0', zIndex: '110', pointerEvents: 'none', visibility: 'visible', transformOrigin: 'center' });
  stage.appendChild(clone);
  const priorVisibility = button.style.visibility;
  button.style.visibility = 'hidden';
  const cs = button.ownerDocument.defaultView.getComputedStyle(button);
  const x = 120 + 540 - (parseFloat(cs.right) || 6) - size / 2;
  const y = 30 + 560 - (parseFloat(cs.bottom) || 6) - size / 2;
  // The live control survives independently, so its ink comes out of scene
  // 3's sheet -- but only its ink: a square clear took the window's rounded
  // corner and shadow with it (the box the owner saw around the button) and
  // left the disc's own edge behind. Scene 4's print arriving keeps its
  // control, under the bridge's landing place (memberPrint), so the wash has
  // somewhere to gather and is never paper alone.
  const source = prints[SHIPS], copy = document.createElement('canvas');
  copy.width = source.width; copy.height = source.height;
  const context = copy.getContext('2d'), k = copy.width / printW();
  const ink = document.createElement('canvas');
  ink.width = ink.height = Math.ceil(size * k);
  const inkContext = ink.getContext('2d');
  inkContext.beginPath(); inkContext.arc(ink.width / 2, ink.height / 2, ink.width / 2, 0, Math.PI * 2); inkContext.clip();
  inkContext.drawImage(source, (x - size / 2 - printRect.x) * k, (y - size / 2 - printRect.y) * k, size * k, size * k, 0, 0, ink.width, ink.height);
  context.drawImage(source, 0, 0);
  context.globalCompositeOperation = 'destination-out';
  context.beginPath(); context.arc((x - printRect.x) * k, (y - printRect.y) * k, (size / 2 + 2) * k, 0, Math.PI * 2); context.fill();
  prints[SHIPS] = copy;
  return {
    element: clone, ink,
    draw(t) {
      // Forward finishes at T_WET, where the reading line reaches scene 4's
      // text, instead of drifting into the cure tail after text has stopped.
      const forward = from === SHIPS;
      const bound = forward ? T_WET : T_TOTAL;
      const q = smooth(0, bound, t), p = forward ? q : 1 - q;
      // Mobile only (owner: "scale up slow at first, then faster, so it
      // spends more time being small"): the control's own scale reads a
      // cubic ease-in of the same t/bound ratio p is built from, instead of
      // p's own symmetric smoothstep -- position (cx/cy) and the fan's
      // expansion below keep p unchanged, so only the size grows unevenly.
      // Unconditionally the forward shape (never 1 - easeQ): mountLeg
      // (renderMorphAt's own leg mounter) always mounts this leg from=SHIPS,
      // to=DEPLOY (from < to, always), so on mobile `forward` above is
      // always true and t always counts up from SHIPS's own rest -- a
      // reader scrolling back from DEPLOY toward SHIPS is t (and easeQ)
      // counting back down through the same curve, not a second, mirrored
      // one, which is what "reverse mirrors it" can only mean for a
      // transform that is a pure function of scroll position: the control
      // spends the same share of the leg's own p small however it is read.
      // (desktop's pour() does mount both orders, which is exactly why this
      // stays mobileLayout()-gated: p itself, unchanged, still drives
      // desktop's own scale.) Cubed, not squared: at the leg's own half
      // progress (p=0.5, t=T_TOTAL/2) this reads (T_TOTAL/2/T_WET)**3, 28%
      // of the control's total size change -- squared read 43%, short of
      // the owner's own margin under 35%.
      const easeQ = Math.pow(clamp01(t / bound), 3);
      const scaleP = mobileLayout() ? easeQ : p;
      const cx = (x + (GEOM.cellW / 2 - x) * p) * SCALE;
      const cy = (y + (GEOM.cellH / 2 - y) * p) * SCALE + (mobileLayout() ? 0 : (1 - SCALE) * GEOM.cellH / 2);
      const scale = SCALE * (1 + (PUB_SCALE * (mobileLayout() ? 1.4 : 1) - 1) * scaleP);
      clone.style.left = (cx - size / 2) + 'px'; clone.style.top = (cy - size / 2) + 'px';
      clone.style.transform = `scale(${scale})`;
      if (mobileLayout()) {
        const expansion = (from === DEPLOY ? 1.4 : 1) + (to === DEPLOY ? .4 : -.4) * smooth(.35, T_TOTAL, t);
        fanEl.style.transformOrigin = `${orbitCx}px ${orbitCy}px`;
        fanEl.style.transform = `translate(${(x - orbitCx) * (1 - p) / expansion}px, ${(y - orbitCy) * (1 - p) / expansion}px) scale(${scale / (SCALE * expansion * PUB_SCALE)})`;
      }
    },
    remove() { button.style.visibility = priorVisibility; clone.remove(); fanEl.style.transform = ''; fanEl.style.transformOrigin = ''; }
  };
}
// The mobile presenter. Everything pour()'s promise and runJoin's leg
// walking did for one wash at a time, this does once a frame for whichever
// leg the reader's own scroll position names -- there is no queue of joins
// and no promise to abort, so the flicker that came from tearing one down
// mid-flight (unit7-scroll-and-flicker-spec.md part A) cannot happen: a
// mount only ever fires when the (from, to) pair itself changes, and
// because p starts each leg at 0 and the previous leg's own p ended at 1,
// that is always a moment cover is already ~0 -- the one place a freshly
// reset, blank simulation frame is invisible.
// One record for everything the mobile presenter needs to persist across
// frames -- pour()'s own equivalents (liveScene, the pigment clock) were
// local to one promise; this outlives any single call, so it has to live
// somewhere, and a single typed record beats a handful of scattered
// top-level lets for the same reason it would anywhere else in this file.
let mob = {
  from: -1, to: -1, pr: null,          // the mounted leg, or pr: null while its prints are still missing
  leg: null,                            // the WatercolorMorph leg presenting it
  scene: -1,                            // showUnderCover's mobile twin: which side of the leg is live
  bridge: null,                         // the Publish bridge clone; non-null only on the SHIPS<->DEPLOY leg
  settled: true,                        // the frame shown is exactly this p's, or nothing to show (cut)
  p: -1, lastCover: -1, drawnT: -1, fanSolid: false,   // last-written values, so an unchanged frame writes nothing
};
// Mounts the leg's canvas if its prints are both on hand, or records the
// cut (mob.pr stays null) if not. Callable every frame regardless of which
// happened last time: a leg that cut for missing prints upgrades to a real
// mount the moment they arrive, rather than staying a cut for the rest of
// the leg -- a fast cold-load scroll can easily outrun capture for one
// frame and not the next.
// `leaving` is the end the reader is leaving, whose members are read as they
// stand; null mounts a leg at rest, from the prints on hand, and it is mounted
// again with its members the moment the reader leaves that end.
function mountLeg(from, to, leaving = null) {
  if (!sheets[from] || !sheets[to]) {
    // A cut mount still leaves the previous leg's bridge behind if it had
    // one (found while consolidating mob: the pre-consolidation version had
    // the same gap) -- only the SHIPS<->DEPLOY pair ever has one, so this
    // only ever fires on the rare cut immediately after leaving it.
    mob.bridge?.remove();
    releasePigmentCover();
    resetWordContrast();
    mob = { ...mob, from, to, pr: null, leg: null, bridge: null, p: -1, lastCover: 0 };
    return;
  }
  const A = leaving == null ? { print: sheets[from], members: [] } : memberPrint(from, leaving === from);
  const B = leaving == null ? { print: sheets[to], members: [] } : memberPrint(to, leaving === to);
  const pr = { [from]: A.print, [to]: B.print };
  mob.bridge?.remove();
  const bridge = publishBridge(from, to, pr);
  bridge?.draw(0);
  const leg = holdMorph(pr[from], pr[to], from, to, true, [A.members, B.members]);
  if (mob.fanSolid) { fanEl.style.filter = ''; fanEl.style.zIndex = ''; }
  mob = { ...mob, from, to, pr, leg, bridge, p: -1, lastCover: -1, drawnT: -1, fanSolid: false };
}
// showUnderCover's mobile twin, module-level because the mount it tracks
// now outlives any one frame. Also the one place `shown` is written for
// mobile: the rest of the page (scene 3's card gating, scene 4's orbit,
// the Publish/video visibility) reads shown as "what the reader is
// currently looking at", which is exactly what this switch decides.
function showMobileScene(scene) {
  if (scene === mob.scene) return;
  snapStage();
  sceneClasses(scene);
  groundAt(scene);
  if (!(mob.bridge && scene === DEPLOY)) scenes(PHASE[scene]);
  if (scene === SHIPS) { sketchVisible(false); videoActive(false); }
  mob.scene = scene;
  shown = scene;
  wentStale();
}
// ── Item D: per-word contrast against the wash behind it ──────────────────
// Owner: "when the dark text intersects with the dark wash in the
// background, do we have a reliable way to flip the overlapping text to
// white, so it's more readable?" Each word of a scene's own copy (its h2
// and .lede, never a button and never #intro's own header -- the scenes'
// own text is the only copy a wash ever paints under) is wrapped in its own
// span at first use, so its background can be sampled and its colour
// flipped independently of its neighbours: a whole line flipping together
// would read as a stripe crossing the wash, not the actual word doing it.
// Nothing here runs unless the canvas is covering (updateWordContrast is
// only ever called from renderMorphAt's own cover===1 branch) and
// resetWordContrast, called once at the cover 1->0 edge, is what returns
// every word this leg touched to its ordinary colour -- there is no
// per-frame cost once the canvas stops covering, only that one clear.
const srgbToLin = (c) => { c /= 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
const relLuminance = (r, g, b) => 0.2126 * srgbToLin(r) + 0.7152 * srgbToLin(g) + 0.0722 * srgbToLin(b);
const contrastOf = (l1, l2) => { const a = Math.max(l1, l2), b = Math.min(l1, l2); return (a + 0.05) / (b + 0.05); };
const parseRGB = (str) => { const m = str.match(/[\d.]+/g); return m ? [+m[0], +m[1], +m[2]] : [0, 0, 0]; };
const wordContrast = { words: null, sample: null, surface: document.createElement('canvas') };
function initWords() {
  wordContrast.words = [];
  const wrap = (el) => {
    for (const child of [...el.childNodes]) {
      if (child.nodeType === Node.ELEMENT_NODE) { wrap(child); continue; }
      if (child.nodeType !== Node.TEXT_NODE || !child.textContent.trim()) continue;
      const frag = document.createDocumentFragment();
      for (const part of child.textContent.split(/(\s+)/)) {
        if (!part) continue;
        if (/^\s+$/.test(part)) { frag.appendChild(document.createTextNode(part)); continue; }
        const span = document.createElement('span');
        span.className = 'word'; span.textContent = part;
        frag.appendChild(span);
      }
      child.replaceWith(frag);
    }
  };
  for (const el of document.querySelectorAll('.scene .scene-text h2, .scene .scene-text p.lede')) {
    wrap(el);
    const heading = el.tagName === 'H2';
    // A two-colour (dark/white) flip is gap-free against every possible
    // background luminance only where the dark colour's own luminance is
    // at most 1.05/required^2 - 0.05 -- headings' --text (~0.022) clears
    // that bound at their 3:1 floor with room to spare, but .lede's own
    // --text2 (~0.10) does not clear it at 4.5:1: measured live, a
    // background luminance band (~0.18-0.63) exists where --text2 already
    // fails 4.5:1 and white would fail worse (a real reading, "Everything
    // lives in local files," ratio 3.42-3.63, stayed dark because white's
    // own contrast there was lower still, 1.9-2.1). Body words get a
    // forced near-black rest colour while the wash is actively read under
    // them (never at rest, never for headings) so the same two-state flip
    // has a dark end that actually clears 4.5:1 everywhere; ld=0 matches
    // that forced colour for the flip decision itself.
    const restColor = heading ? '' : '#000';
    const ld = heading ? relLuminance(...parseRGB(getComputedStyle(el).color)) : 0;
    for (const span of el.querySelectorAll('.word')) wordContrast.words.push({ el: span, ld, restColor });
  }
}
// Integrate the exact word rectangle over the displayed layers, composited
// over the page. Mostly transparent words keep their ordinary page colour.
function bgLuminanceUnder(rect, canvasRect, small) {
  canvasRect = small.rect || canvasRect;
  const nx0 = (rect.left - canvasRect.left) / canvasRect.width, nx1 = (rect.right - canvasRect.left) / canvasRect.width;
  const ny0 = (rect.top - canvasRect.top) / canvasRect.height, ny1 = (rect.bottom - canvasRect.top) / canvasRect.height;
  const left = nx0 * small.w, right = nx1 * small.w;
  const top = ny0 * small.h, bottom = ny1 * small.h;
  const x0 = Math.max(0, Math.floor(left)), x1 = Math.min(small.w, Math.ceil(right));
  const y0 = Math.max(0, Math.floor(top)), y1 = Math.min(small.h, Math.ceil(bottom));
  if (x1 <= x0 || y1 <= y0) return null;
  const px = small.pixels, bg = small.bg;
  let inked = 0, sum = 0, total = 0;
  for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) {
    // Edge cells contribute only the area actually beneath the word.
    const area = (Math.min(x + 1, right) - Math.max(x, left)) * (Math.min(y + 1, bottom) - Math.max(y, top));
    total += area;
    const i = (y * small.w + x) * 4, a = px[i + 3] / 255 * small.opacity;
    if (a >= 0.05) inked += area;
    sum += area * relLuminance(px[i] * a + bg[0] * (1 - a), px[i + 1] * a + bg[1] * (1 - a), px[i + 2] * a + bg[2] * (1 - a));
  }
  return inked / total > 0.3 ? sum / total : null;
}
// Choose the more readable colour, leaving room for raster rounding rather
// than holding the previous colour until it reaches the exact contrast floor.
function updateWordContrast() {
  if (!wordContrast.words) initWords();
  const canvasRect = canvas.getBoundingClientRect();
  if (canvasRect.width < 1 || canvasRect.height < 1) return;
  const visible = wordContrast.words.map(word => ({ word, rect: word.el.getBoundingClientRect() })).filter(({ rect: r }) => r.right > Math.max(0, canvasRect.left) && r.left < Math.min(innerWidth, canvasRect.right) && r.bottom > Math.max(0, canvasRect.top) && r.top < Math.min(innerHeight, canvasRect.bottom));
  for (const word of wordContrast.words) if (!visible.some(v => v.word === word)) word.el.style.color = '';
  if (!visible.length) { wordContrast.sample = null; return; }
  const left = Math.max(0, Math.floor(Math.min(...visible.map(v => v.rect.left))));
  const top = Math.max(0, Math.floor(Math.min(...visible.map(v => v.rect.top))));
  const right = Math.min(innerWidth, Math.ceil(Math.max(...visible.map(v => v.rect.right))));
  const bottom = Math.min(innerHeight, Math.ceil(Math.max(...visible.map(v => v.rect.bottom))));
  const w = right - left, h = bottom - top;
  // One CSS-pixel crop covers all reading words. The Publish ink is retained
  // from the existing scene capture, never recaptured during scrolling.
  const surface = wordContrast.surface, g = surface.getContext('2d', { willReadFrequently: true });
  if (surface.width !== w || surface.height !== h) { surface.width = w; surface.height = h; }
  g.clearRect(0, 0, w, h);
  const style = getComputedStyle(canvas);
  g.globalAlpha = Number(style.opacity);
  g.filter = style.filter;
  const x0 = Math.max(left, canvasRect.left), y0 = Math.max(top, canvasRect.top);
  const x1 = Math.min(right, canvasRect.right), y1 = Math.min(bottom, canvasRect.bottom);
  g.drawImage(canvas, (x0 - canvasRect.left) * canvas.width / canvasRect.width, (y0 - canvasRect.top) * canvas.height / canvasRect.height, (x1 - x0) * canvas.width / canvasRect.width, (y1 - y0) * canvas.height / canvasRect.height, x0 - left, y0 - top, x1 - x0, y1 - y0);
  g.filter = 'none';
  if (mob.bridge) {
    const r = mob.bridge.element.getBoundingClientRect();
    g.globalAlpha = Number(getComputedStyle(mob.bridge.element).opacity);
    g.drawImage(mob.bridge.ink, r.left - left, r.top - top, r.width, r.height);
  }
  let opacity = 1;
  for (let el = canvas.parentElement; el; el = el.parentElement) opacity *= Number(getComputedStyle(el).opacity);
  const small = wordContrast.sample = { w, h, rect: { left, top, width: w, height: h }, pixels: g.getImageData(0, 0, w, h).data, bg: parseRGB(getComputedStyle(document.body).backgroundColor), opacity };
  for (const { word: w, rect: r } of visible) {
    const lbg = bgLuminanceUnder(r, canvasRect, small);
    // A word leaving the pigment must return to its ordinary page colour.
    if (lbg == null) { w.el.style.color = ''; continue; }
    w.el.style.color = contrastOf(1, lbg) > contrastOf(w.ld, lbg) ? '#fff' : w.restColor;
  }
}
function resetWordContrast() {
  wordContrast.sample = null;
  if (!wordContrast.words) return;
  for (const w of wordContrast.words) w.el.style.color = '';
}

function renderMorphAt(progress) {
  const from = Math.max(0, Math.min(SHIPS, Math.floor(progress))), to = from + 1;
  const p = clamp01(progress - from);
  const between = p > 0 && p < 1, leaving = between ? (p < 0.5 ? from : to) : null;
  if (from !== mob.from || to !== mob.to) {
    passThrough(mob.scene === -1 ? from : mob.scene, from);
    mountLeg(from, to, leaving);
  } else if (!mob.pr) {
    mountLeg(from, to, leaving);   // retry: a print an earlier cut was missing may have arrived since
  } else if (between && (mob.p === 0 || mob.p === 1)) {
    mountLeg(from, to, mob.p === 1 ? to : from);   // the reader leaves an end: read its members now
  }
  if (!mob.pr) {
    // Still cut (a fast cold-load scroll outrunning capture): a wash with
    // nothing to reach is a jump straight to the far end, the same rule
    // runJoin's own reach !== shown cut takes. The same latch as the
    // mounted path below, not a bare p <= 0.45 -- retrying every frame
    // while cut means this runs every frame too, and without the mob.scene
    // deadband a p oscillating across 0.45 while the reader merely holds
    // still flips back and forth on nothing (measured: dataset.scene
    // visiting 2,1,2,3,2 while still waiting on SHIPS's print).
    showMobileScene(mob.scene === from ? (p >= 0.55 ? to : from) : (p <= 0.45 ? from : to));
    mob.settled = false;   // keep retrying: a print may still arrive with no further scroll
    driving = false;       // a cut must let the warmer supply its missing print
    return;
  }
  if (p !== mob.p || !mob.settled) {
    const r = mob.leg.render(p, STEPS_PER_FRAME, mobileLayout());
    steps += r.spent; mob.settled = r.exact; mob.p = p;
  }
  // A recorded checkpoint is still a displayed transition. Keep ownership
  // while it covers the live scene so the idle warmer cannot replace its
  // canvas or temporarily switch the stage to a neighbouring scene.
  driving = between || !mob.settled;
  // The canvas takes the whole composition for the whole leg: never hidden
  // while the reader is between the two scenes.
  const cover = p > 0 && p < 1 ? 1 : 0;
  if (cover !== mob.lastCover) {
    stage.style.setProperty('--wash-cover', String(cover));
    mob.lastCover = cover;
    if (!cover) resetWordContrast();   // the live DOM is fully back: no wash left to read a word's background off
  }
  showMobileScene(mob.scene === from ? (p >= 0.55 ? to : from) : (p <= 0.45 ? from : to));
  if (!mob.fanSolid && mob.bridge && to === DEPLOY && p > .35 &&
      textBottom(scenesEl[SHIPS]) <= stage.getBoundingClientRect().top + GEOM.cellH * SCALE / 2) {
    scenes(PHASE[DEPLOY]);
    // The logo group stays solid while the remaining article ink settles.
    fanEl.style.filter = 'none';
    fanEl.style.zIndex = '101';
    mob.fanSolid = true;
  }
  // The Publish carry and the cell's growth keep their own curves, read off
  // the leg's position on the span they were tuned in; an unchanged p has
  // nothing new to draw.
  const t = p * T_TOTAL;
  washT = t;
  if (t !== mob.drawnT) {
    mob.bridge?.draw(t);
    // Ease-in, not smoothstep's symmetric ease: the control spends longer
    // small, then grows fast at the end (owner: "first go slow then fast").
    const sizeProgress = clamp01((t - .35) / (T_TOTAL - .35)) ** 2;
    const size = (from === DEPLOY ? 1.4 : 1) + ((to === DEPLOY ? 1.4 : 1) - (from === DEPLOY ? 1.4 : 1)) * sizeProgress;
    cell.style.transform = `scale(${SCALE}) translate(${(1 - size) * GEOM.cellW / 2}px, ${(1 - size) * GEOM.cellH / 2}px) scale(${size})`;
    mob.drawnT = t;
  }
  // Sample every displayed frame, including incomplete recordings, after
  // the scene and cell transforms have reached their displayed geometry.
  if (cover) updateWordContrast();
  // The bridge's own lifetime is the leg's: mountLeg above already removes
  // the previous one (if any) before creating this leg's, or leaves it
  // null for a leg that isn't the SHIPS<->DEPLOY pair. Nothing here needs
  // to tear it down mid-leg -- an earlier version did, on p <= 0, which is
  // also true on the very first frame after a fresh mount and removed the
  // bridge mountLeg had just built.
}
async function pour(to) {
  const from = shown;
  let terminal = false;
  washesRun++;
  const A = memberPrint(from, true), B = memberPrint(to, false);
  const pr = { [from]: A.print, [to]: B.print };
  const bridge = publishBridge(from, to, pr);
  bridge?.draw(0);
  // The Publish crossing keeps its control solid above the pigment, so the
  // canvas covers the live composition by --wash-cover rather than by hiding
  // it (check-landing-publish-bridge.mjs).
  const covered = !!bridge;
  const leg = holdMorph(pr[from], pr[to], from, to, covered, [A.members, B.members]);
  let liveScene = from, lastCover = -1, shownP = -1, exact = false;
  const showUnderCover = (scene) => {
    if (scene === liveScene) return;
    snapStage();
    sceneClasses(scene);
    groundAt(scene);
    if (!(bridge && scene === DEPLOY)) scenes(PHASE[scene]);
    if (scene === SHIPS) { sketchVisible(false); videoActive(false); }
    liveScene = scene;
  };
  // A turnaround that lands back on a scene this pour has already passed
  // through must not rewrite dataset.scene to a value it had already left.
  const passOnce = (dest) => {
    if (dest === liveScene) return;
    scenes('morph'); passThrough(liveScene, dest); sceneClasses(dest);
    liveScene = dest;
  };
  // The scenes' classes all land in this frame, under the canvas: the ones a
  // jump passes over, and the target's.
  if (!covered) passOnce(to);
  steps = 0; washT = 0;
  // Where the reader stands between the two texts is the whole of what is
  // shown: p is read off the scroll position every frame, so the carry's own
  // travel to a rest is what plays a transition out and a turnaround plays it
  // back, with no clock of its own. The pour ends where p does: at 1 on the
  // scene asked for, or at 0 once the reader has turned back to where it began.
  const settled = await new Promise((finish) => {
    const f = () => {
      if (target === SHARE && xfAt() >= 1) { terminal = true; finish(to); return; }
      const p = clamp01((progressAt() - from) / (to - from));
      if (p !== shownP || !exact) {
        const r = leg.render(p, STEPS_PER_FRAME);
        steps += r.spent; exact = r.exact; shownP = p; washT = p * T_TOTAL;
        if (covered) {
          const cover = p > 0 && p < 1 ? 1 : 0;
          if (cover !== lastCover) { stage.style.setProperty('--wash-cover', String(cover)); lastCover = cover; }
          // Switched only while the canvas covers it, and latched: p is
          // monotone in scroll within a leg, so this changes at most once per
          // direction, and the deadband means even a jittering p cannot chatter.
          showUnderCover(liveScene === from ? (p >= 0.55 ? to : from) : (p <= 0.45 ? from : to));
          bridge.draw(washT);
        } else {
          // The sheet's shadow goes as the scene dissolves and the next one's
          // returns as it consolidates.
          const out = 1 - smooth(0, WatercolorMorph.A_END, p), back = smooth(WatercolorMorph.B_START, 1, p);
          ground(GROUND[from][0] * out + GROUND[to][0] * back, GROUND[from][1] * out + GROUND[to][1] * back);
        }
      }
      if (exact && p >= 1) finish(to);
      else if (exact && p <= 0 && (target - from) * (to - from) <= 0) finish(from);
      else requestAnimationFrame(f);
    };
    requestAnimationFrame(f);
  });
  if (covered) releasePigmentCover();
  bridge?.remove();
  fanEl.style.filter = '';
  fanEl.style.zIndex = '';
  if (terminal) return SHARE;
  if (!bridge && settled === to) setSheet(to, pr[to]);   // the scene now on screen, at rest
  return settled;
}
// The wash needs the scene as pixels, and a browser will not hand over its own
// rendering, so the scene is re-rendered by the same engine: each document
// (the stage, the editor, the preview's chrome, the page inside it) is cloned,
// its images and stylesheets inlined, drawn as an SVG foreignObject image, and
// the images are composited at the rects the live frames occupy. One document
// per image, because a single SVG has one stylesheet namespace and three
// documents' `body`, `h1` and `:root` rules would fight in it. Same engine,
// same fonts, same pixels as the live scene, on whatever machine is looking.
// (A blob: URL taints the canvas in Chromium; a data: URL does not.)
// A print must never wait on the network. Its resources are already on the
// page, so a fetch here is a re-read, and a re-read that hangs (a pooled
// socket gone stale behind an ssh forward, seen on the Mac: the first print
// never finished, so the poem never typed and no wash could arm) must give up
// and print without. Successes are cached for good; a failure is remembered
// for a while so the prints that follow do not each pay the wait, then tried again.
const FETCH_MS = 2500, RETRY_MS = 20000;
const fetchT = (url) => fetch(url, { signal: AbortSignal.timeout(FETCH_MS) });
// Nothing a print waits for may wait forever. Everything that can hang gets the
// same budget as a re-read: the browser's own font loading, and its decode of
// the finished SVG.
const capped = (p, ms, msg) => { let t; return Promise.race([p, new Promise((_, no) => { t = setTimeout(() => no(new Error(msg)), ms); })]).finally(() => clearTimeout(t)); };
// A print must not be taken while the document is still loading a face, or it
// is folded and laid out on fallback metrics. `fonts.ready` settles when the
// faces are loaded and the last layout that wanted them is done; nothing here
// waited for it. It buys less than it looks: the glyphs themselves already
// travel, because `inlineCss` embeds a src `url()` as a data url like any other
// (and the moss faces are all `local()`), so a Chromium print carries the face
// with or without this wait — and a WebKit print does not carry it either way,
// because a WebKit SVG image will not load a data-url @font-face at all
// (measured 2026-09-13, harness/font-presence.mjs: the WebKit fallback-font gap
// is still open). A face that never arrives is not worth a hung capture: it
// gets a re-read's budget and the print goes out on the fallback, which is
// off-brand, not broken.
const fontsReady = (doc) => capped(doc.fonts ? doc.fonts.ready : Promise.resolve(), FETCH_MS, 'fonts timed out').catch(() => {});
const cached = (cache, key, get) => {
  const c = cache.get(key);
  if (c && (!c.failedAt || performance.now() - c.failedAt < RETRY_MS)) return c.p;
  const p = get().catch((e) => { cache.set(key, { p, failedAt: performance.now() }); throw e; });
  cache.set(key, { p });
  return p;
};
const dataCache = new Map(), cssCache = new Map();
function toData(url) {
  if (url.startsWith('data:')) return Promise.resolve(url);
  return cached(dataCache, url, () => fetchT(url).then((r) => r.blob()).then((bl) => new Promise((ok) => { const fr = new FileReader(); fr.onload = () => ok(fr.result); fr.readAsDataURL(bl); }))).catch(() => url);
}
const cssText = (href) => cached(cssCache, href, () => fetchT(href).then((r) => r.text())).catch(() => '');
async function inlineCss(css, base) {
  const urls = [...new Set([...css.matchAll(/url\((['"]?)([^'")]+)\1\)/g)].map((m) => m[2]).filter((u) => !u.startsWith('data:') && !u.startsWith('#')))];
  const got = await Promise.all(urls.map((u) => { let abs; try { abs = new URL(u, base).href; } catch { return null; } return toData(abs).then((d) => [u, d]); }));
  for (const g of got) if (g) css = css.split(g[0]).join(g[1]);
  return css;
}
const svgDoc = new DOMParser().parseFromString('<svg xmlns="http://www.w3.org/2000/svg"/>', 'image/svg+xml');
// A print is flat paint: no shadows, no filters. Safari draws a box-shadow or a
// backdrop-filter inside a foreignObject image as a hard offset block (the pill
// buttons of the shell grew shadow-shaped slabs beside them as the sheet cured),
// and the live page paints them again the moment it returns.
const FLAT = '*, *::before, *::after { box-shadow: none !important; text-shadow: none !important; filter: none !important; backdrop-filter: none !important; -webkit-backdrop-filter: none !important; }';
const flatStyle = (doc) => { const st = doc.createElement('style'); st.textContent = FLAT; return st; };
// One document's element tree, cloned to render standalone: scripts out,
// frames and canvases become empty blocks of their size, images and
// stylesheet urls inlined, comments out (a `--` inside one breaks XML).
// Whether a live element's own box, in its own document's current viewport
// (already what getBoundingClientRect() reads against — no scroll-offset
// math of its own needed, whatever document this is), falls inside the
// frame a print actually shows. Shared by `fold` (which reads it to decide
// what to inline) and `withholdOffscreen` (which reads it to decide what the
// live DOM should be allowed to fetch at all) so the two can never disagree
// about where "on screen" ends.
const rectInFrame = (vw, vh, r) => r.bottom > 0 && r.top < vh && r.right > 0 && r.left < vw;
// A document's own current viewport box — `fold` and `offscreenImgs` both
// need just the size (not `rasterDoc`'s scroll offsets too), so this is the
// one place either reads `scrollingElement` for it.
const docViewport = (doc) => { const vp = doc.scrollingElement || doc.documentElement; return [vp.clientWidth, vp.clientHeight]; };
async function fold(doc, liveRoot, options = {}) {
  const root = liveRoot.cloneNode(true);
  const live = [liveRoot, ...liveRoot.querySelectorAll('*')], copy = [root, ...root.querySelectorAll('*')];
  const drop = [], work = [];   // the inlining runs in parallel: one slow resource must not hold the rest
  // A print only ever shows the frame the reader is looking at — this doc's
  // own current viewport, the same box `rasterDoc` rasters into. An article
  // can carry many images below that (27 plates behind scene 1); DOM
  // `loading=lazy` already keeps the live page from fetching them until
  // scrolled to, and the fold must not undo that by reading every one of
  // them into a data url just to build a print of the first screen. Measured
  // 2026-09-17 (harness/scenes12-perf.mjs): fetching all of them at boot cost
  // Chromium +2.21MB/+170ms and WebKit +6MB/+287ms over the pre-article baseline.
  const [vw, vh] = docViewport(doc);
  for (let i = 0; i < live.length; i++) {
    const L = live[i], C = copy[i]; if (!C) break;
    const tag = L.tagName;
    if (tag === 'SCRIPT' || tag === 'NOSCRIPT' || tag === 'META' || tag === 'TITLE' || (tag === 'LINK' && L.rel !== 'stylesheet')) { drop.push(C); continue; }
    if (L.classList && L.classList.contains('cm-cursorLayer')) { drop.push(C); continue; }
    // A <picture>'s <source srcset> is never rewritten to an absolute or
    // inlined URL (only its sibling <img> is, below) — serialized as-is into
    // the print's foreignObject, its relative path resolves against the TOP
    // page's URL instead of the source document's, 404ing. The <img> already
    // carries the browser's own picked resource via currentSrc, srcset and
    // all, so the source is redundant for a print and safe to drop.
    if (tag === 'SOURCE') { drop.push(C); continue; }
    if (tag === 'IFRAME' || tag === 'CANVAS') {
      // Only ever the Mandelbrot's live WebGL2 field (the 'nb' card; 'sk'
      // is captured directly, in rasterScene3Artifact). Named explicitly as
      // 'webgl' rather than left to classify()'s tag-based default (which
      // is the 2D painter) -- that default would reintroduce the blind
      // toDataURL this routing exists to remove.
      if (tag === 'CANVAS' && options.snapshotCanvases && L.width && L.height) {
        const hole = doc.createElement('div'); hole.setAttribute('style', `display:block;width:${L.clientWidth || L.width}px;height:${L.clientHeight || L.height}px;`); if (L.id) hole.id = L.id; hole.className = L.className;
        C.replaceWith(hole);
        work.push(capped(WatercolorCapture.painters.webgl(L, { w: L.width, h: L.height, dpr: 1 }), FETCH_MS, 'canvas capture timed out').then((r) => {
          if (!r.ok) { reportCaptureFault(`canvas ${L.id || 'webgl'}: ${r.reason}`); return; } // hole already placed: the same fallback this branch always showed on failure
          const im = doc.createElement('img');
          im.setAttribute('style', `display:block;width:${L.clientWidth || L.width}px;height:${L.clientHeight || L.height}px;`);
          im.id = L.id; im.className = L.className;
          im.src = r.canvas.toDataURL('image/png');
          hole.replaceWith(im);
        }).catch((e) => reportCaptureFault(`canvas ${L.id || 'webgl'}: ${e.message}`)));
        continue;
      }
      const hole = doc.createElement('div'); hole.setAttribute('style', `display:block;width:${L.clientWidth}px;height:${L.clientHeight}px;` + (L.getAttribute('style') || '')); if (L.id) hole.id = L.id; hole.className = L.className; C.replaceWith(hole); continue;
    }
    // A video draws nothing inside a foreignObject image, so the print takes the
    // frame that is on screen: the element the sheet actually shows, frozen, the
    // same as every other pixel of a print. Its poster if no frame has decoded
    // yet, and a plain box if there is not even that.
    if (tag === 'VIDEO') {
      const cs = getComputedStyle(L);
      const im = doc.createElement('img');
      im.setAttribute('style', `display:block;width:${L.clientWidth}px;height:${L.clientHeight}px;object-fit:${cs.objectFit};border-radius:${cs.borderRadius};`);
      let frame = null;
      if (L.readyState >= 2 && L.videoWidth) {
        try { const cv = document.createElement('canvas'); cv.width = L.videoWidth; cv.height = L.videoHeight; cv.getContext('2d').drawImage(L, 0, 0); frame = cv.toDataURL('image/jpeg', 0.9); } catch (e) { frame = null; }
      }
      if (frame) im.setAttribute('src', frame);
      else if (L.poster) work.push(toData(L.poster).then((d) => im.setAttribute('src', d)));
      C.replaceWith(im); continue;
    }
    if (tag === 'IMG') { C.removeAttribute('srcset'); C.removeAttribute('loading'); const src = L.currentSrc || L.src;
      if (doc !== document && L.complete && L.naturalWidth) C.style.opacity = '1';
      // the plates are drawn onto the print from their loaded images, not through the raster (capture): no need to read them again.
      // `[data-lqip]` (not bare .plate): moss's own site CSS gives a hero image the
      // class "plate" too (its fit-mode, unrelated to this page's decorative sheets),
      // and without the attribute check a nested preview's own hero silently kept its
      // un-inlined, page-relative src — fine on its own document, a 404 once serialized
      // into this document's print.
      // A .sib card (scene 3's own background/chrome layers) is the same story
      // one level further: takePrint hides `.sib` outright (`.plate, .sib {
      // display: none }`, near takePrint below) because the wash never touches
      // them — they sit on top of the print as ordinary live DOM, page
      // furniture the same as the centre sheet's shadow. So nothing under
      // one is worth reading into a print, at any weight: this is what lets the
      // notebook window hold its real, heavy baked animation
      // (scene3/notebook/breed/mandelbrot-smooth-zoom.webp) instead of a shrunk
      // stand-in — a print never asks for its bytes, heavy or not, in the first
      // place. Before this exclusion, that read scaled with the source file's
      // own bytes and reliably stalled a capture long enough for a drag's
      // pointerdown to land on the wrong element underneath (scene3/README.md
      // has the reliability numbers behind that diagnosis).
      if (L.closest('.sib')) { C.removeAttribute('src'); continue; }
      // getBoundingClientRect() is already relative to this element's own
      // scrolling viewport (the doc's, whatever doc that is), so it needs no
      // scroll-offset math of its own to say whether the print's frame shows it.
      const onScreen = rectInFrame(vw, vh, L.getBoundingClientRect());
      // An offscreen clone keeps no src at all, inlined or not: a bare src
      // left on it is not inert (the comment above already found the
      // relative-path half of this — a page-relative one 404s once
      // serialized), an absolute one resolves fine and the foreignObject
      // fetches it for real, which is the same network cost `toData` would
      // have paid. Only an on-screen image is worth reading at all.
      if (!onScreen) { C.removeAttribute('src'); continue; }
      // A LQIP plate is drawn manually for the landing editor, but the same
      // class is used by the harvested preview article. Its visible image is
      // part of that document's print and must be inlined there; otherwise the
      // wash captures only the placeholder and the article image pops in live.
      if (src) work.push(toData(src).then((d) => {
        if (typeof d === 'string' && d.startsWith('data:')) maxPrintImgBytes = Math.max(maxPrintImgBytes, d.length);
        C.setAttribute('src', d);
      })); continue; }
    if (tag === 'STYLE') { work.push(inlineCss(L.textContent, doc.baseURI).then((css) => { C.textContent = css; })); continue; }
    if (tag === 'LINK') { const st = doc.createElement('style'); C.replaceWith(st); work.push(cssText(L.href).then((css) => inlineCss(css, L.href)).then((css) => { st.textContent = css; })); continue; }
    if (L.style && L.style.backgroundImage && L.style.backgroundImage.includes('url(')) work.push(inlineCss(L.style.backgroundImage, doc.baseURI).then((v) => { C.style.backgroundImage = v; }));
  }
  await Promise.all(work);
  drop.forEach((n) => n.remove());
  { const tw = doc.createTreeWalker(root, NodeFilter.SHOW_COMMENT); const cs = []; while (tw.nextNode()) cs.push(tw.currentNode); cs.forEach((c) => c.remove()); }
  return root;
}
// Render an element tree (plus the stylesheets it needs) at w x h into an Image.
async function raster(nodes, w, h, srcDoc = document) {
  await fontsReady(srcDoc);   // the engine must have the faces before it draws the image
  const host = document.createElement('div'); host.setAttribute('xmlns', 'http://www.w3.org/1999/xhtml');
  host.setAttribute('style', `position:relative;width:${w}px;height:${h}px;overflow:hidden;`);
  nodes.forEach((n) => host.appendChild(n));
  host.querySelectorAll('style').forEach((st) => { const css = st.textContent; st.textContent = ''; st.appendChild(svgDoc.createCDATASection(css.replace(/]]>/g, ']]]]><![CDATA[>'))); });
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><foreignObject width="100%" height="100%">${new XMLSerializer().serializeToString(host)}</foreignObject></svg>`;
  const img = new Image();
  // a decode that never finishes (a malformed serialization, an engine stall)
  // would hold the retake loop or a pour open for good; a distinct message so
  // the caller can tell a slow print from an impossible one
  await capped(new Promise((ok, no) => { img.onload = ok; img.onerror = () => no(new Error('scene raster failed')); img.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(svg); }), FETCH_MS, 'scene raster timed out');
  return img;
}
// The intro title's own small wash: a second makeSim instance sized to the
// h1's own box, driven by the same titleP that already reaches zero at scene
// 1's rest (updateOpening, above). Desktop only, and never under reduced
// motion — both keep the title exactly as plain scrolling text.
(async function setupTitleDissolve() {
  if (!TITLE_WASH || mobileLayout() || reduce) { titleReady = true; return; }
  // Wait for the page to boot AND come to rest before doing any of this: a
  // second WebGL2 context and its shader compiles are synchronous main-thread
  // work, and watchScrollDesktop's own spring clamps its per-frame dt to 0.1s
  // (springStep's stability margin, above), so a compile stall borrows real
  // time the spring never gets to spend — measured settling 60+px short of
  // rest after a single wheel tick. atRest() is the page's own existing
  // signal for "safe to do a background task now" (used to gate stale-print
  // retakes); titleP is 1 (nothing to show yet) for as long as the reader has
  // not scrolled, so nothing is lost by waiting for it here too.
  // A failed boot sets dataset.static instead of ever reaching data-ready
  // (see the ready().catch() path, below), so this loop already cannot
  // advance without it — checked anyway after every wait, cheap insurance
  // against a future change to that exclusivity leaving this polling forever
  // on a page that has already given up and shown the static fallback.
  while (document.documentElement.dataset.ready !== '1') {
    if (document.documentElement.dataset.static) { titleReady = true; return; }
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
  if (document.documentElement.dataset.static) { titleReady = true; return; }
  // Arm at idle after first paint, never mid-gesture (owner: the watercolor
  // sim is an enhancement, not something a scroll should ever have to wait
  // out). requestIdleCallback -- a timeout fallback for engines that lack
  // it, e.g. WebKit -- only fires once the main thread has nothing more
  // pressing queued; atRest() is checked too, since idle can still land
  // inside a reader's own gesture (a fling still coasting), and arming's
  // own stall would freeze that coast dead regardless of which renderer is
  // currently on screen.
  await new Promise((resolve) => {
    const schedule = typeof requestIdleCallback === 'function'
      ? (fn) => requestIdleCallback(fn, { timeout: 2000 })
      : (fn) => setTimeout(fn, 300);
    const tryArm = () => { if (atRest()) resolve(); else schedule(tryArm); };
    schedule(tryArm);
  });
  if (document.documentElement.dataset.static) { titleReady = true; return; }
  const liveRect = openingTitle.getBoundingClientRect();
  if (!liveRect.width || !liveRect.height) { titleReady = true; return; }
  const pad = GEOM.pad;
  // Cloned rather than styled by a shared class, so a future edit to #intro
  // h1's own rule cannot silently desync the two; the cost is borne once per
  // raster, not per frame. Named properties only: copying every computed
  // property (including ones that mean nothing here, like the live sticky
  // positioning) measured fine via a plain DOM append, but pushed the clone's
  // own box to the bottom of the print once serialized through raster()'s
  // SVG foreignObject — some property in the full set is read differently
  // there. This list is exactly what the task needs: font, size, colour,
  // letter-spacing and line breaks (the <br> tags clone with the node).
  const cloneTitle = () => {
    const cs = getComputedStyle(openingTitle);
    const node = openingTitle.cloneNode(true);
    node.style.cssText = `position:absolute; left:${pad}px; top:${pad}px; margin:0; font-family:${cs.fontFamily}; font-size:${cs.fontSize}; font-weight:${cs.fontWeight}; font-style:${cs.fontStyle}; line-height:${cs.lineHeight}; letter-spacing:${cs.letterSpacing}; color:${cs.color}; text-align:${cs.textAlign}; text-transform:${cs.textTransform}; white-space:${cs.whiteSpace};`;
    return node;
  };
  // Fully transparent target, at any size: nothing in it to stretch.
  const blank = document.createElement('canvas'); blank.width = blank.height = 1;
  const fallbackFade = () => { titleMode = 'fade'; titleDissolve = (p) => { openingTitle.style.opacity = String(p); }; titleDissolve(pendingTitleP); titleReady = true; };
  // A stroke a couple of px wetter than its own geometry, not a different
  // print: the swap still lands on the live h1's own pixels (anti-aliasing
  // already softens a 2px difference to invisible at reading distance), but
  // the print's footprint — what foot() downsamples the splash gate against
  // — stops vanishing into the coarse .ft mip between thin strokes.
  const dilate = (img, w, h, r) => {
    const c = document.createElement('canvas'); c.width = w; c.height = h;
    const g = c.getContext('2d');
    for (let dy = -r; dy <= r; dy++) for (let dx = -r; dx <= r; dx++) { if (dx * dx + dy * dy <= r * r) g.drawImage(img, dx, dy); }
    return c;
  };
  // The title's strokes are only 3-5 texels wide at 2 real px/texel — thin
  // enough that WATER's own per-step diffusion, fixed in TEXELS by step
  // count (not by anything this file can scale), reaches well past them
  // before the wash finishes, on screen. Running the title's grid finer
  // halves that spread's own on-screen reach without touching the shader:
  // the same texel-count blur now covers half the real distance. Measured
  // cost (commit body) stays inside a 60fps scroll at 1440x900 at 1; 0.75
  // is available if a future viewport needs it and the cost still allows.
  const TITLE_PX_PER_TEXEL = 1;
  let box = { x: liveRect.left - pad, y: liveRect.top - pad, w: Math.min(MAX_PRINT_EDGE, liveRect.width + 2 * pad), h: Math.min(MAX_PRINT_EDGE, liveRect.height + 2 * pad) };
  const clone = cloneTitle();
  clone.style.width = liveRect.width + 'px'; clone.style.height = liveRect.height + 'px';
  // dilate's radius is real px on the rasterized print (box.w x box.h,
  // independent of the sim's own grid): at the old 2 px/texel grid, 2 real
  // px was 1 texel of anti-vanishing margin. Keeping that same 1-texel
  // margin at the new resolution means the real-px radius scales with
  // TITLE_PX_PER_TEXEL too, rather than doubling the margin along with it.
  const TITLE_DILATE = Math.max(1, Math.round(TITLE_PX_PER_TEXEL));
  let print;
  try { print = dilate(await raster([clone], box.w, box.h), box.w, box.h, TITLE_DILATE); }
  catch (e) { console.warn('title print unavailable, falling back to a fade:', e.message); fallbackFade(); return; }
  if (document.documentElement.dataset.static) { titleReady = true; return; }
  const titleCanvas = document.createElement('canvas');
  titleCanvas.id = 'gl-title';
  // z-index:-1 (see #closing-film): stays behind scene 1's content.
  titleCanvas.style.cssText = 'position:fixed; left:0; top:0; z-index:-1; pointer-events:none; display:none;';
  document.body.appendChild(titleCanvas);
  const gridW = Math.round(box.w / TITLE_PX_PER_TEXEL), gridH = Math.round(box.h / TITLE_PX_PER_TEXEL);
  // A title's strokes cover a small fraction of its own box next to a
  // scene's photos and screenshots, so the same paint-strength release
  // (load 1, what the main wash always uses) reads as a thin stain rather
  // than ink bleeding. TITLE_LOAD raises how much suspended pigment each
  // step's dissolution releases (concentration, not rate — l still grows at
  // the same pace, so "completely gone" keeps its own timing).
  const TITLE_LOAD = 14;
  // The shared splash is eight discrete drops with a circular falloff
  // (WATER, drops[]/dropR): on a photo it hides inside the print's own
  // texture, but on bare glyph strokes it is the whole picture — a judge
  // read it as "a polka-dot brush". Zeroed here: no drop lands, so neither
  // the disk-shaped rise in h nor its outward vel kick exist. What is left
  // is the mist term alone (`uMist * f * paper-grain`), gated by the same
  // print footprint and shaped by the same paper texture every scene wash
  // reads.
  //
  // A prior pass chased a wrong reading of "legible": it pushed MIST/HOLD
  // to a hard, short pulse (0.48/0.019) to force the glyph-mask correlation
  // to peak by 25% specifically, a knife-edge under 0.05 of TITLE_MIST wide
  // on chromium and visibly darker and flatter at 25-50% than this pass's
  // own reference frames for it. The owner's actual bar is simpler: legible
  // through wetting early, then progressively lost, then gone — a floor at
  // 25%, not a peak there (assertion G, below, now asks for exactly that).
  // These are the same amplitude and hold this file used before
  // TITLE_PX_PER_TEXEL went from 2 to 1 (the previous commit): mistAmp and
  // mistHold are seconds- and height-per-step quantities, not inherently
  // tied to the grid's own texel size, and the finer grid's own effect
  // (WATER's per-step blur reaching half as far on screen for the same
  // texel-count spread) is what TITLE_PAPER_SCALE/TITLE_BLOCK below already
  // account for — reapplying the old values here and looking at the result
  // against the reference frames (scratchpad/phase1g/) confirmed the same
  // character carries over rather than needing its own re-derivation.
  const TITLE_SPLASH = 0, TITLE_MIST = 0.14, TITLE_MIST_HOLD = 0.08;
  // R (the splash radius) needs no rescaling for TITLE_PX_PER_TEXEL: with
  // TITLE_SPLASH=0 it multiplies out to nothing in both the h and vel terms
  // (WATER) regardless of its own value, so it is unaffected either way.
  //
  // uPaper's 256-texel period reads at a different size on screen per
  // instance, because "texel" isn't a fixed screen size: the main wash's
  // grid is a fixed 820x780 design-px box (BASE_TEX_W x BASE_TEX_H) at
  // 410x390 texels — 2 design px/texel — rendered through the stage's own
  // `--s` (SCALE, this file's global) design-px-to-real-px factor, so its
  // on-screen px/texel is 2*SCALE. The title's box, by contrast, is
  // #intro's own live layout, not inside that transform, so its
  // gridW = box.w/TITLE_PX_PER_TEXEL already IS TITLE_PX_PER_TEXEL *real*
  // px/texel, unscaled. One period is 256 texels either way, so on screen:
  // main = 256*2*SCALE px, title (at uPaperScale=1) =
  // 256*TITLE_PX_PER_TEXEL px. Setting uPaperScale =
  // TITLE_PX_PER_TEXEL/(2*SCALE) shrinks the title's period by exactly the
  // ratio needed (P() divides t*uPaperScale, so the period in texels is
  // 256/uPaperScale) to match the main wash's on-screen grain size. Half
  // its previous value now that TITLE_PX_PER_TEXEL is 1 instead of 2 — each
  // texel is half the real screen size, so half as much scaling is needed
  // to reach the same physical period. SCALE moves during the intro's own
  // zoom, so this reads it once, at setup — the same moment gridW/gridH
  // themselves are fixed for this sim's lifetime (see onResize, below:
  // uSize is never re-issued either).
  const TITLE_PAPER_SCALE = TITLE_PX_PER_TEXEL / (2 * Math.max(0.05, SCALE));
  // BLOCK is the same on-screen-match problem as the paper period, with 16
  // texels (PIG's uNear neighbourhood) standing in for uPaper's 256:
  // TITLE_BLOCK*TITLE_PX_PER_TEXEL (title's on-screen block size) must
  // equal 16*2*SCALE (main's), the same equality TITLE_PAPER_SCALE solves,
  // so TITLE_BLOCK = 16/TITLE_PAPER_SCALE falls out of it directly.
  const TITLE_BLOCK = Math.max(1, Math.round(16 / TITLE_PAPER_SCALE));
  // What is left, after the above (verified via ?diag=12, uPaper's own
  // fibre channel, and ?diag=6, the sim's actual water depth: the first is
  // now fine grain at this scale, the second still shows the same large
  // round lumps as before the fix). The lumps are not the paper texture —
  // they are WATER's own per-step neighbour blur, `h = mix(h, hl, 0.25)`,
  // compounding over the ~hundreds of steps a wash runs, a diffusion
  // process whose length scale is set by step count, not by what fed it.
  // The main wash's own grid absorbs that spread across many more texels;
  // the title's smaller grid does not. A fix would need that blur (or an
  // equivalent) to become instance-aware too — a second shader change,
  // not the one this pass authorized.
  await warmShaderCache();
  if (document.documentElement.dataset.static) { titleReady = true; return; }
  const titleSim = makeSim({ canvas: titleCanvas, texW: gridW, texH: gridH, rect: () => box, load: TITLE_LOAD, splashAmp: TITLE_SPLASH, mistAmp: TITLE_MIST, mistHold: TITLE_MIST_HOLD, blockSize: TITLE_BLOCK, paperScale: TITLE_PAPER_SCALE });
  // Cost guard: the 4x-larger grid (TITLE_PX_PER_TEXEL halved from 2) costs
  // real GPU time now — measured (real hardware, headless:false; headless
  // Playwright's default SwiftShader answer is not this machine's own) a
  // synced (gl.readPixels-forced) 36-step burst, STEPS_PER_FRAME's own
  // default budget, at 17.5-20.2ms, over a 16.67ms/60fps frame. 24 steps
  // measured a steady 13.0ms, with margin; a worse-case single-frame catch-
  // up after a hard scroll jump now takes more frames instead of stalling
  // one. Only this instance's budget changes — the shared default and the
  // main wash's own call sites are untouched.
  const TITLE_STEP_BUDGET = 24;
  if (!titleSim) { titleCanvas.remove(); fallbackFade(); return; }
  let broken = false;
  // No webglcontextrestored handler: the canvas is removed here, same as the
  // !titleSim path above, so there is nothing left to restore into — a
  // future context would need a fresh setupTitleDissolve, which only runs
  // again on the next full page load.
  const fallBack = () => {
    broken = true;
    titleCanvas.getContext('webgl2')?.getExtension('WEBGL_lose_context')?.loseContext();
    titleCanvas.remove();
    fallbackFade();
  };
  titleCanvas.addEventListener('webglcontextlost', (e) => { e.preventDefault(); fallBack(); });
  titleSim.setPrints(print, blank);
  titleSim.reset();
  // Ink remaining is read from the sim's own dissolved-fraction state (l),
  // weighted by where the print's own ink was, at the sim's own grid — not
  // from the rendered pixels. l only ever grows within a forward wash, so
  // this stays monotone through a bloom that spreads pigment outward and can
  // raise total on-screen coverage while it lifts, and across two engines
  // whose per-step rate is not bit-identical. drawImage's y-axis runs top
  // down; the print was uploaded with UNPACK_FLIP_Y_WEBGL, so the mask is
  // flipped to land in the same row order as the state grid's own readback.
  let mask = null, maskTotal = 1, solidFrac = 0;
  const captureMask = (img) => {
    const c = document.createElement('canvas'); c.width = gridW; c.height = gridH;
    const g = c.getContext('2d'); g.translate(0, gridH); g.scale(1, -1); g.drawImage(img, 0, 0, gridW, gridH);
    const d = g.getImageData(0, 0, gridW, gridH).data;
    const m = new Float32Array(gridW * gridH);
    let total = 0, solid = 0;
    for (let i = 0, p = 0; i < d.length; i += 4, p++) { m[p] = d[i + 3] / 255; total += m[p]; if (m[p] > 0.5) solid++; }
    mask = m; maskTotal = Math.max(1, total); solidFrac = solid / (gridW * gridH);
  };
  captureMask(print);
  landing.title.ink = () => {
    // Armed does not mean handed the h1 yet (driveTitleDissolve only swaps
    // titleDissolve to washDissolve at a titleP boundary, above) -- the
    // canvas stays hidden and introMaskStep keeps being the real answer
    // until it does, same fallback as the pre-arm stub this overwrites.
    if (broken || getComputedStyle(titleCanvas).display === 'none') return introMaskStep < 0 ? Number(getComputedStyle(openingTitle).opacity) : 1 - introMaskStep / 24;
    return titleSim.remaining(mask, maskTotal);
  };
  // Raw readbacks only — no verdict here, so a check can grade what the
  // reader actually sees instead of grading the page's own opinion of it.
  // readAlpha resamples the rendered canvas down to the sim's own grid
  // (gridW x gridH), the same resolution title.mask is already at, so a
  // caller can compare the two index-for-index without knowing anything
  // about devicePixelRatio or the print's own pixel size. No display guard
  // here (raw(), below, adds its own): the one-time rest capture below
  // needs a real readback while the canvas is still display:none.
  const readAlpha = () => {
    const c = document.createElement('canvas'); c.width = gridW; c.height = gridH;
    const g = c.getContext('2d'); g.drawImage(titleCanvas, 0, 0, titleCanvas.width, titleCanvas.height, 0, 0, gridW, gridH);
    const d = g.getImageData(0, 0, gridW, gridH).data;
    // drawImage skips CSS opacity; fold it back in so this reads the page.
    const op = titleCanvas.style.opacity || 1;
    const alpha = new Array(gridW * gridH);
    for (let i = 3, p = 0; i < d.length; i += 4, p++) alpha[p] = d[i] * op;
    return { w: gridW, h: gridH, alpha };
  };
  landing.title.raw = () => {
    if (broken || getComputedStyle(titleCanvas).display === 'none') return { w: gridW, h: gridH, alpha: new Array(gridW * gridH).fill(0) };
    return readAlpha();
  };
  landing.title.mask = () => ({ w: gridW, h: gridH, mask: Array.from(mask) });
  // Mask-weighted mean alpha, the same weighting remaining() uses on the
  // sim's own internal state -- verified index-aligned with raw() by a
  // row-mass probe against the known, asymmetric line positions from
  // title.lines() (both land the same text rows at the same row fraction; a
  // genuine mismatch would not), same alignment G's own raw-vs-mask
  // correlation already assumes.
  const weighted = (alpha) => {
    let sum = 0;
    for (let p = 0; p < alpha.length; p++) sum += (alpha[p] / 255) * mask[p];
    return sum / maskTotal;
  };
  landing.title.renderedAlpha = () => broken || getComputedStyle(titleCanvas).display === 'none' ? 0 : weighted(readAlpha().alpha);
  // Each rendered line's own box, as a fraction of the print's own box
  // (resolution-independent, so a caller scales onto title.raw/title.mask
  // or onto a screenshot equally) — for the per-line, per-column coverage
  // grid a legibility check needs.
  landing.title.lines = () => {
    const range = document.createRange();
    range.selectNodeContents(openingTitle);
    return [...range.getClientRects()].map((r) => ({ x: (r.left - box.x) / box.w, y: (r.top - box.y) / box.h, w: r.width / box.w, h: r.height / box.h }));
  };
  const clock = { t: 0, drawn: -1 };
  let shownAs = 'solid';   // 'solid' | 'wash' | 'gone' — which of {h1, canvas} owns the pixels
  // A later cure onset than the main wash's own T_CURE: only draw() reads
  // this value (T_CURE/T_TOTAL/the step math stay the shared constants), so
  // the bloom this releases stays suspended and visible longer before the
  // display starts blending it toward the (blank) target.
  const TITLE_CURE = T_CURE * 1.35;
  const paint = (t) => titleSim.draw(true, smooth(TITLE_CURE, T_TOTAL, t));
  // Captured once, synchronously, right here: the canvas's own rendered
  // alpha at a genuinely undissolved sim (t=0), before any real step or
  // display flip -- the reference "fully there" reading renderedInk (below)
  // normalizes against, the same role h1's own opacity=1 plays for ink().
  // Without this, weighted()'s natural scale (a mask-weighted mean over a
  // glyph print, not a solid block) sits well under 1.0 at rest, which
  // would misread as most of the ink already gone the moment the canvas
  // takes over. Reading the canvas straight after this synchronous paint()
  // needs no display flip: readAlpha() draws the WebGL canvas's own pixels
  // regardless of its CSS display, which only hides it from the reader.
  paint(0);
  const restRenderedAlpha = Math.max(1e-6, weighted(readAlpha().alpha));
  // The h1-to-canvas handoff's own continuity check (public: a probe or a
  // check reads this instead of reinventing the force-visible readback
  // above). 1.0 at rest (h1 solid or a freshly primed, undrawn canvas),
  // falling toward 0 as the wash actually clears -- same scale as ink(),
  // but built from what is actually painted rather than the sim's internal
  // dissolved-fraction state, which races ahead of it (see TITLE_LOAD).
  landing.title.renderedInk = () => {
    if (broken || getComputedStyle(titleCanvas).display === 'none') return 1;
    return weighted(readAlpha().alpha) / restRenderedAlpha;
  };
  // The simplest monotonic map: scroll fraction straight to clock seconds.
  // T_SPLASH(0.2)/T_TAKE(1.2) put the splash and the start of real dissolving
  // within the first ~10-55% of scroll, TITLE_CURE(~1.2) delays the fade so
  // 50% is still mid-bleed, and it lands on exactly T_TOTAL at 100% — no
  // separate legs, no tuned knots, so there is nothing here fighting what the
  // fixed splash actually does over time.
  const titleGoal = (s) => s * s * (3 - 2 * s) * T_TOTAL;
  washDissolve = (titleP) => {
    if (broken) return;
    const goal = titleGoal(1 - clamp01(titleP));
    // A little agitation, the way a real hand's scroll stirs the main wash's
    // own film — constant here because the title is a fixed clock, not a
    // speed the reader sets — spreads the bloom past single stroke edges.
    const advanced = advanceWash(titleSim, clock, { goal, fwd: true, stir: 0.15, budget: TITLE_STEP_BUDGET }, paint);
    titleSteps += advanced.count;
    lastWashDiag = { goal: +goal.toFixed(4), clockT: +clock.t.toFixed(4), steps: advanced.count };
    // SHOW's grain haze never fully clears; fade opacity out before the cut.
    titleCanvas.style.opacity = String(1 - smooth(T_TOTAL * .5, T_TOTAL * .8, clock.t));
    if (clock.t <= 0) {
      if (shownAs !== 'solid') { titleCanvas.style.display = 'none'; openingTitle.style.opacity = '1'; shownAs = 'solid'; }
    } else if (clock.t >= T_TOTAL - 1e-6) {
      if (shownAs !== 'gone') { titleCanvas.style.display = 'none'; openingTitle.style.opacity = '0'; shownAs = 'gone'; }
    } else if (shownAs !== 'wash') {
      openingTitle.style.opacity = '0'; titleCanvas.style.display = 'block'; shownAs = 'wash';
    }
  };
  // Called once, by driveTitleDissolve, in the same tick it swaps over --
  // see primeWash's own declaration (above) for why a fresh clock is only
  // right for the titleP===1 boundary on its own.
  primeWash = (titleP) => {
    clock.t = titleP === 1 ? 0 : T_TOTAL;
    clock.drawn = -1;
    shownAs = titleP === 1 ? 'solid' : 'gone';
    titleCanvas.style.display = 'none';
    openingTitle.style.opacity = titleP === 1 ? '1' : '0';
  };
  // Armed, not yet active: driveTitleDissolve swaps titleDissolve to
  // washDissolve (and titleMode to 'wash') itself, the next time it sees
  // titleP land on a boundary -- immediately, if pendingTitleP is already
  // there (a cold, unscrolled load: titleP is 1 until the reader's first
  // gesture), never mid-dissolve otherwise.
  simArmed = true;
  titleReady = true;
  // Re-raster on resize or a language swap that changes the box: never
  // mid-dissolve unless the size actually moved, so a reader mid-scroll never
  // sees a pop. A resize past the mobile breakpoint tears the wash down and
  // returns to plain scrolling text, matching a page that loaded there.
  let resizeTimer = null;
  const onResize = () => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(async () => {
      if (mobileLayout() || reduce) { fallBack(); return; }
      const r2 = openingTitle.getBoundingClientRect();
      if (!r2.width || !r2.height) return;
      const w2 = Math.min(MAX_PRINT_EDGE, r2.width + 2 * pad), h2 = Math.min(MAX_PRINT_EDGE, r2.height + 2 * pad);
      const sizeChanged = Math.abs(w2 - box.w) > 1 || Math.abs(h2 - box.h) > 1;
      if (shownAs === 'wash' && !sizeChanged) { box = { x: r2.left - pad, y: r2.top - pad, w: box.w, h: box.h }; return; }
      const node = cloneTitle();
      node.style.width = r2.width + 'px'; node.style.height = r2.height + 'px';
      let print2;
      try { print2 = dilate(await raster([node], w2, h2), w2, h2, TITLE_DILATE); } catch (e) { return; }
      box = { x: r2.left - pad, y: r2.top - pad, w: w2, h: h2 };
      titleSim.setPrints(print2, blank);
      titleSim.reset();
      captureMask(print2);
      clock.t = 0; clock.drawn = -1; shownAs = 'solid';
      titleDissolve(pendingTitleP);
    }, 200);
  };
  addEventListener('resize', onResize);
})();
// A whole document, laid out at w x h, scrolled as it is on screen.
async function rasterDoc(doc, w, h, options = {}) {
  const se = doc.scrollingElement || doc.documentElement;
  const html = await fold(doc, doc.documentElement, options);
  // inside the SVG image `:root` is the <svg>, so a document's `:root` rules
  // (and its `prefers-color-scheme: dark` guard on `:root:not([data-theme=light])`)
  // would land on the wrong element and inherit down as the OS theme
  html.querySelectorAll('style').forEach((st) => { st.textContent = st.textContent.replace(/:root\b/g, 'html'); });
  // percentage heights in the document resolve against these, as they did against the frame
  // at the width the live document lays out in (a scrollbar, if it has one, takes the rest)
  html.style.width = (se.clientWidth || w) + 'px'; html.style.height = h + 'px'; html.style.overflow = 'visible';
  // in a real document body's overflow propagates to the viewport; inside the
  // foreignObject there is none, so body would become a scroll container and
  // grow a scrollbar that steals width from everything it centres
  const body = html.querySelector('body'); if (body) { body.style.height = h + 'px'; body.style.overflow = 'visible'; }
  html.appendChild(flatStyle(doc));
  const shift = document.createElement('div'); shift.setAttribute('style', `position:absolute;left:${-se.scrollLeft}px;top:${-se.scrollTop}px;width:${w}px;height:${h}px;`);
  shift.appendChild(html);
  const base = await raster([shift], w, h + se.scrollTop, doc);
  // WebKit can paint an image's LQIP background but omit its decoded <img>
  // inside SVG foreignObject. Composite those same live pixels directly,
  // with their real fit and clipping, so photos share the pigment surface.
  const images = [...doc.images].filter(im => im.complete && im.naturalWidth && rectInFrame(w, h, im.getBoundingClientRect()));
  if (!images.length) return base;
  const result = document.createElement('canvas'); result.width = base.width; result.height = base.height;
  const g = result.getContext('2d'); g.drawImage(base, 0, 0);
  for (const im of images) {
    const r = im.getBoundingClientRect(), css = doc.defaultView.getComputedStyle(im);
    if (css.visibility === 'hidden' || css.display === 'none' || !r.width || !r.height) continue;
    g.save(); g.beginPath(); g.rect(0, 0, w, h); g.clip();
    for (let el = im; el && el !== doc.body; el = el.parentElement) {
      const style = doc.defaultView.getComputedStyle(el);
      if (el === im || /hidden|clip|auto|scroll/.test(style.overflow)) {
        const rect = el.getBoundingClientRect();
        g.beginPath(); g.roundRect(rect.x, rect.y, rect.width, rect.height, parseFloat(style.borderRadius) || 0); g.clip();
      }
    }
    const fit = css.objectFit, scale = fit === 'contain' ? Math.min(r.width / im.naturalWidth, r.height / im.naturalHeight) : Math.max(r.width / im.naturalWidth, r.height / im.naturalHeight);
    const dw = fit === 'fill' ? r.width : im.naturalWidth * scale, dh = fit === 'fill' ? r.height : im.naturalHeight * scale;
    const pos = css.objectPosition.split(/\s+/).map(v => v.endsWith('%') ? parseFloat(v) / 100 : .5);
    g.drawImage(im, r.x + (r.width - dw) * (pos[0] ?? .5), r.y + (r.height - dh) * (pos[1] ?? .5), dw, dh);
    g.restore();
  }
  return result;
}
// A shell frame's own chrome and, when it wraps a moss preview page, that
// page too — the two-layer print every scene from 'live' on needs for its
// own capture (takePrint). Coordinates are local to the frame's own w x h; a
// caller compositing at an offset onto a larger canvas adds it.
async function rasterFrame(frame, w, h, withInner) {
  const jobs = [rasterDoc(frame.contentDocument, w, h)];
  let innerR = null, innerRadius = 0;
  if (withInner) {
    const f = frame.contentDocument.getElementById('moss-preview-iframe');
    if (f && f.contentDocument) {
      const fr = f.getBoundingClientRect();
      innerR = { x: Math.round(fr.left), y: Math.round(fr.top), w: Math.round(fr.width), h: Math.round(fr.height) };
      innerRadius = parseFloat(getComputedStyle(f).borderRadius) || 0;
      jobs.push(rasterDoc(f.contentDocument, innerR.w, innerR.h));
    }
  }
  const [frameImg, innerImg] = await Promise.all(jobs);
  return { frameImg, innerImg, innerR, innerRadius };
}
// Each plate as the page shows it: its layout box (transform leaves offset*
// alone), turned by --r about its centre, the image cropped as object-fit
// cover with object-position, the LQIP colour under it until the image is in.
function drawPlates(g, k) {
  for (const pl of document.querySelectorAll('.plate')) {
    const cs = getComputedStyle(pl);
    const x = pl.offsetLeft - printRect.x, y = pl.offsetTop - printRect.y, w = pl.offsetWidth, h = pl.offsetHeight;
    const rot = parseFloat(cs.getPropertyValue('--r')) * Math.PI / 180 || 0, radius = parseFloat(cs.borderRadius) || 0;
    g.save();
    g.translate((x + w / 2) * k, (y + h / 2) * k); g.rotate(rot); g.translate(-w / 2 * k, -h / 2 * k);
    g.beginPath(); g.roundRect(0, 0, w * k, h * k, radius * k); g.clip();
    g.fillStyle = cs.backgroundColor; g.fillRect(0, 0, w * k, h * k);
    const im = pl.querySelector('img');
    if (im && im.complete && im.naturalWidth) {
      const pos = (cs.getPropertyValue('--pos') || '50% 50%').trim().split(/\s+/).map((v) => parseFloat(v) / 100);
      const sc = Math.min(w / im.naturalWidth, h / im.naturalHeight), sw = w / sc, sh = h / sc;
      const sx = (im.naturalWidth - sw) * (pos[0] ?? 0.5), sy = (im.naturalHeight - sh) * (pos[1] ?? 0.5);
      g.drawImage(im, sx, sy, sw, sh, 0, 0, w * k, h * k);
    }
    g.restore();
  }
}
function roundClip(g, r, k, radius) { g.beginPath(); g.roundRect(r.x * k, r.y * k, r.w * k, r.h * k, radius * k); g.clip(); }
// Scene 3's three artifacts are live layers rather than part of the shell
// print. Mobile needs their current pixels in the wash target, so rasterize
// the two same-origin documents (including a live sketch canvas) and freeze
// the detached video's already-decoded frame without asking for new assets.
async function rasterScene3Artifact(id) {
  const c = CARDS[id], el = c && c.el;
  if (!c || !el) return null;
  if (id === 'video') {
    const v = el.querySelector('video');
    if (!v) return null;
    if (v.readyState < 2 || !v.videoWidth) {
      const poster = new Image(); poster.src = v.poster || v.dataset.poster;
      try { await capped(poster.decode(), FETCH_MS, 'video poster timed out'); return poster; } catch (e) { return null; }
    }
    try {
      const cv = document.createElement('canvas'); cv.width = v.videoWidth; cv.height = v.videoHeight;
      cv.getContext('2d').drawImage(v, 0, 0);
      const im = new Image(); im.src = cv.toDataURL('image/jpeg', .9); await im.decode();
      return im;
    } catch (e) { return null; }
  }
  const frame = el.querySelector('iframe');
  const doc = frame && frame.contentDocument;
  if (!doc) return null;
  if (id === 'sk') {
    const canvas = doc.querySelector('canvas');
    if (!canvas) return null;
    // mandelbrot-renderer.js's own field, via the registry's webgl painter:
    // same mossCaptureFrame hook, no blind toDataURL fallback. drawImage
    // accepts its canvas the same as it accepted the Image this returned
    // before. A timeout/declared failure degrades to a missing artifact
    // (takePrint's own artifactImages check), never a rejection that would
    // take the whole print down with it.
    try {
      const r = await capped(WatercolorCapture.painters.webgl(canvas, { w: canvas.width, h: canvas.height, dpr: 1 }), FETCH_MS, 'sketch frame timed out');
      if (!r.ok) reportCaptureFault(`sk: ${r.reason}`);
      return r.ok ? r.canvas : null;
    } catch (e) { reportCaptureFault(`sk: ${e.message}`); return null; }
  }
  try { return await rasterDoc(doc, Math.round(c.width), Math.round(c.height), { snapshotCanvases: true }); }
  catch (e) { console.warn(`Artifact ${id}: ${e.message}`); return null; }
}
function drawScene3Artifacts(g, k, images, raised = null) {
  const ids = [...S3_ORDER].sort((a, b) => (+(CARDS[a].el.style.zIndex || 0)) - (+(CARDS[b].el.style.zIndex || 0)));
  const ox = -printRect.x, oy = -printRect.y;
  for (const id of ids) {
    const im = images[id], c = CARDS[id], p = c.pose;
    if (!im || !p) continue;
    const isRaised = !!(c.docked || s3Drag?.id === id || c.el.matches(':focus-visible'));
    if (raised !== null && isRaised !== raised) continue;
    const w = c.width * p.s, h = c.height * p.s;
    g.save();
    // Docked artifacts are clipped by the article's measured paper aperture;
    // outside artifacts retain their full current pose. Normal outside cards
    // are drawn before the preview window; held/focused and docked cards are
    // drawn after it, matching cardDepths() and the live z-order.
    if (c.docked) {
      const x = Math.max(p.x, s3Paper.x), y = Math.max(p.y, s3Paper.y);
      const r = Math.min(p.x + w, s3Paper.x + s3Paper.w), b = Math.min(p.y + h, s3Paper.y + s3Paper.h);
      if (r <= x || b <= y) { g.restore(); continue; }
      g.beginPath(); g.rect((x + ox) * k, (y + oy) * k, (r - x) * k, (b - y) * k); g.clip();
    }
    g.drawImage(im, (p.x + ox) * k, (p.y + oy) * k, w * k, h * k);
    g.restore();
  }
}
async function prepareScene3Capture() {
  warmScene3Media();
  const waitFrame = (frame) => {
    if (frame.contentDocument?.readyState === 'complete' && frame.contentDocument.location.href !== 'about:blank') return Promise.resolve();
    return new Promise((resolve) => frame.addEventListener('load', resolve, { once: true }));
  };
  await capped(Promise.all([waitFrame($('sk')), waitFrame($('nb'))]), FETCH_MS, 'scene 3 media timed out').catch(() => {});
  await nestedPreviewReady(SHIPS);
  const video = $('s3-video-el');
  // A paused, preload-none video uses its existing poster until playback starts.
  // Capture can warm scene 3 while another scene is shown. Establish the same
  // deterministic poses enterScene3() would have supplied before measuring or
  // rasterizing any artifact.
  if (!s3Initialized) { placeScene3Artifacts(); s3Initialized = true; }
  cardDepths();
  let ready = false;
  for (let i = 0; i < 8 && !ready; i++) {
    ready = layoutScene3Layers();
    if (!ready) await new Promise((resolve) => requestAnimationFrame(resolve));
  }
  sketchVisible(true);
  await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  sketchVisible(false);
  if (!ready) throw new Error('scene 3 layers not ready');
}
// The print of the scene the DOM is showing now (the wash switches the DOM
// under its own canvas before asking for the target's print).
let captureMs = 0, washT = 0, recordSteps = 0;
// Counters for the harness only; nothing in the page reads them. A wrapper so
// that the count is right however `takePrint` leaves (returned, thrown, timed out).
let captures = 0, inFlight = 0, peakInFlight = 0;
// The heaviest single <img> any print has actually inlined (fold(), IMG
// branch below) — a general trip-wire, not keyed to any one file: a heavy
// image that slips into a print despite the .sib/.plate exclusions shows up
// here as a byte count instead of as an intermittent drag failure.
let maxPrintImgBytes = 0;
// A painter or artifact a print degraded past rather than threw away.
// Bounded so a fault firing every tick cannot grow this unbounded.
const captureFaults = [];
function reportCaptureFault(where) { captureFaults.push({ t: performance.now(), where }); if (captureFaults.length > 20) captureFaults.shift(); }
landing.captureFaults = () => captureFaults.slice();
async function capture(scene) {
  // Checked here, not by letting an outside caller replace this function's
  // export, so it reaches every caller of capture() including the ones
  // inside this script that only ever hold the local binding.
  if (landing.faults.captureHang) return new Promise(() => {});
  captures++; inFlight++; peakInFlight = Math.max(peakInFlight, inFlight);
  const generation = printGeneration;
  try {
    const c = await takePrint(scene);
    c.__printGeneration = generation;
    return c;
  } finally { inFlight--; }
}
async function takePrint(scene) {
  const t0 = performance.now();
  if (scene === 1) {
    await nestedPreviewReady(scene);
    const doc = shFrame.contentDocument?.getElementById('moss-preview-iframe')?.contentDocument;
    if (doc) {
      const [width, height] = docViewport(doc);
      const images = [...doc.querySelectorAll('img[src]')].filter(im => rectInFrame(width, height, im.getBoundingClientRect()));
      await capped(Promise.all(images.map(im => { im.loading = 'eager'; return im.decode().catch(() => {}); })), FETCH_MS, 'preview image decode timed out').catch(() => {});
    }
  }
  const artifactIds = scene === SHIPS ? [...S3_ORDER] : [];
  if (artifactIds.length) await prepareScene3Capture();
  const k = Math.min(mobileLayout() ? 1 : 2, devicePixelRatio || 1, MAX_PRINT_EDGE / Math.max(printW(), printH()));
  // the stage: plates, the box's shadow and ground, frames as holes
  const stageC = await fold(document, stage);
  // A print is the sheet at rest: not mid-wash, and not with scene 4's fan
  // already out. The strokes are page furniture the same as the siblings and
  // the shadow — they are drawn after the sheet has dried, and the sheet the
  // wash dissolves and cures is the control at the size it was left.
  stageC.classList.remove('morphing', 'fanned');
  stageC.querySelector('#gl')?.remove(); stageC.querySelector('#fan')?.remove(); stageC.querySelectorAll('.ground').forEach((n) => n.remove());
  const styles = [...document.querySelectorAll('style')].map((st) => st.cloneNode(true));
  // The plates are drawn straight onto the print from their loaded images,
  // not through the SVG raster: Safari tiles an <img> inside a foreignObject
  // image, and a tiled plate dissolves into a grid.
  // The siblings are not in the print at all: the wash dissolves the centre
  // sheet, and they are page furniture around it, the same as its shadow.
  const noPlates = document.createElement('style'); noPlates.textContent = '.plate, .sib, #s3-video { display: none !important; }'; styles.push(noPlates, flatStyle(document));
  const at = document.createElement('div'); at.setAttribute('style', `position:absolute;left:${-printRect.x}px;top:${-printRect.y}px;width:${GEOM.cellW}px;height:${GEOM.cellH}px;--s:1;`);
  at.appendChild(stageC);
  // the box's own layout box, not its rect: in scene 4 the grown control has a
  // transform on it, and a transform moves the rect while the print is of the
  // sheet at rest
  const boxR = { x: box.offsetLeft - printRect.x, y: box.offsetTop - printRect.y, w: box.offsetWidth, h: box.offsetHeight };
  const radius = parseFloat(getComputedStyle(box).borderRadius) || 0;
  const frame = FRAMES[scene];
  // every scene but the editor is the shell around a page of its own; scene 4 keeps none of the page
  const jobs = [
    raster([...styles, at], printW(), printH()),
    rasterFrame(frame, Math.round(boxR.w), Math.round(boxR.h), scene >= 1 && scene !== DEPLOY),
  ];
  const artifactJobs = artifactIds.map((id) => rasterScene3Artifact(id));
  const [stageImg, shell, ...artifactImages] = await Promise.all([...jobs, ...artifactJobs]);
  // A missing artifact degrades to its own honest gap -- drawScene3Artifacts
  // already skips an id with no image (its `if (!im || !p) continue`) --
  // rather than throwing the whole print away, which used to take every
  // other scene's wash down with it for the rest of the session (capture()'s
  // callers latch printable=false on any throw here).
  artifactIds.forEach((id, i) => { if (!artifactImages[i]) reportCaptureFault(`scene3 artifact ${id}`); });
  const { frameImg, innerImg, innerRadius } = shell;
  // The inner frame's own rect (rasterFrame's, local to the shell) is
  // shifted onto the box's, in the cell's coordinates: the shell's viewport
  // is the box's own width in the shell's own CSS pixels, whatever the whole
  // composition is scaled to on screen. Dividing it by that scale again drew
  // the inner page 753 wide inside a 692 window at a 1440 one — latent while
  // the page's max-width was 1440 and --s was 1 there, live since the third
  // scene's two siblings widened it to 1584.
  const innerR = shell.innerR && { x: boxR.x + shell.innerR.x, y: boxR.y + shell.innerR.y, w: shell.innerR.w, h: shell.innerR.h };
  const cut = scene === DEPLOY && ((cs) => (n) => parseFloat(cs.getPropertyValue(n)) || 0)(getComputedStyle(stage));
  // The sheet from its layers. Scene 3's cards are layered between the
  // window's own, so a card read afresh at a leg's start (memberPrint) is
  // composed back in here, at its current pose, not pasted over the top.
  const pw = printW(), ph = printH();
  const compose = (images) => {
    const c = document.createElement('canvas'); c.width = pw * k; c.height = ph * k;
    const g = c.getContext('2d');
    g.drawImage(stageImg, 0, 0, c.width, c.height);
    if (scene === 0) drawPlates(g, k);
    if (artifactIds.length) drawScene3Artifacts(g, k, images, false);
    g.save();
    // Scene 4's sheet is the shell cut to the control's own circle, the same cut
    // the stylesheet makes on the live box: the print carries the real pixels of
    // the real control and nothing of the window around it.
    if (cut) { g.beginPath(); g.arc((boxR.x + cut('--pub-cx')) * k, (boxR.y + cut('--pub-cy')) * k, cut('--pub-r') * k, 0, Math.PI * 2); g.clip(); }
    else roundClip(g, boxR, k, radius);
    g.drawImage(frameImg, boxR.x * k, boxR.y * k, boxR.w * k, frameImg.height * k); g.restore();
    if (innerImg) { g.save(); roundClip(g, boxR, k, radius); roundClip(g, innerR, k, innerRadius); g.drawImage(innerImg, innerR.x * k, innerR.y * k, innerR.w * k, innerImg.height * k); g.restore(); }
    if (artifactIds.length) {
      drawScene3Artifacts(g, k, images, true);
      c.artifacts = images;
      c.recompose = (fresh) => { const r = compose({ ...images, ...fresh }); r.__printGeneration = c.__printGeneration; return r; };
    }
    return c;
  };
  const c = compose(Object.fromEntries(artifactIds.map((id, i) => [id, artifactImages[i]])));
  captureMs = Math.round(performance.now() - t0);
  return c;
}
landing.capture = capture;
// The prints on hand, so a trigger never waits for a capture. The scene off
// screen is at rest (the poem finished, or the shell held), so its print is
// taken once, when the other scene is reached: the canvas shows the print of
// the scene on screen while the DOM switches under it for two frames. The
// scene on screen moves (the poem is typed), so its print is retaken every
// second the page rests, and at once when a sheet is put down.
const sheets = new Array(PHASE.length).fill(null);
// The scenes a wash can reach from here, and so the prints that must be on hand.
// A boundary whose join is not a wash carries nothing across as pixels, so it
// asks for no print — and the scenes on either side of one are not held shut
// waiting for a capture that would never be used.
const neighbours = (s) => [s - 1, s + 1].filter((n) => n >= 0 && n < PHASE.length && joinAt(s, n).wash);
// What this scene cannot do without: its own print and its washing neighbours'.
// A scene no wash ever leaves needs none at all, not even its own.
const needed = (s) => { const ns = neighbours(s); return ns.length ? [s, ...ns] : []; };
// Every scene a wash can reach, and so every print a jump may ask for. What
// holds the page shut is still `needed` — a jump is rare and can stop to take
// its own print — but the idle tick fills these, so a fling from a scene the
// reader has been resting on is one wash and not one wash behind one capture.
const washable = PHASE.map((_, s) => s).filter((s) => neighbours(s).length);
// When the shown print was replaced, and by which capture: a capture that
// started before the print now on hand was put there is stale, and a stale
// print must never take the place of a newer one.
let shownAt = 0, shownSeq = 0;
const setSheet = (scene, c) => {
  if (!c || c.__printGeneration !== printGeneration) return false;
  sheets[scene] = c; shownAt = performance.now(); shownSeq++; return true;
};
// Prints are the wash's. Reduced motion cuts and a machine without float
// render targets cuts (sim is then never created); neither needs a capture.
// A scene whose capture failed once used to cut for the rest of the session
// too (a `printable` latch, set false and never reset) -- deleted: every
// site of a capture failure now reports the fault and lets the existing
// retake loop try again, instead of disabling washes for good.
const washing = () => !!sim && !reduce;
const primed = () => !washing() || needed(shown).every((n) => sheets[n]);   // a cut needs no prints
// The article iframe nested inside a scene's own shell (the preview pane),
// if this scene has one, and its offscreen `<img>`s by the same on-screen
// test `fold` uses. Not `fold`'s own clone — the LIVE elements, because what
// forces them to load lives one layer up from the fold.
// The nested moss-preview-iframe's own navigation (kicked off by the shell's
// own `setPreview`, ui/shell.html) is independent of `__shell` becoming
// available on its parent frame — `when()` above only proves the shell's own
// script has run, not that the article it just pointed the preview at has
// finished loading. `offscreenImgs()` below reads that iframe's live
// `<img>` elements to decide what to withhold from a reveal (2ec26e2); read
// too early, before the navigation lands, it finds none and withholds
// nothing, so every one of those images (not just the ones on screen) is
// free to fetch the moment the frame is revealed. `readyState === 'complete'`
// alone doesn't prove the article has loaded — the still-blank starting
// document the iframe is created with is 'complete' too — so this also
// checks the navigation has actually landed before trusting it.
async function nestedPreviewReady(scene) {
  const frame = FRAMES[scene], outer = frame && frame.contentDocument;
  const f = outer && outer.getElementById('moss-preview-iframe');
  if (!f || !f.src) return;
  const navigated = () => f.contentDocument && f.contentDocument.readyState === 'complete' && f.contentDocument.location.href !== 'about:blank';
  if (navigated()) return;
  await capped(new Promise((resolve) => f.addEventListener('load', resolve, { once: true })), FETCH_MS, 'preview iframe load timed out').catch(() => {});
}
function offscreenImgs(scene) {
  if (scene < 1 || scene === DEPLOY) return [];
  const frame = FRAMES[scene], outer = frame && frame.contentDocument;
  const f = outer && outer.getElementById('moss-preview-iframe');
  const doc = f && f.contentDocument; if (!doc) return [];
  const [vw, vh] = docViewport(doc);
  return [...doc.querySelectorAll('img[src]')].filter((im) => !rectInFrame(vw, vh, im.getBoundingClientRect()));
}
// `sceneClasses(other)` below is the one thing that can make this preview
// iframe visible for the first time — and a same-origin iframe going from
// unseen to seen flushes every `loading=lazy` image inside it at once, not
// just the ones its own viewport shows (the earlier article had 27 plates in
// one burst, confirmed independent of `fold`'s own inlining — CDP shows the
// browser's own loader as the initiator, not this page's script). Pulling an
// offscreen image's src off for exactly the reveal-and-capture window, and
// putting it back once captured, keeps that flush from ever seeing them:
// restoring while the frame is already visible (not freshly unfrozen) lets
// native lazy loading re-evaluate them correctly, which is what the
// resource-stall-free steady state already does for the shown scene.
function withholdOffscreen(scene) {
  // A decoded image is already safe from the reveal burst and is valid print
  // input. Keep its source in place so a warm capture cannot serialize a blank
  // hole or leave the live preview waiting on restoration.
  const imgs = offscreenImgs(scene).filter((im) => !(im.complete && im.naturalWidth));
  if (!imgs.length) return () => {};
  const saved = imgs.map((im) => {
    const pic = im.parentElement && im.parentElement.tagName === 'PICTURE' ? im.parentElement : null;
    const sources = pic ? [...pic.querySelectorAll('source[srcset]')] : [];
    const entry = { im, src: im.getAttribute('src'), srcset: im.getAttribute('srcset'), sources: sources.map((s) => [s, s.getAttribute('srcset')]) };
    im.removeAttribute('src'); im.removeAttribute('srcset');
    sources.forEach((s) => s.removeAttribute('srcset'));
    return entry;
  });
  let restored = false;
  return () => {
    if (restored) return; restored = true;
    for (const { im, src, srcset, sources } of saved) {
      if (src != null) im.setAttribute('src', src);
      if (srcset != null) im.setAttribute('srcset', srcset);
      for (const [s, v] of sources) if (v != null) s.setAttribute('srcset', v);
    }
  };
}
// Which scenes is the caller's: a wash refreshes the neighbours it might run to
// next, and a jump or the idle tick asks for one the prints on hand are missing.
async function takeOthers(list = neighbours(shown)) {
  // Neighbour prints are immutable handoff state once warmed. Re-entering a
  // live scene to refresh an already-held print changes its measured shell
  // width and data-scene for several frames; that mutation is visible even
  // when the capture itself is hidden under the wash canvas. A stale neighbour
  // can safely keep its existing print until the next real wash replaces it.
  list = list.filter((other) => !sheets[other]);
  if (!list.length) return;
  const from = shown;
  let held = false;
  for (const other of list) {
    // Two frames pass before the capture, and a reader can start a wash inside
    // them: the print is then the next rest's, not this one's.
    if (running()) break;
    await nestedPreviewReady(other);
    if (running() || shown !== from) break;
    if (!held) { holdCanvas(sheets[from], sheets[from], from, true); held = true; }   // one print at both ends: the direction cannot show
    const restoreImgs = withholdOffscreen(other);
    sceneClasses(other);
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    if (running()) { restoreImgs(); break; }
    // An unprintable neighbour must not lock every transition behind primed()
    // and repeatedly switch the live stage while retrying. Use the existing
    // cut fallback for this visit, then restore the visible scene below.
    try { setSheet(other, await capture(other)); }
    // A capture failure here is this visit's, not the session's: report it
    // and stop this tick's loop (the comment above already covers why not
    // retrying immediately matters) rather than latching printable=false,
    // which used to disable every wash for good after one bad capture.
    catch (e) { reportCaptureFault(`takeOthers ${other}: ${e.message}`); break; }
    finally { restoreImgs(); }
  }
  // The reader may have crossed a boundary while the foreignObject capture was
  // drawing. Restore the scene that actually won, not the one the warmer began
  // on, or its old width and morph cover flash after the new scene has arrived.
  sceneClasses(shown);
  // The scene comes back already standing. Its furniture — the siblings, the
  // control, the fan — has not been anywhere the reader could see, and replaying
  // the transitions that bring it in is a flicker every time a print is taken.
  if (held) snapStage();
  if (held && !running()) stage.classList.remove('morphing');   // a wash that has begun meanwhile keeps the canvas up
  if (held && !running() && shown === SHIPS) layoutScene3Layers();
}
// One retake at a time. A capture slower than the tick (WebKit's foreignObject
// path, or a document that has grown) used to let the next tick start a second
// one: the two competed for the same work, each made the other slower, and they
// could finish out of order and put a stale print back on hand. A tick that
// finds one running is skipped, not queued.
let retaking = false;
// A scene reached without a wash (the harness's `landing.still`, a capture that failed
// once) leaves a print missing, and `primed()` then blocks every wash and every
// retake — including the retake that would have filled it. The tick that retakes
// fills whatever gap it finds first, the scene on screen before its neighbours.
// `booted` is what keeps the tick off the boot: until ready() has taken its own
// prints nothing is primed, and a fill that starts meanwhile is a second capture
// drawing beside the first — which is exactly what the retake flag exists to stop.
// It fills past what this scene needs, as far as every scene a wash can reach,
// because the scene a jump asks for is not one of this scene's neighbours.
let booted = false;
async function fillPrints() {
  if (!booted || retaking || running() || !washing() || !needed(shown).length) return false;   // scene 5 has no print to hold the canvas with
  // one a tick: each capture holds the canvas up over a reader who is resting
  const scene = [...needed(shown), ...washable].find((s) => !sheets[s]);
  if (scene == null) return false;
  if (mobileLayout() && scene >= SHIPS && shown < 1 && target < 1) return false;
  retaking = true;
  try {
    if (scene === shown) { const c = await capture(scene); if (scene === shown) setSheet(scene, c); }
    else await takeOthers([scene]);
  } catch (e) { console.warn('print fill skipped:', e.message); }
  finally { retaking = false; }
  return true;
}
async function retakeShown() {
  if (mobileLayout() || retaking || running() || !washing() || !needed(shown).length) return;
  retaking = true;
  const scene = shown, seq = shownSeq;
  try {
    const c = await capture(scene);
    // Not if a wash has begun meanwhile, not if a newer print landed while this
    // one drew, and not if the scene changed under it: with three scenes the
    // print of the one that was showing is not the print of the one that is.
    if (!running() && scene === shown && seq === shownSeq && c.__printGeneration === printGeneration) setSheet(scene, c);
  } catch (e) {
    console.warn('retake skipped:', e.message);   // late or failed: the print on hand is still good
  } finally { retaking = false; }
}
// Every print the page takes for itself is discretionary work, and none of it
// may run under a reader who is moving. A capture is 14 to 54ms of clone and
// serialize on the main thread (measured 2026-09-14), and `takeOthers` switches
// the DOM under the canvas besides — a reader still scrolling would see a page
// that had stopped answering. So the warmer waits for a real rest — the scroll
// still for IDLE_REST and nothing running — and then the browser's own idle
// callback picks the frame (WebKit shipped one late, hence the fallback). One
// piece of work a tick, so no rest pays for two captures at once. A join owed
// but held shut for want of a print is not a reason to wait: filling that print
// is the only thing that can unblock it.
const IDLE_REST = 180, WARM_MS = 1000, IDLE_WAIT = 300;
const whenIdle = window.requestIdleCallback ? (fn) => requestIdleCallback(fn, { timeout: IDLE_WAIT }) : (fn) => setTimeout(fn, 1);
const atRest = () => !running() && !document.hidden && performance.now() - restSince >= IDLE_REST;
// The prints a wash has just left behind. The scene it came from was being
// typed into a moment before, so the print taken of it is no longer what the
// live scene shows — it stays on hand, because a wash back is better run onto a
// stale print than cut, and it is retaken at the next rest.
const stale = new Set();
const wentStale = () => { for (const n of neighbours(shown)) stale.add(n); warmSoon(0); };
async function refreshStale() {
  if (retaking || running() || !washing()) return false;
  const scene = [...stale][0];
  if (scene == null) return false;
  stale.delete(scene);
  retaking = true;
  try { await takeOthers([scene]); } catch (e) { console.warn('stale print skipped:', e.message); }
  finally { retaking = false; }
  return true;
}
let warmTimer = null;
function warmSoon(ms = WARM_MS) { if (warmTimer != null) return; warmTimer = setTimeout(() => { warmTimer = null; whenIdle(warm); }, ms); }
async function warm() {
  if (document.documentElement.dataset.static) return;
  warmSoon();            // the next tick is a second from this one's start, not from its end
  if (!atRest() && !(target !== shown && !primed())) return;
  stale.delete(shown);   // the scene on screen has its own retake
  // One thing a tick, in this order: a print a jump may ask for that nobody has
  // taken, then one a wash has just left behind, then the scene on screen, whose
  // print goes off because the scene moves.
  if (!await fillPrints() && !await refreshStale()) await retakeShown();
  if (atRest()) warmDissolve();
}
// A dissolve a leg from here will need, recorded while nothing moves: one
// print a tick. Not the desktop scene on screen, whose print is retaken every
// second at rest -- its record is made as its leg starts, from the print the
// leg actually carries.
function warmDissolve() {
  // Not beside a print being taken: a record holds the main thread for its
  // whole length (seconds on a software GPU), and a raster waiting on it
  // times out and leaves its scene without a print (seen at boot, 2026-09-23).
  if (!morph || running() || !booted || inFlight) return;
  morph.retain(needed(shown).map((scene) => sheets[scene]));
  for (const scene of needed(shown)) {
    if (!mobileLayout() && scene === shown) continue;
    const spent = morph.warm(sheets[scene]);
    if (spent) { recordSteps += spent; return; }
  }
}
warmSoon();
function setTarget(t) {
  if (t === SHARE && xfAt() >= 1) {
    target = t;
    // Closing copy and its poster are independent of the active join.
    mountLoop(); setCross(1); fiveOn(true);
    if (!running() && shown !== SHARE) standAtTerminalClose();
    return;
  }
  if (t === target) return;
  target = t;
}
// The closing scene has no captured print or embedded app dependency. A cold
// load can reach the document bottom while those earlier scenes are still
// loading; let the terminal position stand immediately instead of queuing it
// behind prints the reader has already passed. If they reverse before boot,
// onScroll keeps the new target and ready() drives the ordinary joins back.
function standAtTerminalClose() {
  shown = target = SHARE;
  // mob.scene is showMobileScene's own dedup key; left at DEPLOY here it
  // strands shown at SHARE forever on a mobile reversal, since renderMorphAt
  // then finds mob.scene already equal to the `to` it would show and no-ops.
  mob.scene = SHARE;
  still(SHARE);
  mountLoop();
  setCross(1);
  fiveOn(true);
}
// Scroll: the reading line is mid-window, and where it stands is what names the
// scene. A wash then runs at its own pace and sets where it is; the scroll only
// works it.
const LINE = 0.5;
const textTop = (sec) => sec.firstElementChild.getBoundingClientRect().top;
const textBottom = (sec) => sec.lastElementChild.getBoundingClientRect().bottom;
// Where the reader is, read whole rather than counted up one crossing at a
// time. The line standing inside a scene's text names that scene. Between two
// texts it names neither, and what it names there is the scene being travelled
// toward: that is what hands the wash the whole gap between the texts to run
// across, and what a turnaround reads to send it back. Because this is a
// position and not a tally, a fling that clears three texts names the scene it
// landed on from its first frame, and the wash in flight is re-pointed at it.
// The pair-of-edges crossing detector this replaces could only ever step one
// boundary at a time, so scene 1 to scene 3 played both joins in sequence and
// the reader watched the page morph through scene 2 to get there.
// One number: the scene whose text the reading line stands in, or a fraction
// between two scenes when it stands in the gap between their texts. Everything
// that asks which scene a position belongs to reads it here — the target the
// wash follows, and the scene a rest is carried to — so the two cannot come to
// disagree as either scene's markup changes.
const progressAt = () => {
  if (mobileLayout()) {
    const entrance = mobileEntranceProgress();
    if (entrance < 1) return entrance;
    for (let scene = 1; scene < DEPLOY; scene++) {
      const q = mobileInkProgress(scenesEl[scene].firstElementChild, landing.mobileEarlyBy(scene));
      if (q < 1) return scene + q;
    }
    return DEPLOY + mobileClosingProgress();
  }

  const line = innerHeight * LINE;
  for (let i = 0; i < JOINS.length; i++) {
    if (textBottom(scenesEl[i]) >= line) return i;   // the line is still inside this text
    // The last join is the crossfade, not a wash: its own progress is the
    // scrub the visual paints from -- #five's own band against scrollY --
    // not a text-flow gap. scenesEl[DEPLOY + 1] is a bare 1px marker with no
    // crossfade geometry of its own (closing-progress unit, unit 3,
    // review-phases-2-4.md Job 2 item 5). restY(SHARE) sits at or past
    // where this plateaus at DEPLOY + 1, its far end.
    if (i === DEPLOY) return DEPLOY + clamp01((scrollY - (five.offsetTop - innerHeight * .8)) / (innerHeight * .6));
    const top = textTop(scenesEl[i + 1]);
    if (top >= line) { const bot = textBottom(scenesEl[i]); return i + (line - bot) / Math.max(1, top - bot); }
  }
};
// In the gap the position names neither scene, and what it names there is the
// one being travelled toward. Same zero-is-not-downward rule as
// sceneForRest: dir is 0 before the reader's first gesture, which a
// scroll-restored reload reaches with the position already mid-gap, so the
// ceil branch below must not stand in for "no direction yet".
const targetAt = (dir) => {
  const p = progressAt();
  // Pixel rounding at a rest must not start a reverse wash by a fraction.
  if (Math.abs(p - Math.round(p)) < .002) return Math.round(p);
  // Keep the same pair of prints while a finger reverses within their wash.
  if (mobileLayout()) return p < shown ? Math.floor(p) : Math.ceil(p);
  return dir === 0 ? Math.round(p) : dir < 0 ? Math.floor(p) : Math.ceil(p);
};
let scrollV = 0, asked = null, restSince = performance.now();
// warmScene3Media (defined with the rest of scene 3's own script, below)
// needs this declared up here, ahead of watchScroll's own IIFE (further
// down): that IIFE's first call is synchronous, part of this same top-level
// script's own run, and on a reload that restores scrollY past scene 1 it
// reaches onScroll's `t >= SHIPS - 1` check below before the script has gone
// anywhere near warmScene3Media's own definition — a `let` declared down
// there would throw "before initialization" on that very first tick, an
// uncaught exception that stops the rest of this script cold (`landing.state`
// and everything after it would never be defined). A `let` this early has no
// such window.
let s3MediaWarmed = false;
function onScroll(dy) {
  // scroll speed in window heights per second, a leaky integrator of the
  // displacement (time constant V_TAU, decayed by the frame): what agitates
  // and tilts the film
  scrollV += dy / innerHeight / V_TAU;
  if (dy) restSince = performance.now();
  const t = targetAt(Math.sign(dy));
  if (!booted && shown === SHARE) {
    const q = xfAt();
    asked = target = t;
    setCross(q);
    fiveOn(q >= 1);
    return;
  }
  if (t === SHARE && xfAt() >= 1) {
    asked = t; setTarget(SHARE);
    return;
  }
  // Scene 3's own heavy media (warmScene3Media, defined below) starts here:
  // `t` is read off the real scroll position every frame no matter how the
  // reader got here, so this is the earliest the page ever knows they are
  // headed for scene 3 or beyond.
  if (t >= SHIPS - 1) warmScene3Media();
  // The position asks for a scene when the scene it names changes, so a scene
  // set by another hand — the harness's `landing.still`, a restored position — is
  // left standing until the reader moves. And nothing may fire before the first
  // prints are on hand: `ready` starts the first join.
  if (t === asked) return;
  asked = t;
  if (booted) setTarget(t); else target = t;
}
// The sheets are loose: while the editor is on screen a plate can be dragged
// anywhere plateBounds() allows, and the print rectangle expands on drop when
// it leaves the resting domain. The wash then takes the live layout from that
// expanded print, using the same single watercolor engine.
{
  let held = null;
  stage.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return;
    const pl = e.target.closest('.plate'); if (!pl || running() || shown !== 0) return;
    held = { pl, x: e.clientX, y: e.clientY, left: pl.offsetLeft, top: pl.offsetTop };
    pl.classList.add('held'); pl.setPointerCapture(e.pointerId); e.preventDefault();
  });
  stage.addEventListener('pointermove', (e) => {
    if (!held) return;
    const b = plateBounds(), w = held.pl.offsetWidth, h = held.pl.offsetHeight;
    const x = clampTo(held.left + (e.clientX - held.x) / SCALE, b.xMin, Math.max(b.xMin, b.xMax - w));
    const y = clampTo(held.top + (e.clientY - held.y) / SCALE, b.yMin, Math.max(b.yMin, b.yMax - h));
    held.pl.style.left = x + 'px'; held.pl.style.top = y + 'px';
  });
  const drop = () => {
    if (!held) return;
    const { pl } = held;
    pl.classList.remove('held'); held = null;
    const outsideBase = pl.offsetLeft < -GEOM.pad || pl.offsetTop < -GEOM.pad ||
      pl.offsetLeft + pl.offsetWidth > GEOM.cellW + GEOM.pad ||
      pl.offsetTop + pl.offsetHeight > GEOM.cellH + GEOM.pad;
    if (outsideBase) { printExpanded = true; setPrintRect(currentPrintRect()); }
    retakeShown();
  };
  stage.addEventListener('pointerup', drop); stage.addEventListener('pointercancel', drop);
}
// Read the position every frame rather than listening for scroll events: the
// events arrive a frame late at best and were seen not to arrive at all after a
// long frame, and a comparison per frame costs nothing.
let lastScrollY = null;
let lastWatchT = performance.now();
// Wherever the reader comes to rest, the page finishes: the scene the reader
// was carrying themselves toward (or, inside DEAD, the one just left), the
// wash run out to it, and the copy carried onto that scene's designed position.
// Not CSS scroll snap — that re-snaps after every scroll, a deliberate small one
// included, and knows nothing of the wash it would have to land with. And not
// the catch window this replaces either: that fired only on a coast dying
// within a third of a window of a designed position, so a reader who simply
// stopped between two texts was left with the film wet and the print smeared,
// waiting on a creep that takes thirteen seconds to reach even the hold it was
// creeping to (measured 2026-09-14). A stop is not a state the page can hold.
// The inverse of progressAt(): the scroll position at the centre of the same
// plateau it reads as this scene — textTop() to textBottom(), the identical
// pair it names the scene from — so a settle and the function judging where
// it landed can never disagree by construction, not just by measurement.
// Frame the title and signup together; the footer remains a natural scroll away.
function closingRestY() {
  const max = Math.max(0, document.documentElement.scrollHeight - innerHeight);
  if (mobileLayout()) return max;
  const top = scrollY + document.querySelector('#five h2').getBoundingClientRect().top;
  const bottom = scrollY + document.querySelector('#beta .input-row').getBoundingClientRect().bottom;
  const centered = (top + bottom - innerHeight) / 2;
  const withFormVisible = Math.max(centered, bottom - innerHeight + 56);
  return Math.round(Math.max(0, Math.min(max, withFormVisible)));
}
const restY = (scene) => {
  if (scene < 0) return 0;
  if (scene === 0 && mobileLayout()) return Math.max(0, Math.round(scrollY + textTop(scenesEl[0]) - 88));
  if (scene === SHARE) return closingRestY();
  if (mobileLayout()) {
    const band = mobileVisualBand();
    return Math.round(scrollY + (scene === 1 ? textTop(scenesEl[1]) - band.bottom - 24 : textBottom(scenesEl[scene - 1]) - band.top));
  }
  const top = textTop(scenesEl[scene]), bot = textBottom(scenesEl[scene]);
  return Math.round(scrollY + (top + bot) / 2 - innerHeight * LINE);
};
// Tell native proximity snap where the reading wells are. The margin is
// derived from the same geometry as progressAt/restY, so a snap cannot land
// at a position the presenter would call a different scene.
function setupCssSnap() {
  if (mobileLayout()) return;
  for (const [i, el] of scenesEl.entries()) {
    el.style.scrollMarginTop = `${el.getBoundingClientRect().top + scrollY - restY(i)}px`;
  }
  five.style.scrollMarginTop = `${five.getBoundingClientRect().top + scrollY - closingRestY()}px`;
}
addEventListener('resize', setupCssSnap);
setupCssSnap();
// The browser owns motion here; settleAtRest is never called. On mobile this is a strictly
// observational path: native scrolling supplies scrollY, which drives the scenes and closing
// scrub continuously, and no release may move the page after the reader lets go.
let mobileWatchKey = '', finalDissolve = 0, finalWash = null, finalPrints = null;
// The viewport target begins taking shape while scene 4 is still in contact;
// the shared .9s cure would leave the first 43% of this closing wash clear.
// This changes only its display onset; the shared pigment clock is unchanged.
const CLOSING_CURE = T_CURE * .2;
let closingWashCanvas = document.createElement('canvas');
closingWashCanvas.id = 'closing-wash';
document.querySelector('.page').appendChild(closingWashCanvas);
const closingWashRect = () => ({ x: 0, y: 0, w: innerWidth, h: innerHeight });
let closingWashSim = null, closingWashSize = '';
async function armClosingWash() {
  if (reduce) return;
  while (document.documentElement.dataset.ready !== '1') {
    if (document.documentElement.dataset.static) return;
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
  // Construct once readiness releases us. Waiting for another at-rest idle
  // window strands a fast reader at the scene-4 contact before this small
  // viewport sim exists; readiness already kept the work off the boot path.
  const size = `${innerWidth}x${innerHeight}`;
  if (closingWashSim && closingWashSize === size) return;
  if (closingWashSim) {
    if (finalWash) cancelAnimationFrame(finalWash.raf);
    finalWash = null; finalPrints = null;
    closingWashCanvas.getContext('webgl2')?.getExtension('WEBGL_lose_context')?.loseContext();
    closingWashCanvas.remove();
    closingWashCanvas = document.createElement('canvas');
    closingWashCanvas.id = 'closing-wash';
    document.querySelector('.page').appendChild(closingWashCanvas);
  }
  closingWashSim = makeSim({
    canvas: closingWashCanvas,
    texW: Math.max(1, Math.round(innerWidth / 4)),
    texH: Math.max(1, Math.round(innerHeight / 4)),
    rect: closingWashRect,
    paperScale: .5,
  });
  closingWashSize = closingWashSim ? size : '';
  if (closingWashSim) updateFinalDissolve();
}
armClosingWash();
let closingWashResizeTimer = null;
addEventListener('resize', () => {
  clearTimeout(closingWashResizeTimer);
  closingWashResizeTimer = setTimeout(armClosingWash, 200);
});
// Scene 4 as pixels: with `control` the grown control where it stands, and
// with `logos` the targets at their exact current physics positions. The
// closing wash dissolves both. A leg to scene 3 carries the targets alone,
// since the control stays solid above it (publishBridge) and its ink run out
// from under it would be a stain spreading from a button that does not
// dissolve (measured: a dark blot by p = 0.05); a leg arriving from scene 3
// carries the control alone, for the wash to gather into, while the targets
// pop out of it as the scene's own performance.
function deployPrint({ logos = true, control = true } = {}) {
  const copy = document.createElement('canvas'); copy.width = printW(); copy.height = printH();
  const g = copy.getContext('2d'), ox = -printRect.x, oy = -printRect.y;
  for (let i = 0; logos && i < (orbitNodes?.length || 0); i++) {
    const n = orbitNodes[i], el = orbitEls[i], im = el.querySelector('img');
    const radius = ORBIT_D * Math.max(0, popScale(n, performance.now())) / 2;
    if (!n.popAt || !radius) continue;
    g.save(); g.beginPath(); g.arc(n.x + ox, n.y + oy, radius, 0, Math.PI * 2); g.clip();
    g.fillStyle = getComputedStyle(el).backgroundColor; g.fillRect(n.x + ox - radius, n.y + oy - radius, radius * 2, radius * 2);
    const fb = el.querySelector('.orbit-fallback');
    if (im?.complete && im.naturalWidth) {
      const k = Math.min(radius * 1.24 / im.naturalWidth, radius * 1.24 / im.naturalHeight);
      g.drawImage(im, n.x + ox - im.naturalWidth * k / 2, n.y + oy - im.naturalHeight * k / 2, im.naturalWidth * k, im.naturalHeight * k);
    } else if (fb) {
      // a target with no mark shows its name; the print shows it too
      const fs = getComputedStyle(fb);
      g.fillStyle = fs.color; g.textAlign = 'center'; g.textBaseline = 'middle';
      g.font = `${fs.fontWeight} ${parseFloat(fs.fontSize) * radius * 2 / ORBIT_D}px ${fs.fontFamily}`;
      g.fillText(fb.textContent, n.x + ox, n.y + oy);
    }
    g.restore();
  }
  const source = control && sheets[DEPLOY];
  if (source) {
    const cs = getComputedStyle(stage), cx = parseFloat(cs.getPropertyValue('--pub-cx')), cy = parseFloat(cs.getPropertyValue('--pub-cy'));
    const k = source.width / printW(), r = pubR, diameter = r * 2 * PUB_SCALE;
    g.drawImage(source, (-32 + cx - r + ox) * k, (cy - r + oy) * k, r * 2 * k, r * 2 * k,
      GEOM.cellW / 2 + ox - diameter / 2, GEOM.cellH / 2 + oy - diameter / 2, diameter, diameter);
  }
  return copy;
}
function closingPigmentPrint() {
  const sheet = document.createElement('canvas'); sheet.width = innerWidth; sheet.height = innerHeight;
  const g = sheet.getContext('2d');
  if (closingPoster.complete && closingPoster.naturalWidth) {
    const k = Math.max(sheet.width / closingPoster.naturalWidth, sheet.height / closingPoster.naturalHeight);
    g.drawImage(closingPoster, (sheet.width - closingPoster.naturalWidth * k) / 2, (sheet.height - closingPoster.naturalHeight * k) / 2, closingPoster.naturalWidth * k, closingPoster.naturalHeight * k);
  } else { g.fillStyle = '#777777'; g.fillRect(0, 0, sheet.width, sheet.height); }
  g.globalCompositeOperation = 'destination-in';
  const gradient = g.createRadialGradient(sheet.width / 2, sheet.height / 2, 35, sheet.width / 2, sheet.height / 2, Math.hypot(sheet.width, sheet.height) * .55);
  gradient.addColorStop(0, '#000'); gradient.addColorStop(.55, '#000b'); gradient.addColorStop(1, '#0000');
  g.fillStyle = gradient; g.fillRect(0, 0, sheet.width, sheet.height);
  return sheet;
}
function closingSourcePrint() {
  const sheet = document.createElement('canvas'); sheet.width = innerWidth; sheet.height = innerHeight;
  const r = canvas.getBoundingClientRect(), control = cell.getBoundingClientRect(), g = sheet.getContext('2d');
  // Scene 4's moving targets retain their current geometry; its stable ink
  // source is the centred publish disc. Cached sheets may have handed these
  // members back to the live DOM, so read both explicitly here.
  g.drawImage(deployPrint({ control: false }), r.left, r.top, r.width, r.height);
  g.fillStyle = '#28251f'; g.beginPath();
  g.arc(control.left + control.width / 2, control.top + control.height / 2, buttonR() * SCALE, 0, Math.PI * 2);
  g.fill();
  return sheet;
}
function updateFinalDissolve() {
  if (!closingWashSim || reduce) return;
  const q = mobileLayout() ? mobileClosingProgress() : xfAt();
  const active = q > 0;
  finalDissolve = q;
  if (!q) {
    if (finalWash) {
      cancelAnimationFrame(finalWash.raf); finalWash = null;
      // Not releasePigmentCover(): on mobile, 'mobile-handoff' and --wash-cover
      // are also mob's (renderMorphAt) for as long as the reader is anywhere
      // at or past SHIPS -- mob mounts that class once and never remounts to
      // re-add it, so removing it here left #gl with no matching rule and its
      // opacity fell back to CSS's un-set default (1) instead of the 0 the
      // wash had already reached: the up-leg-off-DEPLOY flip in
      // check-landing-monotone.mjs. Writing the property itself to 0 reaches
      // the same invisible result without touching a class mob still owns.
      closingWashCanvas.style.display = 'none'; closingWashCanvas.style.filter = '';
      stage.style.setProperty('--wash-cover', '0'); canvas.style.filter = '';
      fanEl.classList.remove(WatercolorMorph.MEMBER);
      if (shown === DEPLOY) orbitSim?.restart();
    }
    return;
  }
  if (!finalWash) {
    if (!active) return;
    orbitSim?.stop();
    // Retain these exact pixels through the final scene and the return trip.
    if (!finalPrints || finalPrints.w !== innerWidth || finalPrints.h !== innerHeight) {
      finalPrints = { source: closingSourcePrint(), film: closingPigmentPrint(), w: innerWidth, h: innerHeight };
    }
    closingWashSim.setPrints(finalPrints.source, finalPrints.film);
    closingWashSim.reset();
    closingWashSim.draw(true, 0);
    closingWashCanvas.style.display = 'block';
    finalWash = { t: 0, drawn: -1, goal: q * T_TOTAL, raf: 0, covered: true, shownQ: -1 };
  }
  const wash = finalWash; wash.goal = q * T_TOTAL;
  // Cover and the grayscale switch are q's alone (unit7 part A's rule: what
  // is presented is a pure function of scroll position). A wash recreated
  // at t=0 toward an already-large goal must read as that goal on this same
  // frame, not wait however many frames advanceWash's step budget takes to
  // close the gap -- that wait was the mobile reverse-leg bug: cover pinned
  // at 0 while the sim caught up, then jumping straight to ~1.
  if (q !== wash.shownQ) {
    closingWashCanvas.style.opacity = String(1 - smooth(.72, 1, q));
    // The targets are in this print (deployPrint): members, hidden once the
    // canvas fully covers them and not before, since this cover fades in.
    fanEl.classList.toggle(WatercolorMorph.MEMBER, q >= .18);
    closingWashCanvas.style.filter = `grayscale(${smooth(.1, .65, q)})`;
    wash.shownQ = q;
  }
  if (wash.raf) return;
  const frame = () => {
    if (finalWash !== wash) return;
    wash.raf = 0;
    // The sim itself may still lag or rebuild behind q -- it is a
    // simulation, not the presenter -- so only the actual paint waits on it.
    // Reverse scroll reconstructs the same forward field at the earlier
    // position. One clock owns both directions, without competing fade loops.
    const result = advanceWash(closingWashSim, wash, { goal: wash.goal, fwd: true }, t => {
      closingWashSim.draw(true, smooth(CLOSING_CURE, T_TOTAL, t), -.2 + 1.4 * smooth(.94, 1, wash.goal / T_TOTAL));
    });
    if (!result.caughtUp) wash.raf = requestAnimationFrame(frame);
  };
  wash.raf = requestAnimationFrame(frame);
}

function watchScrollNative() {
  const mobile = mobileLayout();
  const key = `${scrollY}:${innerWidth}:${innerHeight}`;
  const scrolled = key !== mobileWatchKey;
  // Reaching the target still needs frames after the scroll that named it
  // stops: the pigment clock can still be behind p after scrollY stops.
  if (booted && (scrolled || !mob.settled)) renderMorphAt(progressAt());
  if (!scrolled) return;
  mobileWatchKey = key;
  const dy = lastScrollY == null ? 0 : scrollY - lastScrollY;
  lastScrollY = scrollY;
  // Mobile retains the target bookkeeping used by its touch/capture path.
  // Desktop has one authority: renderMorphAt(progressAt()) above. Calling
  // setTarget here would start the deleted clock-driven presenter beside it.
  if (mobile) onScroll(dy);
  else {
    scrollV += dy / innerHeight / V_TAU;
    if (progressAt() >= SHIPS - 1) warmScene3Media();
  }
  // Native scroll owns the closing crossfade too. Keep the film opacity on
  // the same geometry-derived position as the final wash on every frame;
  // otherwise --xf remains at its previous rest until the terminal cut.
  setCross(xfAt());
  updateFinalDissolve();
  const now = performance.now();
  const dt = Math.min(0.1, Math.max(0, (now - lastWatchT) / 1000));
  lastWatchT = now;
  scrollV *= Math.exp(-dt / V_TAU);
}
(function watchScroll() {
  if (document.documentElement.dataset.static) return;
  updateOpening();
  watchScrollNative();
  requestAnimationFrame(watchScroll);
})();

// ── Scene 1: a poem being written ─────────────────────────────────────────
// Each scene change is a generation; a loop runs while its own generation is
// current, so a return to the same scene starts a fresh loop and the old one
// falls out at its next wait.
let gen = 0;
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
async function writeLoop(afterWash) {
  const g = gen;
  if (afterWash) await new Promise((r) => setTimeout(r, 1800));   // the poem the wash returned stays to be read
  while (gen === g) {
    editorPlayback(true);
    ed.setDoc(SEED);
    await new Promise((r) => setTimeout(r, 700));
    if (gen !== g) break;
    const done = await ed.type(TYPED, { seed: 11, median: window.__LANG === 'zh' ? 95 : 18, slipAt: 47 });
    editorPlayback(false);
    if (!done) break;
    await new Promise((r) => setTimeout(r, 2600));   // the poet reads it back
  }
}

// ── Scene 2: the site keeps up ────────────────────────────────────────────
// The hairline breathes: each breath is one rebuild, slow inhale to full,
// then the dissolve. Each breath lands one more change on the publish ring,
// which grows into it: an edit, a new page, more edits, a stylesheet change
// that restyles every page (the outer orbit), a deletion. Then the author
// publishes, the ring clears, and the afternoon starts over.
const EVENTS = [
  { edited: 1 },
  { edited: 1, added: 1 },
  { edited: 2, added: 1 },
  { edited: 2, added: 2 },
  { edited: 3, added: 2, restyled: 14 },
  { edited: 4, added: 2, deleted: 1, restyled: 14 },
  { edited: 5, added: 3, deleted: 1, restyled: 14 },
];
async function liveLoop() {
  const g = gen;
  while (gen === g) {
    sh.setPending({});
    await wait(900);
    for (const ev of EVENTS) {
      if (gen !== g) break;
      const breathed = await sh.breathe(2200, 700);
      if (!breathed || gen !== g) break;
      await sh.growTo(ev, 700);
    }
    if (gen !== g) break;
    await wait(1200);
    sh.publishFlash(); await wait(2200);
  }
}

// Scene 3 keeps genuine app chrome above the artifacts and website paper
// around a fixed article. Only the three artifacts change position.
const NB_SRC = 'scene3/notebook/breed/mandelbrot-live.html';
const S3_ARTICLE = '/scene3/article/';
const S3_VIDEO = '/scene3/movie/pool/clips/general-railroad-ties.mp4';
const CARDS = {
  sk: { docked: false, el: $('sib-sk'), label: 'the algorithmic artwork — drag anywhere' },
  nb: { docked: false, el: $('sib-nb'), label: 'the notebook, an IPython zoom — drag to move' },
  video: { el: $('s3-video'), label: 'the video itself — drag to move', docked: true },
};
// Warm the local documents only as Scene 3 approaches.
function warmScene3Media() {
  // watchScroll's own first tick runs synchronously, before the Promise.all
  // below has assigned `vd` — reachable when a reload restores scrollY past
  // scene 1, same as the TDZ case this shares a comment with. Bail without
  // marking it warmed: the very next tick, a real animation frame away, runs
  // after that Promise has settled and retries for real.
  if (s3MediaWarmed || !vd) return;
  s3MediaWarmed = true;
  $('sk').src = $('sk').dataset.src;
  $('nb').src = NB_SRC;
  vd.setPreview(S3_ARTICLE);
  $('s3-video-el').poster = $('s3-video-el').dataset.poster;
  $('s3-video-el').src = S3_VIDEO;
}
// The detached video and the copy inside the assembled preview share playback
// state: only Scene 3 runs them, and reduced motion starts both paused.
function videoActive(on) {
  let nestedVideo = null;
  try { nestedVideo = vdFrame.contentDocument.getElementById('moss-preview-iframe').contentDocument.querySelector('video'); } catch (e) {}
  for (const media of [nestedVideo, $('s3-video-el')]) {
    if (!media) continue;
    if (on && !document.hidden && !reduce) media.play().catch(() => {}); else media.pause();
  }
}
document.addEventListener('visibilitychange', () => {
  syncLoop();
  videoActive(shown === SHIPS);
  sketchVisible(!document.hidden && shown === SHIPS);
});

// Measure docking inside the real preview; the window remains a single layer.
function layoutScene3Layers() {
  try {
    const shellDoc = vdFrame.contentDocument;
    const nested = shellDoc.getElementById('moss-preview-iframe');
    if (!nested || !nested.contentDocument) return false;
    const sourceVideo = nested.contentDocument && nested.contentDocument.querySelector('video');
    if (!sourceVideo) return false;
    const nr = nested.getBoundingClientRect(), vr = sourceVideo.getBoundingClientRect();
    s3Paper = { x: box.offsetLeft + nr.left, y: box.offsetTop + nr.top, w: nr.width, h: nr.height };
    s3Slot = { x: s3Paper.x + vr.left, y: s3Paper.y + vr.top, w: vr.width, h: vr.height };
    CARDS.video.height = CARDS.video.width * vr.height / vr.width;
    for (const id of S3_ORDER) {
      const c = CARDS[id];
      if (c.pose && c.docked && s3Drag?.id !== id) { stopCard(id); c.pose = dockPose(id); paintCard(id); }
    }
    // The shell and article remain one intact preview. Only its original
    // media is hidden: the movable artifacts paint above the whole window.
    const article = nested.contentDocument;
    let hide = article.getElementById('s3-hide-video');
    if (!hide) {
      hide = article.createElement('style'); hide.id = 's3-hide-video';
      hide.textContent = '.moss-ambient-video{visibility:hidden!important}';
      article.head.appendChild(hide);
    }
    nested.style.visibility = '';
    stage.classList.add('s3-ready');
    return true;
  } catch (e) { return false; }
}
function showAssembledPreview() {
  stage.classList.remove('s3-ready');
  try { const d = vdFrame.contentDocument; d.documentElement.classList.remove('s3-aperture'); d.getElementById('moss-preview-iframe').style.visibility = ''; } catch (e) {}
}
function armScene3Layers() {
  let tries = 0;
  const attempt = () => {
    if (shown !== SHIPS || layoutScene3Layers() || tries++ > 120) return;
    requestAnimationFrame(attempt);
  };
  attempt();
}
// The sketch's own canvas already pauses when its IntersectionObserver says
// its canvas is out of view (scene3/sketch/native.html?id=processing-03) — which never fires
// on its own here, since the canvas fills its iframe's whole viewport
// whatever the parent page is doing. This is the signal that observer is
// missing: whether scene 3 itself, the one place the sketch is ever drawn,
// is actually the scene on screen.
function sketchVisible(on) {
  for (const id of ['sk', 'nb']) {
    const w = $(id).contentWindow;
    try { w && w.postMessage({ type: 'moss-visible', visible: on && !document.hidden }, location.origin); } catch (e) {}
  }
}

// ── Scene 3: one pose per artifact, in the scaled cell's coordinates ────
// Scaling the complete artifact preserves the harvested UI's proportions.
Object.assign(CARDS.nb, { width: 520, height: 520 * 560 / 670 });
Object.assign(CARDS.sk, { width: 440, height: 440 });
Object.assign(CARDS.video, { width: 520, height: 390 });
let s3Drag = null, s3HardStop = true, s3Touched = false, s3SuppressClick = false;
let s3Paper = { x: 120, y: 78, w: 540, h: 512 };
let s3Slot = { x: 180, y: 290, w: 380, h: 285 };
let s3Stack = ['video'], s3Initialized = false;
const s3Jitter = [Math.random() - .5, Math.random() - .5, Math.random() - .5, Math.random() - .5];
const cardId = el => S3_ORDER.find(id => CARDS[id].el === el);
const localPoint = e => { const r = cell.getBoundingClientRect(); return { x: (e.clientX - r.left) / SCALE, y: (e.clientY - r.top) / SCALE }; };
const inPreview = p => p.x >= s3Paper.x + 16 && p.x <= s3Paper.x + s3Paper.w - 16 && p.y >= s3Paper.y + 64 && p.y <= s3Paper.y + s3Paper.h - 12;
function dockPose(id) {
  const c = CARDS[id], scale = s3Slot.w / c.width;
  return { x: s3Slot.x, y: s3Slot.y, s: scale };
}
function cardDepths() {
  for (const id of S3_ORDER) {
    const c = CARDS[id], focused = s3Drag?.id === id || c.el.matches(':focus-visible');
    // Outside creations sit behind the preview window. A held or focused card
    // rises above it, while docked cards use the same stack order as the paper.
    c.el.style.zIndex = String(focused ? 20 : c.docked ? 12 + Math.max(0, s3Stack.indexOf(id)) : 1 + S3_ORDER.indexOf(id));
    c.el.dataset.placement = c.docked ? 'preview' : 'outside';
  }
}
function paintCard(id) {
  const c = CARDS[id], p = c.pose;
  c.el.style.left = c.el.style.top = '0px';
  c.el.style.width = c.width + 'px'; c.el.style.height = c.height + 'px';
  c.el.style.transform = `translate(${p.x.toFixed(2)}px,${p.y.toFixed(2)}px) scale(${p.s.toFixed(5)})`;
  if (c.docked && !s3Drag) {
    const left = Math.max(0, (s3Paper.x - p.x) / p.s), top = Math.max(0, (s3Paper.y - p.y) / p.s);
    const right = Math.max(0, (p.x + c.width * p.s - s3Paper.x - s3Paper.w) / p.s);
    const bottom = Math.max(0, (p.y + c.height * p.s - s3Paper.y - s3Paper.h) / p.s);
    c.el.style.clipPath = `inset(${top}px ${right}px ${bottom}px ${left}px round 3px)`;
  } else c.el.style.clipPath = '';
}
function stopCard(id) { cancelAnimationFrame(CARDS[id].raf); CARDS[id].raf = 0; }
function settleCard(id, target) {
  const c = CARDS[id]; stopCard(id);
  if (reduce) { c.pose = { ...target }; paintCard(id); return; }
  const v = { x: 0, y: 0, s: 0 }; let last = performance.now();
  const frame = now => {
    c.raf = 0; if (s3HardStop || s3Drag?.id === id) return;
    const dt = Math.min(.05, (now - last) / 1000); last = now;
    for (const key of ['x', 'y', 's']) { let d; [d, v[key]] = springStep(c.pose[key] - target[key], v[key], dt, CARD_SETTLE_K); c.pose[key] = target[key] + d; }
    const done = Math.abs(c.pose.x - target.x) < .2 && Math.abs(c.pose.y - target.y) < .2 && Math.abs(c.pose.s - target.s) < .0005;
    if (done) c.pose = { ...target };
    paintCard(id); if (!done) c.raf = requestAnimationFrame(frame);
  };
  c.raf = requestAnimationFrame(frame);
}
function updateDrag(now) {
  const d = s3Drag; if (!d || s3HardStop) return;
  const c = CARDS[d.id], dt = Math.min(.05, Math.max(0, (now - d.last) / 1000)); d.last = now;
  const targetScale = d.inside ? dockPose(d.id).s : 1;
  let delta; [delta, d.vs] = springStep(c.pose.s - targetScale, d.vs, dt, 260);
  c.pose.s = reduce ? targetScale : targetScale + delta;
  c.pose.x = d.point.x - d.grab.x * c.pose.s; c.pose.y = d.point.y - d.grab.y * c.pose.s;
  paintCard(d.id); c.raf = requestAnimationFrame(updateDrag);
}
stage.addEventListener('pointerdown', e => {
  if (shown !== SHIPS || phase === 'morph' || s3Drag || e.button !== 0) return;
  const el = e.target.closest('[data-asset]'), id = el && cardId(el); if (!id) return;
  const c = CARDS[id], point = localPoint(e); stopCard(id); s3Touched = true;
  s3Drag = { id, el, point, startX: e.clientX, startY: e.clientY, inside: inPreview(point), grab: { x: (point.x - c.pose.x) / c.pose.s, y: (point.y - c.pose.y) / c.pose.s }, origin: { ...c.pose }, wasDocked: c.docked, last: performance.now(), vs: 0, pointerId: e.pointerId };
  el.classList.add('s3-drag'); el.focus({ preventScroll: true }); cardDepths();
  el.setPointerCapture(e.pointerId); e.preventDefault(); c.raf = requestAnimationFrame(updateDrag);
});
stage.addEventListener('pointermove', e => {
  if (!s3Drag) return;
  const dx = e.clientX - s3Drag.startX, dy = e.clientY - s3Drag.startY;
  if (dx * dx + dy * dy > 16) s3SuppressClick = true;
  s3Drag.point = localPoint(e); s3Drag.inside = inPreview(s3Drag.point);
});
function endDrag(cancelled = false) {
  if (!s3Drag) return;
  const d = s3Drag, c = CARDS[d.id]; stopCard(d.id); s3Drag = null;
  d.el.classList.remove('s3-drag'); d.el.blur();
  if (d.el.hasPointerCapture(d.pointerId)) d.el.releasePointerCapture(d.pointerId);
  c.docked = cancelled ? d.wasDocked : d.inside;
  s3Stack = s3Stack.filter(id => id !== d.id); if (c.docked) s3Stack.push(d.id);
  let target;
  if (cancelled) target = d.origin;
  else if (c.docked) target = dockPose(d.id);
  else { target = { x: d.point.x - d.grab.x, y: d.point.y - d.grab.y, s: 1 }; c.outside = { ...target }; }
  cardDepths(); settleCard(d.id, target);
  if (target.x < printRect.x || target.y < printRect.y || target.x + c.width * target.s > printRect.x + printRect.w || target.y + c.height * target.s > printRect.y + printRect.h) {
    printExpanded = true; setPrintRect(currentPrintRect()); warmSoon(0);
  }
}
stage.addEventListener('pointerup', e => { if (s3Drag) { s3Drag.point = localPoint(e); s3Drag.inside = inPreview(s3Drag.point); endDrag(); } });
stage.addEventListener('pointercancel', () => endDrag(true));
stage.addEventListener('lostpointercapture', () => endDrag(true));
function toggleCardPlacement(id) {
  const c = CARDS[id]; if (!c) return;
  s3Touched = true; c.docked = !c.docked;
  s3Stack = s3Stack.filter(key => key !== id); if (c.docked) s3Stack.push(id);
  cardDepths(); settleCard(id, c.docked ? dockPose(id) : c.outside);
}
stage.addEventListener('click', e => {
  if (shown !== SHIPS || s3SuppressClick) { s3SuppressClick = false; return; }
  const id = cardId(e.target.closest('[data-asset]')); if (!id) return;
  e.preventDefault(); toggleCardPlacement(id);
});
stage.addEventListener('keydown', e => {
  if (shown !== SHIPS) return;
  if (e.key === 'Escape' && s3Drag) { e.preventDefault(); endDrag(true); return; }
  if (!['Enter', ' '].includes(e.key) || s3Drag) return;
  const id = cardId(e.target.closest('[data-asset]')); if (!id) return;
  e.preventDefault(); s3Touched = true;
  toggleCardPlacement(id);
});
stage.addEventListener('focusin', () => { if (shown === SHIPS) cardDepths(); });
stage.addEventListener('focusout', () => { if (shown === SHIPS) requestAnimationFrame(cardDepths); });
function placeScene3Artifacts() {
  const r = cell.getBoundingClientRect(), left = Math.max(-260, (16 - r.left) / SCALE);
  const homes = { nb: { x: left + 8 + s3Jitter[0] * 20, y: -4 + s3Jitter[1] * 16, s: 1 }, sk: { x: left + 42 + s3Jitter[2] * 28, y: 305 + s3Jitter[3] * 20, s: 1 }, video: { x: left + 20, y: 185, s: 1 } };
  for (const id of S3_ORDER) { const c = CARDS[id]; c.outside = homes[id]; c.pose = c.docked ? dockPose(id) : { ...c.outside }; paintCard(id); }
}
addEventListener('resize', () => {
  if (shown !== SHIPS) return;
  layoutScene3Layers(); if (!s3Touched && !s3Drag) placeScene3Artifacts();
});
for (const id of ['nb', 'sk']) $(id).addEventListener('load', () => sketchVisible(shown === SHIPS));
function enterScene3() {
  s3HardStop = false; warmScene3Media();
  for (const id of S3_ORDER) {
    const c = CARDS[id]; c.el.tabIndex = 0;
    c.el.setAttribute('role', 'button'); c.el.setAttribute('aria-label', c.label + '; click or press Enter to move it into or out of the article');
    c.el.setAttribute('aria-keyshortcuts', 'Enter Space');
  }
  if (!s3Initialized) { placeScene3Artifacts(); s3Initialized = true; }
  cardDepths(); armScene3Layers(); sketchVisible(true); videoActive(true);
}
function leaveScene3() {
  if (s3Drag) endDrag(true); s3HardStop = true;
  for (const id of S3_ORDER) {
    const el = CARDS[id].el; stopCard(id);
    el.tabIndex = -1; el.removeAttribute('role'); el.removeAttribute('aria-label'); el.removeAttribute('aria-keyshortcuts');
  }
  showAssembledPreview(); videoActive(false); sketchVisible(false);
}
function scenes(next) {
  if (!ed || !sh || !vd) return;
  if (next === phase) return;
  const from = phase; phase = next; gen++;
  if (from === 'write') { ed.stop(); ed.setDoc(FINAL); ed.blur(); }   // the poem is finished; the brush arrives
  if (next !== 'write') editorPlayback(false);
  if (next !== 'live') { sh.hold(); sh.setPending({}); }
  if (from === 'ships' && next !== 'ships') leaveScene3();
  setFan(next === 'deploy');
  if (next === 'write') writeLoop(from === 'morph');
  else if (next === 'live') liveLoop();
  else if (next === 'ships') enterScene3();
  // Scene 2's Publish cue: this is its own dispatch, both ways -- 'live'
  // starting is the only moment it may turn on, and 'live' ending (a wash
  // to elsewhere sets phase to 'morph' before that wash's first frame) is
  // the moment it must turn off, not up to 2+ seconds later when that wash
  // finally lands.
  syncPubCue();
}

// For the harness.
landing.still = (scene) => {
  shown = target = scene; still(scene);   // the tick fills whatever print this leaves missing
  // A real arrival always runs onScroll (which triggers warmScene3Media)
  // before still() ever does — this bypasses that, so it stands in.
  if (scene >= SHIPS - 1 && scene <= SHIPS + 1) warmScene3Media();
};
landing.wash = (to) => { if (running()) return Promise.resolve(false); target = to; return runJoin().then(() => true); };
landing.probe = (x, y) => sim.probe(x, y);
// The transition on screen (WatercolorMorph): the p it last presented and the
// two prints it carries, whether that frame was exactly p's, and a
// dissolve's length and checkpoint spacing.
landing.morph = { current: () => morph?.current(), leg: () => morph?.current()?.tag ?? null, steps: DISSOLVE_STEPS, every: CHECKPOINT_EVERY, bounds: [WatercolorMorph.A_END, WatercolorMorph.B_START] };
// The visible background sampled for word contrast: top-down straight-alpha
// RGBA, inherited opacity and the page beneath it. Null without a wash.
landing.readback = () => wordContrast.sample;
// The live array itself, not a copy: a fault is injected by assigning into
// an element (e.g. prints[3] = null), so a snapshot here would turn that
// into a no-op that still passes.
landing.prints = sheets;
landing.restY = restY;   // the settle's own geometry, so a rest check reads it rather than keeping a second copy
landing.targetAt = targetAt;   // what a rested position names, read the same way the wash reads it
landing.printGeneration = () => printGeneration;   // a monotonic count, bumped only by applyPrintRect -- ground truth for "did the rect actually change", never a transient read of prints itself
landing.state = () => ({ titleWash: TITLE_WASH, shown, target, running: running(), phase, steps, joins: joinsRun, washes: washesRun, fanned: stage.classList.contains('fanned'), xf: +xf.toFixed(3), loop: !loopMounted ? 'unmounted' : loopVid.error ? 'error' : loopVid.paused ? 'paused' : 'playing', washT: +washT.toFixed(2), captureMs, scrollV: +scrollV.toFixed(2), progress: +progressAt().toFixed(3), primed: primed(), sim: !!sim, ready: !!(ed && sh) && primed(), recordSteps, titleSteps, titleReady, titleMode });

const when = (frame, key) => new Promise((resolve, reject) => {
  const deadline = setTimeout(() => { clearInterval(poll); reject(new Error(`Demo ${frame.id} did not initialize`)); }, 10000);
  const poll = setInterval(() => {
    const api = frame.contentWindow?.[key];
    if (api) { clearInterval(poll); clearTimeout(deadline); resolve(api); }
  }, 50);
});
async function ready() {
  // Images and creative artifacts share one complete printable domain before
  // any scene is captured, including untouched initial arrangements.
  placeScene3Artifacts(); s3Initialized = true;
  printExpanded = true; setPrintRect(currentPrintRect());
  // a print that never arrives must not hold the page shut: with none, the
  // scenes cut, which is the path reduced motion already takes. Reported
  // rather than latched permanently unprintable -- retakeShown's own tick
  // (below) already retries a late/failed print gracefully; boot's first
  // attempt failing is this attempt's problem, not the rest of the session's.
  if (washing() && needed(shown).length) { try { setSheet(shown, await capture(shown)); await takeOthers(mobileLayout() ? neighbours(shown) : washable.filter((scene) => scene !== shown)); } catch (e) { reportCaptureFault(`boot: ${e.message}`); } }
  booted = true;
  renderMorphAt(progressAt());
  target = asked = shown;
  document.documentElement.dataset.ready = '1';
}
// Every frame must be there before the first prints are taken: a print of a
// scene whose document has not loaded is a blank sheet.
Promise.all([when(edFrame, '__editor'), when(shFrame, '__shell'), when(vdFrame, '__shell')])
  .then(([e, s, v]) => {
    ed = e; sh = s; vd = v;
    // The control scene 4 is left holding is a live one, in both the scenes
    // that show it — set once, so a print taken in scene 3 and the control the
    // wash cures onto in scene 4 are the same control in the same state. Files
    // to upload and no classified change set: the pill is ink and awake, and
    // the composition ring stays clear, so the only colour in the scene is ink.
    vd.setPending({ flatUpload: 1 });
    measurePub();
    return ready();
  }).catch(error => {
    console.warn('Showing the readable landing page:', error.message);
    document.documentElement.dataset.static = '1';
    page.inert = false; five.inert = false;
    // watchScroll (and with it driveTitleDissolve) stops once dataset.static
    // is set, so a dissolve caught mid-flight would otherwise freeze the
    // title invisible; the static layout needs it back as plain text.
    openingTitle.style.opacity = '';
    document.getElementById('gl-title')?.style.setProperty('display', 'none');
  });
