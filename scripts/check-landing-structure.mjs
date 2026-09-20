#!/usr/bin/env node
// A ratchet on the runtime script's own bloat, not on what it does: every
// phase of the landing rewrite either lowers one of these four numbers or
// leaves it alone, and this fails a phase that quietly lets one rise. Unlike
// a normal ratchet it also fails on an unlocked improvement (see below) —
// this script's own stored numbers are the baseline every later phase edits
// downward, in the same commit that earns the lower number.
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const INDEX_HTML = resolve(fileURLToPath(new URL('.', import.meta.url)), '..', 'site', 'index.html');

// Recorded after phase 2's `window.__landing` consolidation (2026-09-20).
// Lower these numbers, in the same commit that earns it, whenever a later
// phase actually reduces one — see the fail message below for why leaving
// a lowered number unrecorded is itself a failure.
const BASELINE = {
  windowAssignments: 1,
  topLevelLets: 90,
  sceneComparisons: 14,
  byteSize: 282550,
};

// Locates the runtime script: the one inline, non-`src` `<script>` block
// that defines `window.__landing` — the state machine, the print pipeline,
// the fluid renderer and the scroll drivers all live in it. Not the tiny
// loader scripts in <head>, and not the per-iframe setup snippet in the
// markup, which are a handful of lines each and carry none of this.
function runtimeScript(html) {
  const blocks = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map((m) => m[1]);
  const found = blocks.find((b) => b.includes('window.__landing'));
  if (!found) throw new Error('runtime script (window.__landing) not found in site/index.html');
  return found;
}

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
const SCENE_FUNCTIONS = ['pour', 'takePrint', 'watchScrollIntent', 'watchScrollWait', 'watchScrollCss'];

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
const script = runtimeScript(html);
const current = {
  windowAssignments: countWindowAssignments(script),
  topLevelLets: countTopLevelLetBindings(script),
  sceneComparisons: countSceneComparisons(script).total,
  byteSize: Buffer.byteLength(html, 'utf8'),
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
  console.log('structure: window.__ assignments, top-level let bindings, scene comparisons in the shared functions, and page byte size all match the recorded baseline');
}
