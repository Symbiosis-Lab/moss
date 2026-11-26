#!/usr/bin/env node
// Git merge driver for CHANGELOG.md; .gitattributes names it and scripts/install-git-hooks.mjs registers it.
//   node scripts/merge-changelog.mjs <base> <ours> <theirs>
// The [Unreleased] block merges heading by heading: each `###` section keeps ours' entries, drops an entry the
// other side deleted, and adds theirs' new ones after, so two branches that each add a `### Fixed` section get
// one heading. A line-based union writes that heading twice when one side also adds a section above it.
// An entry both sides reworded, and anything outside the block both sides changed, stays a conflict: exit 1,
// with git's ordinary markers in ours.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';

class Conflict extends Error {}

// pre ends with the [Unreleased] heading line, post starts at the next `## ` heading.
function split(text) {
  const lines = text.split('\n');
  const start = lines.findIndex((l) => /^## \[?Unreleased\]?\s*$/i.test(l));
  if (start < 0) return { pre: text, sections: null, post: '' };
  let end = lines.findIndex((l, i) => i > start && l.startsWith('## '));
  if (end < 0) end = lines.length;
  const sections = new Map();
  let cur = null;
  for (const line of lines.slice(start + 1, end)) {
    if (line.startsWith('### ')) sections.set((cur = line.trim()), sections.get(line.trim()) ?? []);
    else if (!line.trim()) continue;
    else if (!cur) throw new Conflict();
    else if (/^[-*] /.test(line)) sections.get(cur).push(line);
    else {
      const items = sections.get(cur);
      if (!items.length) throw new Conflict();
      items[items.length - 1] += `\n${line}`;
    }
  }
  return { pre: lines.slice(0, start + 1).join('\n'), sections, post: lines.slice(end).join('\n') };
}

function pick(base, ours, theirs) {
  if (ours === theirs || theirs === base) return ours;
  if (ours === base) return theirs;
  throw new Conflict();
}

// A rewording is a deletion plus an addition, so a base entry gone from both sides while each side added one
// is read as both sides rewording it.
function mergeItems(base = [], ours = [], theirs = []) {
  const b = new Set(base), t = new Set(theirs), o = new Set(ours);
  const added = (side) => side.some((x) => !b.has(x));
  if (base.some((x) => !o.has(x) && !t.has(x)) && added(ours) && added(theirs)) throw new Conflict();
  return [...ours.filter((x) => t.has(x) || !b.has(x)), ...theirs.filter((x) => !o.has(x) && !b.has(x))];
}

// Ours' headings in order; a heading only theirs has goes before the next one both have, as it sits in theirs.
function headings(ours, theirs) {
  const out = [...ours];
  theirs.forEach((h, i) => {
    if (out.includes(h)) return;
    const next = theirs.slice(i + 1).find((x) => out.includes(x));
    out.splice(next ? out.indexOf(next) : out.length, 0, h);
  });
  return out;
}

function merge(baseText, oursText, theirsText) {
  const base = split(baseText), ours = split(oursText), theirs = split(theirsText);
  if (!ours.sections || !theirs.sections) throw new Conflict();
  const b = base.sections ?? new Map();
  const body = [];
  for (const h of headings([...ours.sections.keys()], [...theirs.sections.keys()])) {
    const items = mergeItems(b.get(h), ours.sections.get(h), theirs.sections.get(h));
    if (items.length) body.push(`${h}\n\n${items.join('\n')}\n`);
  }
  const pre = pick(base.pre, ours.pre, theirs.pre), post = pick(base.post, ours.post, theirs.post);
  const block = body.join('\n');
  return post ? `${pre}\n\n${block}${block && '\n'}${post}` : `${pre}\n\n${block}`;
}

const [basePath, oursPath, theirsPath] = process.argv.slice(2);
if (!theirsPath) {
  console.error('usage: merge-changelog.mjs <base> <ours> <theirs>');
  process.exit(2);
}
try {
  const read = (p) => fs.readFileSync(p, 'utf8');
  fs.writeFileSync(oursPath, merge(read(basePath), read(oursPath), read(theirsPath)));
} catch (e) {
  if (!(e instanceof Conflict)) throw e;
  spawnSync('git', ['merge-file', '-L', 'ours', '-L', 'base', '-L', 'theirs', oursPath, basePath, theirsPath]);
  process.exit(1);
}
