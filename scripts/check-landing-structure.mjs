#!/usr/bin/env node
// A ratchet on the runtime script's own bloat, not on what it does: every
// phase of the landing rewrite either lowers one of these five numbers or
// leaves it alone, and this fails a phase that quietly lets one rise. Unlike
// a normal ratchet it also fails on an unlocked improvement (see below) —
// this script's own stored numbers are the baseline every later phase edits
// downward, in the same commit that earns the lower number.
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const SITE_DIR = resolve(fileURLToPath(new URL('.', import.meta.url)), '..', 'site');
const INDEX_HTML = resolve(SITE_DIR, 'index.html');
const LANDING_JS = resolve(SITE_DIR, 'landing.js');

// Recorded after phase 3's script extraction (2026-09-20): the runtime moved
// verbatim from an inline <script> in site/index.html to site/landing.js,
// loaded the same way (a classic, non-deferred, non-module <script src>) at
// the same position, so the two byte counts below replace the single
// byteSize this script used to track. Lower any of these numbers, in the
// same commit that earns it, whenever a later phase actually reduces one —
// see the fail message below for why leaving a lowered number unrecorded is
// itself a failure. The same logic runs in reverse for a real rise: the site
// owner's five-item polish pass (2026-09-20) added scene 2's Publish cue and
// its markup, CSS and keyframes, so htmlBytes and scriptBytes below are the
// recorded, not-quiet new floor, not a loosened tolerance -- the fixes for
// scene 1's plate margins, the window-radius match and the mobile header
// scrim added only inline data/CSS. topLevelLets itself does NOT carry a new
// binding for the cue (2026-09-21): its first cut used one top-level `let`
// (publishActivated) and a setInterval poll violating R8 (no periodic work
// at rest); the event-driven rewrite reads "activated" off a dataset flag on
// the already-existing #pub-cue element and syncs from scenes()'s own
// dispatch plus resize/reduced-motion/click listeners instead, landing back
// on the same topLevelLets this file already had. scriptBytes rose again
// with that rewrite's own comments and listeners.
const BASELINE = {
  windowAssignments: 1,
  // Unit 1 (2026-09-21): fell 87 -> 84. review-phases-2-4.md's M1/M2/M4/M5:
  // the gesture record's six-ish scattered names (gesture/inputArmed/the four
  // settle* diagnostics) become contact/run/armed/gesture/settleAt, one
  // object per concern instead of one let per field, with kind fully DERIVED
  // rather than written by five call sites. contact/gesture/settleAt are
  // const (only their fields mutate), so the fall is real, not a rename.
  // Unit 1b (2026-09-21): rose 84 -> 85, then fell back to 84 same day.
  // Deleting the once-a-frame kind cache needed a binding to carry the raw
  // value tickGesture itself saw last call across calls (wasHeld), added as
  // its own top-level let; housekeeping folded it onto the existing const
  // contact record (contact.wasHeld) instead, restoring the count -- the
  // ratchet's own numbers only ever go down.
  // Unit 2 (2026-09-21), dissolve step C: fell 84 -> 83. The printable
  // latch (set false on any capture failure, never reset) is deleted --
  // every failure site now reports the fault and lets the existing retake
  // loop try again instead of disabling washes for the rest of the session.
  // Speedups item 1 (2026-09-21): fell 83 -> 81. `let run = null, restOwed
  // = false;` moved into site/landing-gesture.js's createLandingGesture()
  // closure along with the rest of the gesture record -- landing.js reads
  // and writes both through the gestureModel accessor pair it destructures
  // instead of owning the bindings itself.
  // Intro title restoration (finding 1, 2026-09-21): rose 81 -> 86.
  // introMaskStep (introWatercolor's own cache key), washDissolve/primeWash
  // (the sim's renderer and its boundary-handoff snap, split out of
  // titleDissolve so driveTitleDissolve can gate the swap instead of
  // reassigning it directly), simArmed (arming without activating), and
  // paperPixelsCache (the hoisted, memoized noise buffer) -- five real
  // bindings for a real restoration, not five ways to say the same thing.
  // 86 -> 87 (2026-09-22): e14c2f0f added mobileMount/mobileLiveScene/
  // mobileBridge (89) without updating this baseline; consolidating them
  // into one `mob` record brings it to 87, still below that unrecorded high.
  // 87 -> 88 (2026-09-24, measured): item D's `words` (the wrapped-span
  // records updateWordContrast reads and writes, built once and lazily).
  // 88 -> 89 (2026-09-24, measured): item D's lastContrastRefresh, the
  // throttle clock for the small readback (found live: an unthrottled
  // synchronous GPU readback every rendered frame measurably slowed the
  // page, enough to break check-landing-mobile.mjs's own small-wheel-delta
  // check elsewhere on the same run).
  topLevelLets: 77,
  // Rose 14 -> 15 on 2026-09-21: the last scene's rest is a floor, not a point,
  // so a visitor can reach the footer on a short window. It is written as one
  // comparison against SHARE in the desktop drive. The scene table pays this
  // back: a floor rest becomes a property of the scene, not a literal here.
  // Fell 15 -> 11 (unit7-scroll-and-flicker-spec.md part A): the mobile
  // presenter moved from pour() to renderMorphAt, so the mobileLayout()-gated
  // logo-solid trigger and the mobile cell-transform's size/sizeProgress
  // formula inside pour() were dead the moment mobile stopped calling it --
  // 4 of this count's own `to === DEPLOY`/`from === DEPLOY` comparisons,
  // deleted with them rather than left to read as live.
  // Fell 11 -> 9 (2026-09-23): pour() and mountLeg() no longer special-case
  // scene 3 for a retake; memberPrint reads every leg's members the same way.
  sceneComparisons: 8,
  // Unit 0a: rose again -- the mobile header band's fix (one shared
  // body::before band interpolating colour through --xf instead of two
  // separate .brand/.language-picker background boxes) added net new
  // comment explaining why, not new markup or a new element.
  // Unit 0a follow-up: rose again for the --hdr flip fixing the header
  // contrast dip (a real defect a visitor could stop the scroll inside,
  // not new markup) -- the explanatory comment plus five colour rules
  // rewritten to use --hdr instead of --xf, mobile-scoped only.
  // Unit 4 (2026-09-21), the pin: rose again, 51193 -> 54304. #c5's own
  // margin-bottom (the pin fix, real new geometry) and #five's compensating
  // margin-top, plus desktop's own --hdr flip (item d, a real contrast
  // regression the pin fix itself causes) and its own measured derivation,
  // each with the comment this file's style expects; the dead position/
  // inset deleted from #five's base rule is what kept this from rising
  // further still.
  // Unit 2 (2026-09-21), dissolve step B: rose 54304 -> 54350. One
  // <script src="watercolor-capture.js"> tag: the painter registry
  // landing.js's own capture now routes the Mandelbrot's two capture sites
  // through, instead of each inlining its own blind-toDataURL fallback.
  // Speedups item 1 (2026-09-21): rose 54350 -> 54393. One more tag, same
  // shape as the one above: <script src="landing-gesture.js"></script>,
  // loaded immediately before landing.js so the extracted gesture record is
  // defined before landing.js calls window.LandingGesture().
  // Publish-ring restoration (finding 3, 2026-09-21): fell 54393 -> 54391.
  // The stroke-thickness fix wraps each ring's <circle> in a <g>, moves the
  // breathing transform onto it, and adds overflow:visible (the <g>'s own
  // growth needs it spelled out -- an <svg> clips to its own box by
  // default, which cost nothing while the host itself carried the scale).
  // The corrected comment above .pub-ring -- the old one asserted
  // vector-effect works "regardless of scale", the false premise that let
  // the bug ship -- is shorter than what it replaced by more than the code
  // grew.
  // Icons (finding 6, 2026-09-21): rose 54391 -> 54990. Four new <link>
  // tags (apple-touch-icon, PNG 32, PNG 16, plus the existing SVG) and the
  // comment explaining why this page repeats what the docs template
  // already declares -- real markup, not bloat.
  // 54990 -> 55034 (2026-09-23): measured -- one <script src="watercolor-morph.js"> tag.
  // 55034 -> 54436 (2026-09-23): the siblings' and the video's own fade ramps
  // and their four delays gave way to one membership class.
  // 54436 -> 54941 (2026-09-24, measured): item D's own `.word` rule and
  // its explanatory comment (site/index.html).
  // 54941 -> 55331 (2026-09-25): the mobile scene-2 plateau and
  // contrast-safe discrete word colours are intentional interface geometry;
  // the extra bytes replace the old animated colour transition.
  htmlBytes: 55551,
  // Unit 0a: rose again for landing.printGeneration(), a getter exposing
  // the existing printGeneration counter on __landing's read side -- the
  // fix for check-landing-invariants.mjs's I-fuzz-invalidated racing on a
  // transient read of prints itself needed a ground-truth signal instead.
  // Unit 1 (2026-09-21): rose again, 240660 -> 244161. tickGesture, the
  // derived gestureKind/gestureHeld, holdDirect and the position-gated
  // closing trigger are real new logic (M1/M2/M4), each with the comment
  // this file's own style expects; topLevelLets above is what this unit
  // was sent to lower, and it did. Includes a follow-up within the same
  // unit: state()'s held/settle read gestureHeld/run live rather than the
  // once-a-frame gesture.kind cache, caught by I-gesture(c) itself -- a
  // synchronous pointerdown-then-read from the harness landed between the
  // event and the next tickGesture and saw the stale cached kind.
  // Unit 1b (2026-09-21): rose again, 244161 -> 245796, for a strict
  // re-review's four MUST fixes: deleting the kind cache (every production
  // read site now calls gestureKind(now) itself, replacing one field read
  // with a short comment at each site), the closing gate's xfAt() < 1 upper
  // bound deleted (a real regression -- it excluded a hard flick past the
  // band from ever settling to closingRestY()), one-line pointers at the
  // gate's own duplicated geometry naming the unit that deletes it, and the
  // new overshoot-close invariant this bug needed to be caught at all.
  // Unit 1b housekeeping: rose 2 bytes, 245796 -> 245798, folding wasHeld
  // onto contact and updating the comments that named it.
  // Unit 3 (2026-09-21), closing-progress unification (review-phases-2-4.md
  // Job 2 item 5): rose 245798 -> 246609. progressAt()'s desktop branch
  // gained real crossfade geometry for the DEPLOY..SHARE interval instead of
  // a text-flow gap unrelated to #five's own band; xfAt() derives from it on
  // desktop (mobile keeps mobileClosingProgress() through its own smooth()
  // ease, untouched). I-progress is the new invariant.
  // Unit 4 (2026-09-21), the pin: fell 246609 -> 245300. nativeScroll()'s
  // xfAt() > 0 clause and watchScrollNative's whole closing-settle special
  // case are deleted -- desktop stays on watchScrollDesktop through the
  // close now, the DEPLOY..SHARE boundary commits the same way every other
  // one does. Locked in the same commit that earned it.
  // Unit 2 (2026-09-21), dissolve steps B/C: rose 246178 -> 248954. fold()'s
  // canvas branch and rasterScene3Artifact's 'sk' branch (the only two
  // sites that ever captured the Mandelbrot canvas) now call
  // WatercolorCapture.painters.webgl instead of inlining
  // mossCaptureFrame()-or-blind-toDataURL; a missing scene 3 artifact
  // degrades to its own gap instead of throwing the whole print away; the
  // new captureFaults/reportCaptureFault report every degrade on
  // __landing's own read side. Real new capability -- an honest per-element
  // fallback and fault reporting where there was a silent hole and a
  // session-wide throw -- not unlocked bloat.
  // Unit 5 step 1a (2026-09-21), unit5-physics-spec.md section 1a: rose
  // 248954 -> 250084. Uptake gated on the dissolved fraction with the old
  // clock kept as a floor -- three new PIG uniforms, their link-time
  // defaults, the take()-gated ad computation and its comment, and the
  // per-step JS split into uAds/uTakeFloor. Within this unit's pre-approved
  // 6000-byte rise (budget consumed so far: 1130 of 6000).
  // Unit 5 step 1b (2026-09-21), unit5-physics-spec.md section 1b: rose
  // 250084 -> 250538. uMixHold, its link-time default and the mixGate
  // computation that damps the whole-film mean while a texel is still
  // releasing A's ink. Budget consumed so far: 1584 of 6000.
  // Unit 5 step 1c (2026-09-21), unit5-physics-spec.md section 1c: rose
  // 250538 -> 251166. The cure masks its l-clamp and its SHOW substitution
  // by foot(), so it settles a deposit rather than painting one out of
  // nothing outside the wash's own footprint; no new uniform. Budget
  // consumed so far: 2212 of 6000.
  // Unit 5 step 3 (2026-09-21), unit5-physics-spec.md section 2: rose
  // 251166 -> 254615. The GPU keyframe ring (captureKeyframe/
  // maybeCaptureKeyframe/restoreNearestKeyframe, the blit helper, and
  // advanceWash's own switch from reset()-and-replay to a keyframe
  // restore) -- reversal bounded to a 63-step replay instead of 252.
  // Budget consumed so far: 5661 of 6000.
  // Rose 254615 -> 256274 on 2026-09-21 for five owner items: the title behind scene 1
  // and its haze, the Publish carry landing with scene 4's text, and three mobile timings.
  // Speedups item 1 (2026-09-21): fell 256274 -> 253580. The gesture record
  // (contact, run, restOwed, gestureHeld, gestureKind, tickGesture,
  // cancelRun, armSettle, holdDirect, holdOff and their comments) moved
  // verbatim to site/landing-gesture.js, which this script does not count --
  // it measures site/landing.js only, by design (SITE_DIR/LANDING_JS
  // above); the new file is scripted separately if a future phase wants a
  // ratchet on it too.
  // Mobile scene1->2 restoration (finding 7, 2026-09-21): rose 253580 ->
  // 254419. mobileEntranceProgress's own Math.min(entrance, pinned) --
  // pinned a boolean -- is now one clamp01 ratio (net smaller), but the
  // reasoning for its 0.55*band.height span and the earlyBy constant's new
  // name and window.__landing.mobileEarlyBy exposure (so
  // check-landing-text-track.mjs observes it instead of re-typing 40) are
  // real additions, not bloat.
  // Intro title restoration (finding 1, 2026-09-21): rose 254419 -> 264473.
  // introWatercolor and its cache, restored close to verbatim from the
  // pre-refactor build (git tag landing-live-20260919); the boundary-gated
  // handoff (primeWash, simArmed, driveTitleDissolve's own swap check) that
  // replaces the direct titleDissolve reassignment the bug lived in;
  // warmShaderCache (KHR_parallel_shader_compile polling) and
  // computePaperPixels (genPaper hoisted and memoized) for the arming
  // stall; and landing.title.trace, the check's own opt-in, zero-cost-when-
  // off instrumentation for verifying any of it without a sampler race.
  // Real, requested restoration and its own verification, not bloat.
  // +443 on 2026-09-21: the TITLE_WASH gate and its comment; the wash pops on the first tick after idle.
  // 264916 -> 276392 (2026-09-22): e14c2f0f's renderMorphAt/mountLeg/
  // showMobileScene left this unrecorded at 274239; the mob-record
  // consolidation (real new logic plus its own explanatory comments)
  // brings it to 276392.
  // 276392 -> 276643 (2026-09-22): standAtTerminalClose's mob.scene = SHARE
  // fix (and its comment) for the reverse-latch bug that stranded shown at
  // SHARE on mobile.
  // 276643 -> 277909 (2026-09-22): updateFinalDissolve's cover/grayscale
  // switch moved off finalWash's own catch-up paint onto q directly (the
  // mobile reverse-leg-off-the-closing-scene fix), plus dropping the
  // releasePigmentCover() call that stripped a class renderMorphAt still
  // owned -- real logic plus its own explanatory comments, not bloat.
  // 277909 -> 278381 (2026-09-22): mobileClosingProgress rewritten to
  // measure against closingRestY() instead of mobileInkProgress's own
  // text-height span (owner report, "after scene 5 I cannot scroll back") --
  // real logic plus its own explanatory comment, not bloat.
  // 278381 -> 278771 (2026-09-23): SHOW's alpha gained a wet*foot() sheen
  // floor and its own comment (check-landing-morph.mjs clause 3, canvas
  // coverage collapsing on every wash leg because alpha depended only on
  // pigment, never on the water the shader already computed) -- real logic
  // plus its own explanatory comment, not bloat.
  // 278771 -> 286090 (2026-09-23): measured, set after correctness -- the three-phase transition's engine half (dissolve recorder, mass fixer, SHOWK/FIX), net of pour()'s deleted clock and the sheen floor.
  // 286090 -> 289151 (2026-09-23): measured -- the dissolve shown as the real film, replayed from checkpoints (seek, checkpoint save/load, the mass fixer's interpolation), net of the stored-frame bookkeeping it replaced.
  // 289151 -> 295117 (2026-09-23): measured -- membership (memberPrint, artifactNow, a recomposable scene 3 print, deployPrint's two halves, the members' hide and fade in watercolor-morph.js's caller), the fixer read every 6 steps, and the warmer kept off a capture in flight.
  // 295117 -> 295300 (2026-09-24): measured -- mobileEntranceProgress touch-gated (item A), replacing the #col-pin read with #c2's own text and its explanatory comment.
  // 295300 -> 297340 (2026-09-24): measured -- item B: holdMorph's carry flag, the Publish control's mobile-only cubic ease-in scale (publishBridge's draw()), and their explanatory comments.
  // 297340 -> 300637 (2026-09-24): measured -- item C: mobile-only wash bounds/step-ease (MOBILE_MORPH_BOUNDS), the SHOWK shader's wet-chroma gain, and their explanatory comments including the measured tuning numbers.
  // 300637 -> 319748 (2026-09-24, measured): item D itself -- initWords,
  // bgLuminanceUnder, updateWordContrast/resetWordContrast and the small
  // downsampled GPU readback they read (makeSim's own refreshSmall/tick),
  // plus their explanatory comments including the measured tuning numbers.
  // 319748 -> 320915 (2026-09-24, measured): the CONTRAST_REFRESH_MS
  // throttle and its own comment, plus the watchScrollNative branch that
  // calls updateWordContrast on a frame renderMorphAt itself skipped.
  // 320915 -> 320980 (2026-09-24, measured): a misordered comment above
  // refreshSmall/smallPixels put back next to the method it describes, and
  // landing.readback's own comment corrected (64x64 -> 128x128, stale
  // since item D's own resolution bump).
  // 320980 -> 320787 (2026-09-25): the mobile timing comments were shortened
  // while widening the entrance ramp and softening its phase curve.
  // 320787 -> 318982 (2026-09-25): the earlier opening trigger is named,
  // contrast reads the whole composited word box, and obsolete fade-margin
  // policy gave way to each colour's actual contrast floor.
  scriptBytes: 294964,
};

function countWindowAssignments(text) {
  // A real assignment only: `window.__NAME =` with a single `=`, not the
  // `==`/`===` a read or a comparison uses.
  return (text.match(/window\.__[A-Za-z0-9_]+\s*=(?!=)/g) || []).length;
}

function countTopLevelLetBindings(text) {
  // Every name a top-level `let` statement declares, comma list included
  // (`let a = 1, b = 2;` is two bindings) — this is what a later phase's
  // record-shaped merges actually shrink, not the statement count.
  let count = 0;
  for (const stmt of text.matchAll(/^let\s+([^;]+);/gm)) {
    let depth = 0, cur = '', parts = [];
    for (const ch of stmt[1]) {
      if ('([{'.includes(ch)) depth++;
      else if (')]}'.includes(ch)) depth--;
      if (ch === ',' && depth === 0) { parts.push(cur); cur = ''; }
      else cur += ch;
    }
    parts.push(cur);
    count += parts.filter((p) => p.trim()).length;
  }
  return count;
}

function extractFunction(text, name) {
  const re = new RegExp(`(?:async\\s+)?function\\s+${name}\\s*\\([^)]*\\)\\s*\\{`);
  const m = re.exec(text);
  if (!m) throw new Error(`function ${name}() not found in the runtime script`);
  let depth = 0, start = m.index + m[0].length - 1, i = start;
  for (; i < text.length; i++) {
    if (text[i] === '{') depth++;
    else if (text[i] === '}') { depth--; if (depth === 0) break; }
  }
  if (depth !== 0) throw new Error(`function ${name}() body did not close`);
  return text.slice(start, i + 1);
}

// The shared print, wash and scroll functions (moss-landing-lab code map
// §2b/§2c and the scroll drivers §2a): the only places a scene index is
// meant to be tested against a literal or SHIPS/DEPLOY/SHARE at all, once
// phase 5 merges PHASE/FRAMES/GROUND/JOINS into one table.
const SCENE_FUNCTIONS = ['pour', 'takePrint', 'watchScrollNative'];

function countSceneComparisons(text) {
  const ident = '(?:scene|to|from|target|shown)';
  const literal = '(?:-?\\d+|SHIPS|DEPLOY|SHARE)';
  const re = new RegExp(`\\b${ident}\\s*[=!]==?\\s*${literal}\\b|\\b${literal}\\s*[=!]==?\\s*${ident}\\b`, 'g');
  let total = 0;
  const perFunction = {};
  for (const name of SCENE_FUNCTIONS) {
    const count = (extractFunction(text, name).match(re) || []).length;
    perFunction[name] = count;
    total += count;
  }
  return { total, perFunction };
}

const html = await readFile(INDEX_HTML, 'utf8');
const script = await readFile(LANDING_JS, 'utf8');
const current = {
  windowAssignments: countWindowAssignments(script),
  topLevelLets: countTopLevelLetBindings(script),
  sceneComparisons: countSceneComparisons(script).total,
  htmlBytes: Buffer.byteLength(html, 'utf8'),
  scriptBytes: Buffer.byteLength(script, 'utf8'),
};

const failures = [];
for (const [key, baseline] of Object.entries(BASELINE)) {
  const value = current[key];
  if (value > baseline) failures.push(`${key} rose from ${baseline} to ${value}`);
  else if (value < baseline) failures.push(`${key} fell from ${baseline} to ${value} — lower BASELINE.${key} to ${value} in this script, in the commit that earned it`);
}

console.log(JSON.stringify({ current, baseline: BASELINE }, null, 2));
if (failures.length) {
  for (const f of failures) console.error(f);
  process.exitCode = 1;
} else {
  console.log('structure: window.__ assignments, top-level let bindings, scene comparisons in the shared functions, and the HTML and script byte sizes all match the recorded baseline');
}
