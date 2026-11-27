// The scratch-site prune runs while other gates may be building the same site.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pruneUnnamed } from '../../tests/e2e/helpers/prune-unnamed.mjs';

function site() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'scratch-prune-'));
  for (const rel of ['index.md', 'places/a.md', 'places/old.md', '.moss/cache/objects/3c/blob', 'Icon\r']) {
    fs.mkdirSync(path.dirname(path.join(dir, rel)), { recursive: true });
    fs.writeFileSync(path.join(dir, rel), 'x');
  }
  return dir;
}

test('removes only the files the spec does not name', () => {
  const dir = site();
  assert.equal(pruneUnnamed(dir, ['index.md', 'places/a.md']), true);
  assert.deepEqual(fs.readdirSync(path.join(dir, 'places')), ['a.md']);
  assert.ok(fs.existsSync(path.join(dir, 'Icon\r')));
  assert.equal(pruneUnnamed(dir, ['index.md', 'places/a.md']), false);
});

// Another process's build deletes .moss/cache during the walk; a walk that enters
// it fails on the missing directory. An unreadable one fails the same way, every time.
test('never enters a dot directory', { skip: process.getuid?.() === 0 && 'root reads any directory' }, () => {
  const dir = site();
  const cache = path.join(dir, '.moss/cache');
  fs.chmodSync(cache, 0);
  try {
    assert.equal(pruneUnnamed(dir, ['index.md', 'places/a.md', 'places/old.md']), false);
  } finally {
    fs.chmodSync(cache, 0o755);
  }
});

test('removes a theme stylesheet the spec no longer names', () => {
  const dir = site();
  const sheet = path.join(dir, '.moss/theme/style.css');
  fs.mkdirSync(path.dirname(sheet), { recursive: true });
  fs.writeFileSync(sheet, 'x');
  assert.equal(pruneUnnamed(dir, ['index.md', 'places/a.md', 'places/old.md']), true);
  assert.ok(!fs.existsSync(sheet));
  assert.ok(fs.existsSync(path.join(dir, '.moss/cache/objects/3c/blob')));
});
