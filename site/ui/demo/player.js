// Runs a scene's `steps` against a driver. Knows nothing about the host page, iframes, layout,
// or UI strings — see site/ui/demo/README.md for the module boundaries.
//
// A driver is any object shaped like:
//   typeInto(targetName, text, { signal }) -> Promise<boolean>   animated typing into a named input; resolves false if interrupted
//   setTextInto(targetName, text) -> void | Promise<void>    instant equivalent
//   click(targetName, { signal }) -> Promise<boolean>   animated click; resolves false if interrupted
//   clickInstant(targetName, { signal }) -> void | Promise<void>    instant equivalent
//   context(targetName, { signal }) -> Promise<boolean>   animated right-click; resolves false if interrupted
//   contextInstant(targetName, { signal }) -> void | Promise<void>    instant equivalent
//   dblclick(targetName, { signal }) -> Promise<boolean>   animated double-click; resolves false if interrupted
//   dblclickInstant(targetName, { signal }) -> void | Promise<void>    instant equivalent
// `player.js` never touches `scene.fixture` or `scene.open` — loading a scene's opening state is
// the stage's job (it owns the iframe and the fixture). This module only plays `scene.steps`.

/** One entry per verb a scene step can use. Adding a verb is adding an entry here. Each `run`
 * takes `reveals` last and forwards it to the driver untouched — player.js knows only that a step
 * may carry it, never what it means beyond that (site/ui/demo/README.md, "Pace"); the driver alone owns
 * the formula reveals feeds. */
const VERBS = {
  // Types into a named input (site/ui/demo/README.md, "Scene format") — `{ "typeInto": { "target": "...",
  // "text": "..." } }` — for the site's own `<input>` fields (a rename prompt, a properties search
  // box) that aren't the harvested editor's own typing surface (driver.js).
  typeInto: {
    run: (driver, arg, locale, signal) => driver.typeInto(arg.target, resolveLocale(arg.text, locale), { signal }),
    instant: (driver, arg, locale) => driver.setTextInto(arg.target, resolveLocale(arg.text, locale)),
  },
  click: {
    run: (driver, arg, locale, signal, reveals) => driver.click(arg, { signal, reveals }),
    instant: (driver, arg, locale, signal) => driver.clickInstant(arg, { signal }),
  },
  // A right-click — how the harvested app's own menus open (site/ui/demo/README.md, "Scene format").
  context: {
    run: (driver, arg, locale, signal, reveals) => driver.context(arg, { signal, reveals }),
    instant: (driver, arg, locale, signal) => driver.contextInstant(arg, { signal }),
  },
  // A double-click — how the harvested app's tree divider collapses/reopens the tree
  // (gestures.md, "Collapse the tree").
  dblclick: {
    run: (driver, arg, locale, signal, reveals) => driver.dblclick(arg, { signal, reveals }),
    instant: (driver, arg, locale, signal) => driver.dblclickInstant(arg, { signal }),
  },
};

// Step keys recognized alongside the one verb key, not counted against the "exactly one verb"
// rule below. `reveals` is the only one today (site/ui/demo/README.md, "Pace": `{ "context": "crumb.file",
// "reveals": 10 }`).
const NON_VERB_STEP_KEYS = new Set(['reveals']);

/** Resolves a step argument that is either a plain string or a per-locale `{en, "zh-hant", ...}` object. */
export function resolveLocale(value, locale) {
  if (typeof value === 'string') return value;
  if (value && typeof value === 'object') return value[locale] ?? value.en ?? '';
  return '';
}

/**
 * Plays `scene.steps` in order against `driver`.
 * Returns `'done'` once every step completes, or `'paused'` the instant `signal` aborts or a step
 * itself reports it did not finish (the reader interrupted it). `instant` skips every verb's
 * animated path — used under `prefers-reduced-motion` and for the unit-test fast path.
 */
export async function play(scene, driver, { locale = 'en', instant = false, signal } = {}) {
  for (const step of scene.steps) {
    if (signal?.aborted) return 'paused';
    const verbKeys = Object.keys(step).filter((key) => !NON_VERB_STEP_KEYS.has(key));
    if (verbKeys.length !== 1) throw new Error(`Scene step must have exactly one verb: ${JSON.stringify(step)}`);
    const [verb] = verbKeys;
    const handler = VERBS[verb];
    if (!handler) throw new Error(`Unknown scene verb: "${verb}"`);
    const phase = instant ? handler.instant : handler.run;
    const outcome = await phase(driver, step[verb], locale, signal, step.reveals);
    if (signal?.aborted || outcome === false) return 'paused';
  }
  return 'done';
}

/**
 * Plays an `after` chain (site/ui/demo/README.md, "Scene format"): every entry but the last runs through
 * `play`'s existing instant mode — no second playback mechanism — and only the last, the scene
 * actually requested, plays under `instant` as given, so reduced motion still applies to it like
 * any other scene. `chain` is ordered eldest ancestor first, the requested scene last; resolving
 * `after` names into that order — following the chain, fetching each scene's data — is scene
 * loading and lives in demo-frame.js, not here (see the module table). This only runs what it is
 * handed, refusing a chain that repeats a name as a defensive, cheaply-testable last check (a real
 * `after` cycle is rejected earlier, while demo-frame.js is still fetching scene data, so it never
 * has to fetch forever).
 */
export async function playChain(chain, driver, { locale = 'en', instant = false, signal } = {}) {
  const seen = new Set();
  for (const { name } of chain) {
    if (seen.has(name)) throw new Error(`Scene "${name}" appears twice in an "after" chain — cycle rejected`);
    seen.add(name);
  }
  for (let i = 0; i < chain.length; i++) {
    const isTarget = i === chain.length - 1;
    const outcome = await play(chain[i].scene, driver, { locale, instant: isTarget ? instant : true, signal });
    if (outcome !== 'done') return outcome;
  }
  return 'done';
}
