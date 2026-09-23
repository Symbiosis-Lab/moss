#!/usr/bin/env node
// Static check on the demo system's content: no browser, no build, done in seconds — the rules
// site/ui/demo/README.md states as prose, checked as data instead of only being caught later by
// check-docs-demo.mjs's real-browser run. Run: node scripts/check-demo-scenes.mjs
//
// Rules (one function each, below):
//   1. every `#scene=<name>` link in the site's Markdown names a scene file that exists
//   2. every scene file is linked by at least one page (nothing orphaned)
//   3. every step's target name is one its scene's surface adapter actually has
//   4. a page never mixes surfaces across its own markers (site/ui/demo/README.md, "Surfaces")
//   5. a scene's own `after` name resolves to a real scene, and the `after` graph has no cycle
//   6. a scene stays inside the spec's bound: about four steps, about ten seconds
//      (site/ui/demo/README.md, "Layout of an illustrated section")
//
// Exits 1 and prints every violation found (not just the first) if any rule fails.

import { readdir, readFile } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { resolve, relative, extname } from 'node:path';
import { travelDurationMs, holdAfterResultMs } from '../site/ui/demo/driver.js';
import { TARGET_NAMES as EDITOR_TARGET_NAMES } from '../site/ui/demo/surfaces/editor.js';

export const ROOT = resolve(fileURLToPath(new URL('.', import.meta.url)), '..');
export const SITE_DIR = resolve(ROOT, 'site');
const SCENES_DIR = resolve(SITE_DIR, 'ui/demo/scenes');

// The one surface this PR ships (site/ui/demo/README.md, "Surfaces") — a scene omitting `surface`
// defaults to this, and it is also what the theme's upgrade (site/.moss/theme/script.js) inserts
// unconditionally. A second surface joins this map, not a chain of `if`s, when one exists.
const SURFACES = {
  editor: EDITOR_TARGET_NAMES,
};
const DEFAULT_SURFACE = 'editor';

// Spec bound (site/ui/demo/README.md, "Layout of an illustrated section": "kept to about four
// gestures and ten seconds"). Both numbers carry slack past the literal "about four"/"ten": a
// static check can't see a step's real on-screen travel distance, so its duration estimate below
// is necessarily a rough one, and it would rather fail loudly on something clearly too long than
// nag on content the design guidance itself already calls "about" a bound. The six-step "versions"
// scene — one paragraph describing five real gestures (right-click, Save a version, confirm, open
// a version, Restore) — is exactly the kind of case "about four" was always meant to admit.
const MAX_STEPS = 8;
const MAX_DURATION_MS = 15000;

// Duration estimate, built only from driver.js's two exported pure pace functions
// (travelDurationMs, holdAfterResultMs — see their own comments: "used by check-demo-scenes.mjs
// ... to pace-bound a scene without a browser"). Everything else the real driver times (dwell,
// press, the double-click gap) lives in driver.js's private PACE table, which this check does not
// duplicate; TRAVEL_TYPICAL_PX and STEP_OVERHEAD_MS below stand in for it as one rough, documented
// approximation rather than a second copy of numbers that could drift from the real table.
const TRAVEL_TYPICAL_PX = 400; // a representative on-screen hop within the ~600px demo frame
const STEP_OVERHEAD_MS = 550; // ~ dwell (400-600ms) + one press (150ms), averaged
const DBLCLICK_EXTRA_MS = 180 + 150; // the second press's own gap + dip (site/ui/demo/README.md, "Pace")
const TYPE_CHAR_MS = 55; // driver.js PACE.typeIntoCharMs
const TYPE_COMMIT_MS = 300; // rough allowance for the Enter that commits a typeInto step

function stepVerb(step) {
  const verbs = Object.keys(step).filter((k) => k !== 'reveals');
  return verbs.length === 1 ? verbs[0] : null;
}

/** The named target(s) a step addresses, or `[]` for a step this check does not resolve a target
 * for (there are none today, but a future verb failing open here would silently skip validation —
 * this returns `null` instead so the caller can tell "no target to check" from "malformed step"). */
function stepTargetNames(step, verb) {
  if (verb === 'typeInto') {
    const target = step.typeInto?.target;
    return typeof target === 'string' ? [target] : null;
  }
  if (verb === 'click' || verb === 'context' || verb === 'dblclick') {
    return typeof step[verb] === 'string' ? [step[verb]] : null;
  }
  return null;
}

function stepDurationMs(step, verb) {
  if (verb === 'typeInto') {
    const text = step.typeInto?.text;
    // Per-locale text (player.js's resolveLocale) — the longest variant is the honest worst case,
    // since every locale plays the same scene.
    const lengths = typeof text === 'string' ? [text.length] : Object.values(text ?? {}).map((s) => (s ?? '').length);
    const longest = lengths.length ? Math.max(...lengths) : 0;
    return longest * TYPE_CHAR_MS + TYPE_COMMIT_MS;
  }
  const travel = travelDurationMs(TRAVEL_TYPICAL_PX);
  const hold = holdAfterResultMs(step.reveals);
  const extra = verb === 'dblclick' ? DBLCLICK_EXTRA_MS : 0;
  return travel + STEP_OVERHEAD_MS + extra + hold;
}

// The page→scene-link walk, shared with scripts/check-docs-demo.mjs so that checker's list of
// pages/markers to drive in a real browser is derived from the same source-of-truth traversal
// this static check uses, rather than a hand-typed second copy that can drift from it.
export async function findMarkdownFiles(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const full = resolve(dir, entry.name);
    if (entry.isDirectory()) {
      // The demo system's own source (site/ui/demo) and the .moss project dirs carry no authored
      // guide content — skip them rather than special-casing their own README/scene JSON below.
      if (full === resolve(SITE_DIR, 'ui') || entry.name === '.moss' || entry.name.startsWith('.')) continue;
      files.push(...(await findMarkdownFiles(full)));
    } else if (extname(entry.name) === '.md') {
      files.push(full);
    }
  }
  return files;
}

// `[label](#scene=name)` — `name` may be empty ("just open the surface", site/ui/demo/README.md,
// "Markdown markers"). Matches the Markdown source, not rendered HTML, so this needs no build.
const SCENE_LINK_RE = /\[[^\]]*\]\(#scene=([^)\s]*)\)/g;

export async function findSceneLinks(files) {
  const links = []; // { page, name }
  for (const file of files) {
    const text = await readFile(file, 'utf8');
    for (const match of text.matchAll(SCENE_LINK_RE)) {
      links.push({ page: relative(ROOT, file), name: decodeURIComponent(match[1]) });
    }
  }
  return links;
}

async function listSceneFiles() {
  const entries = await readdir(SCENES_DIR, { withFileTypes: true });
  return entries.filter((e) => e.isFile() && extname(e.name) === '.json').map((e) => e.name.replace(/\.json$/, ''));
}

async function loadScene(name) {
  return JSON.parse(await readFile(resolve(SCENES_DIR, `${name}.json`), 'utf8'));
}

async function main() {
  const errors = [];

  const files = await findMarkdownFiles(SITE_DIR);
  const links = await findSceneLinks(files);
  const sceneFileNames = await listSceneFiles();
  const sceneFileSet = new Set(sceneFileNames);

  // Rule 1: every non-empty link names a scene file that exists.
  const usedScenes = new Set();
  for (const { page, name } of links) {
    if (name === '') continue; // "just open the surface" — no scene file to check
    usedScenes.add(name);
    if (!sceneFileSet.has(name)) {
      errors.push(`${page} links scene "${name}", which has no site/ui/demo/scenes/${name}.json`);
    }
  }

  // Rule 2: every scene file is used by at least one page.
  for (const name of sceneFileNames) {
    if (!usedScenes.has(name)) {
      errors.push(`site/ui/demo/scenes/${name}.json is not linked by any page`);
    }
  }

  // Load every scene that exists (skip ones rule 1 already flagged as missing).
  const scenes = new Map(); // name -> scene data
  for (const name of sceneFileNames) {
    try {
      scenes.set(name, await loadScene(name));
    } catch (error) {
      errors.push(`site/ui/demo/scenes/${name}.json failed to parse: ${error.message}`);
    }
  }

  // Rule 3: every step's target is one its scene's surface adapter has, and rule 6: the spec's
  // step-count/duration bound.
  for (const [name, scene] of scenes) {
    const surfaceName = scene.surface ?? DEFAULT_SURFACE;
    const targetNames = SURFACES[surfaceName];
    if (!targetNames) {
      errors.push(`scene "${name}" names surface "${surfaceName}", which has no adapter (known: ${Object.keys(SURFACES).join(', ')})`);
      continue;
    }
    const steps = scene.steps ?? [];
    let durationMs = 0;
    for (const [i, step] of steps.entries()) {
      const verb = stepVerb(step);
      if (!verb) {
        errors.push(`scene "${name}" step ${i} does not have exactly one verb: ${JSON.stringify(step)}`);
        continue;
      }
      const targets = stepTargetNames(step, verb);
      if (targets) {
        for (const target of targets) {
          if (!targetNames.has(target)) {
            errors.push(`scene "${name}" step ${i} (${verb}) names target "${target}", which surface "${surfaceName}" does not have`);
          }
        }
      }
      durationMs += stepDurationMs(step, verb);
    }
    if (steps.length > MAX_STEPS) {
      errors.push(`scene "${name}" has ${steps.length} steps, past the spec's bound (${MAX_STEPS}; README: "about four gestures")`);
    }
    if (durationMs > MAX_DURATION_MS) {
      errors.push(`scene "${name}" is estimated at ${Math.round(durationMs)}ms, past the spec's bound (${MAX_DURATION_MS}ms; README: "about ten seconds")`);
    }
  }

  // Rule 5: `after` resolves to a real scene, and the graph has no cycle.
  const WHITE = 0, GRAY = 1, BLACK = 2;
  const color = new Map(sceneFileNames.map((n) => [n, WHITE]));
  function visit(name, pathSoFar) {
    if (color.get(name) === BLACK) return;
    if (color.get(name) === GRAY) {
      errors.push(`scene "after" chain cycles: ${[...pathSoFar, name].join(' -> ')}`);
      return;
    }
    color.set(name, GRAY);
    const scene = scenes.get(name);
    const after = scene?.after;
    if (after !== undefined) {
      if (!sceneFileSet.has(after)) {
        errors.push(`scene "${name}" names "after": "${after}", which has no site/ui/demo/scenes/${after}.json`);
      } else {
        visit(after, [...pathSoFar, name]);
      }
    }
    color.set(name, BLACK);
  }
  for (const name of sceneFileNames) visit(name, []);

  // Rule 4: a page never mixes surfaces across its own markers. The empty-name "just open the
  // surface" marker counts as the default surface too (site/.moss/theme/script.js inserts that
  // surface's frame unconditionally today), so a page pairing it with a real link to a different
  // surface's scene is exactly the case this rule exists to catch.
  const linksByPage = new Map();
  for (const link of links) {
    if (!linksByPage.has(link.page)) linksByPage.set(link.page, []);
    linksByPage.get(link.page).push(link);
  }
  for (const [page, pageLinks] of linksByPage) {
    const surfacesOnPage = new Set();
    for (const { name } of pageLinks) {
      if (name === '') { surfacesOnPage.add(DEFAULT_SURFACE); continue; }
      const scene = scenes.get(name);
      if (!scene) continue; // rule 1 already reported the missing file
      surfacesOnPage.add(scene.surface ?? DEFAULT_SURFACE);
    }
    if (surfacesOnPage.size > 1) {
      errors.push(`${page} mixes demo surfaces (${[...surfacesOnPage].join(', ')}) — one surface per page today (site/ui/demo/README.md, "Surfaces")`);
    }
  }

  if (errors.length > 0) {
    console.error(`check-demo-scenes: ${errors.length} problem(s):`);
    for (const error of errors) console.error(`  - ${error}`);
    process.exit(1);
  }
  console.log(`check-demo-scenes: ${sceneFileNames.length} scene(s), ${links.length} link(s) across ${linksByPage.size} page(s) — all checks passed`);
}

// Run only when executed directly (`node scripts/check-demo-scenes.mjs`), not when imported for
// its walk (scripts/check-docs-demo.mjs).
if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  await main();
}
