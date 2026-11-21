#!/usr/bin/env node
// Git merge driver for the JSON files whose conflicts are mechanical; .gitattributes names them and
// scripts/install-git-hooks.mjs registers the driver.
//   node scripts/merge-json.mjs <keys|ratchet> <base> <ours> <theirs>
// Each key merges three ways, so a key one side deleted stays deleted, unless the other side changed it. keys: every array is a set of strings,
// merged the same way and sorted. ratchet: a number both sides changed takes the larger; `ratchet.mjs check`
// re-measures it. Writes the result into <ours>, as git expects, and exits 1 on anything else both sides
// changed, leaving git's conflict markers in <ours> for a person.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';

class Conflict extends Error {}

const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const isObject = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);

function mergeSet(base, ours, theirs) {
  const b = new Set(Array.isArray(base) ? base : []), o = new Set(ours), t = new Set(theirs);
  const kept = [...new Set([...ours, ...theirs])].filter((x) => (o.has(x) && t.has(x)) || !b.has(x));
  return kept.sort();
}

function merge(mode, base, ours, theirs) {
  if (isObject(ours) && isObject(theirs)) {
    const b = isObject(base) ? base : {}, out = {};
    for (const k of [...Object.keys(ours), ...Object.keys(theirs).filter((k) => !(k in ours))]) {
      if (k in ours && k in theirs) out[k] = merge(mode, b[k], ours[k], theirs[k]);
      else if (!(k in b)) out[k] = k in ours ? ours[k] : theirs[k];
      else if (!same(b[k], k in ours ? ours[k] : theirs[k])) throw new Conflict();
    }
    return out;
  }
  if (same(ours, theirs)) return ours;
  if (same(ours, base)) return theirs;
  if (same(theirs, base)) return ours;
  if (mode === 'keys' && Array.isArray(ours) && Array.isArray(theirs)) return mergeSet(base, ours, theirs);
  if (mode === 'ratchet' && typeof ours === 'number' && typeof theirs === 'number') return Math.max(ours, theirs);
  throw new Conflict();
}

const [mode, basePath, oursPath, theirsPath] = process.argv.slice(2);
if (!['keys', 'ratchet'].includes(mode) || !theirsPath) {
  console.error('usage: merge-json.mjs <keys|ratchet> <base> <ours> <theirs>');
  process.exit(2);
}
try {
  const raw = fs.readFileSync(oursPath, 'utf8');
  const read = (p) => { const s = fs.readFileSync(p, 'utf8'); return s.trim() ? JSON.parse(s) : {}; };
  const out = merge(mode, read(basePath), JSON.parse(raw), read(theirsPath));
  const indent = raw.match(/^\{\n( +)"/)?.[1].length;
  fs.writeFileSync(oursPath, JSON.stringify(out, null, indent) + (raw.endsWith('\n') ? '\n' : ''));
} catch (e) {
  if (!(e instanceof Conflict || e instanceof SyntaxError)) throw e;
  spawnSync('git', ['merge-file', '-L', 'ours', '-L', 'base', '-L', 'theirs', oursPath, basePath, theirsPath]);
  process.exit(1);
}
