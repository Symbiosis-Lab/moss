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
  topLevelLets: 87,
  sceneComparisons: 14,
  htmlBytes: 49493,
  scriptBytes: 240465,
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
const SCENE_FUNCTIONS = ['pour', 'takePrint', 'watchScrollDesktop', 'watchScrollReduced', 'watchScrollNative'];

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
