import { test } from 'node:test';
import assert from 'node:assert/strict';
import { problems } from '../release-notes-check.mjs';

const notes = "moss 1.2.3 does a thing.\n\n## What's New\n\n- **A thing.** A pending update is shown as pending.\n";
const download = '\n## Download\n\nMac: `moss.dmg`\n';

test('real notes pass, words like "pending" included', () => {
  assert.deepEqual(problems(notes + download, 'v1.2.3'), []);
});

test('the old placeholder body fails', () => {
  const body = 'Release notes pending — run `/moss-release-notes` in Claude Code to write them above the Download section.\n' + download;
  assert.equal(problems(body, 'v1.2.3').length, 3);
});

test("a release pull request's private sections fail", () => {
  const pr = notes + '\n## Client sites\n\nA regression run.\n\n## Merging\n\nBefore merge: rebase.\n';
  assert.equal(problems(pr, 'v1.2.3').length, 3);
});

test("only an app release needs What's New", () => {
  assert.deepEqual(problems('ffmpeg 7.1 for moss.\n', 'ffmpeg-7.1'), []);
  assert.deepEqual(problems('Fixes.\n', 'v1.2.3'), ['has no "## What\'s New" section']);
});

test("a draft's untagged- name fails on any release", () => {
  assert.deepEqual(problems(notes, 'untagged-0123abcd'), ["is under a draft's placeholder tag untagged-0123abcd"]);
});
