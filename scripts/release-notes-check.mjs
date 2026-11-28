#!/usr/bin/env node
// Refuses a release body that is not real notes.
//
//   node scripts/release-notes-check.mjs check <file> <tag>
//
// An app release (vX.Y.Z) must have "## What's New"; every release is refused
// on the strings below or a draft's untagged- name. Matching is anchored:
// bare "pending" or "Claude Code" occur in real notes.

import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

const FORBIDDEN = [
  [/^## Client sites\b/m, 'has a "## Client sites" section (client names are not public)'],
  [/^## Merging\b/m, 'has a "## Merging" section (it belongs to the release pull request)'],
  [/Before merge/, 'has the release pull request\'s "Before merge" text'],
  [/Release notes pending/, 'is a placeholder'],
  [/\/moss-release-notes/, 'tells the reader to run /moss-release-notes'],
];

export function problems(body, tag) {
  const found = [];
  if (tag.startsWith('untagged-')) found.push(`is under a draft's placeholder tag ${tag}`);
  if (/^v\d+\.\d+\.\d+$/.test(tag) && !/^## What's New$/m.test(body)) found.push('has no "## What\'s New" section');
  for (const [re, why] of FORBIDDEN) if (re.test(body)) found.push(why);
  return found;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [cmd, file, tag] = process.argv.slice(2);
  if (cmd !== 'check' || !file || !tag) {
    console.error('usage: release-notes-check.mjs check <file> <tag>');
    process.exit(2);
  }
  if (!fs.existsSync(file)) {
    console.error(`::error::${tag}: ${file} is missing`);
    process.exit(1);
  }
  const list = problems(fs.readFileSync(file, 'utf8'), tag);
  for (const p of list) console.error(`::error::${tag}: the release body ${p}`);
  if (list.length) process.exit(1);
  console.log(`${tag}: release notes ok`);
}
