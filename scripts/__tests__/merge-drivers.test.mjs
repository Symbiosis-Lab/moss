import { after, test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const KEYS = 'packages/moss-ui/src/ui/i18n/rust-keys.generated.json';
const BASELINE = 'scripts/ratchet-baseline.open.json';

const dirs = [];
after(() => dirs.forEach((d) => fs.rmSync(d, { recursive: true, force: true })));

// A scratch repo carrying this checkout's attributes, driver and installer, with a base commit and two branches.
function repo(base, ours, theirs) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'merge drivers-'));
  dirs.push(dir);
  const git = (...args) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', ...args], { cwd: dir, encoding: 'utf8' });
  git('init', '-q', '-b', 'base');
  const write = (files) => {
    for (const [f, body] of Object.entries(files)) {
      fs.mkdirSync(path.join(dir, path.dirname(f)), { recursive: true });
      fs.writeFileSync(path.join(dir, f), typeof body === 'string' ? body : JSON.stringify(body, null, 2) + '\n');
    }
    git('add', '-A');
  };
  for (const f of ['.gitattributes', 'scripts/merge-json.mjs', 'scripts/merge-changelog.mjs', 'scripts/install-git-hooks.mjs']) write({ [f]: fs.readFileSync(path.join(ROOT, f), 'utf8') });
  write(base); git('commit', '-qm', 'base');
  git('checkout', '-qb', 'ours'); write(ours); git('commit', '-qm', 'ours');
  git('checkout', '-q', 'base'); git('checkout', '-qb', 'theirs'); write(theirs); git('commit', '-qm', 'theirs');
  return { dir, git };
}

function mergeTree(r, install = true) {
  if (install) execFileSync('node', ['scripts/install-git-hooks.mjs'], { cwd: r.dir });
  const res = spawnSync('git', ['merge-tree', '--write-tree', 'ours', 'theirs'], { cwd: r.dir, encoding: 'utf8' });
  const tree = res.stdout.split('\n')[0];
  return { status: res.status, show: (f) => r.git('cat-file', '-p', `${tree}:${f}`) };
}

const keyFile = (chip) => ({ $comment: 'generated', chipLabelKeys: chip, fmHintKeys: ['hint.a'] });
const baseline = (rows) => ({ repo: 'open', rows: { prod_lines_per_file: { disposition: 'permanent', value: rows } } });

test('gate ports both sides add at the end are both kept', () => {
  const ports = (...k) => k.map((g) => `${g}\n`).join(''), PORTS = 'playwright/gate-ports.txt';
  const m = mergeTree(repo({ [PORTS]: ports('a') }, { [PORTS]: ports('a', 'ours') }, { [PORTS]: ports('a', 'theirs') }));
  assert.equal(m.status, 0);
  assert.equal(m.show(PORTS), ports('a', 'ours', 'theirs'));
});

test('the i18n key list merges as sets: both additions kept, a deletion kept, sorted', () => {
  const r = repo({ [KEYS]: keyFile(['chip.b', 'chip.old']) }, { [KEYS]: keyFile(['chip.b', 'chip.old', 'chip.z']) }, { [KEYS]: keyFile(['chip.a', 'chip.b']) });
  const m = mergeTree(r);
  assert.equal(m.status, 0);
  assert.equal(m.show(KEYS), JSON.stringify(keyFile(['chip.a', 'chip.b', 'chip.z']), null, 2) + '\n');
});

test('a ratchet baseline takes the larger of two changed numbers and keeps a deletion', () => {
  const r = repo(
    { [BASELINE]: baseline({ 'a.rs': 100, 'b.rs': 50, 'c.rs': 10 }) },
    { [BASELINE]: baseline({ 'a.rs': 90, 'b.rs': 50, 'c.rs': 10 }) },
    { [BASELINE]: baseline({ 'a.rs': 110, 'c.rs': 10, 'd.rs': 7 }) },
  );
  const m = mergeTree(r);
  assert.equal(m.status, 0);
  assert.deepEqual(JSON.parse(m.show(BASELINE)), baseline({ 'a.rs': 110, 'c.rs': 10, 'd.rs': 7 }));
});

test('a key one side changed and the other deleted stays a conflict', () => {
  for (const [ours, theirs] of [[{ 'a.rs': 120 }, {}], [{}, { 'a.rs': 120 }]]) {
    const r = repo({ [BASELINE]: baseline({ 'a.rs': 100 }) }, { [BASELINE]: baseline(ours) }, { [BASELINE]: baseline(theirs) });
    assert.equal(mergeTree(r).status, 1, JSON.stringify(ours));
  }
});

test('a value both sides changed that is not a number stays a conflict', () => {
  const changed = (d) => ({ [BASELINE]: { repo: 'open', rows: { prod_lines_per_file: { disposition: d, value: {} } } } });
  const m = mergeTree(repo(changed('permanent'), changed('temporary'), changed('waiting')));
  assert.equal(m.status, 1);
  assert.match(m.show(BASELINE), /^<<<<<<< ours\n[^]*"temporary"[^]*\n=======\n[^]*"waiting"[^]*\n>>>>>>> theirs$/m);
});

test('without the installer the same key lists conflict', () => {
  const r = repo({ [KEYS]: keyFile(['chip.b']) }, { [KEYS]: keyFile(['chip.b', 'chip.z']) }, { [KEYS]: keyFile(['chip.a', 'chip.b']) });
  assert.equal(mergeTree(r, false).status, 1);
});

const log = (...sections) => `# Changelog\n\n## [Unreleased]\n\n${sections.map(([h, ...items]) => `### ${h}\n\n${items.map((i) => `- ${i}\n`).join('')}`).join('\n')}\n## [0.1.0]\n\n- first\n`;

test('two branches that each add a Fixed section get one heading, theirs after ours', () => {
  const r = repo(
    { 'CHANGELOG.md': log(['New', 'n']) },
    { 'CHANGELOG.md': log(['New', 'n'], ['Fixed', 'ours one', 'ours two']) },
    { 'CHANGELOG.md': log(['New', 'n', 'theirs new'], ['Changed', 'theirs changed'], ['Fixed', 'theirs']) },
  );
  const m = mergeTree(r);
  assert.equal(m.status, 0);
  assert.equal(m.show('CHANGELOG.md'), log(['New', 'n', 'theirs new'], ['Changed', 'theirs changed'], ['Fixed', 'ours one', 'ours two', 'theirs']));
});

test('a changelog entry one side deleted stays deleted, and an emptied section goes', () => {
  const r = repo(
    { 'CHANGELOG.md': log(['New', 'n'], ['Fixed', 'old']) },
    { 'CHANGELOG.md': log(['New', 'n', 'ours']) },
    { 'CHANGELOG.md': log(['New', 'n'], ['Fixed', 'old', 'theirs']) },
  );
  const m = mergeTree(r);
  assert.equal(m.status, 0);
  assert.equal(m.show('CHANGELOG.md'), log(['New', 'n', 'ours'], ['Fixed', 'theirs']));
});

test('both sides adding the same heading, in a nested changelog, get one heading', () => {
  const f = 'crates/a/CHANGELOG.md';
  const m = mergeTree(repo({ [f]: log(['Added', 'a']) }, { [f]: log(['Added', 'a'], ['Fixed', 'ours']) }, { [f]: log(['Added', 'a'], ['Fixed', 'theirs']) }));
  assert.equal(m.status, 0);
  assert.equal(m.show(f), log(['Added', 'a'], ['Fixed', 'ours', 'theirs']));
});

test('a wrapped entry moves whole', () => {
  const wrapped = 'theirs, wrapped\n  onto a second line';
  const m = mergeTree(repo({ 'CHANGELOG.md': log(['New', 'n']) }, { 'CHANGELOG.md': log(['New', 'n', 'ours']) }, { 'CHANGELOG.md': log(['New', 'n', wrapped]) }));
  assert.equal(m.status, 0);
  assert.equal(m.show('CHANGELOG.md'), log(['New', 'n', 'ours', wrapped]));
});

test('a release cut on one side keeps the other side\'s new entry in Unreleased', () => {
  const cut = log().replace('## [0.1.0]', '## [0.2.0]\n\n### New\n\n- n\n\n## [0.1.0]');
  const m = mergeTree(repo({ 'CHANGELOG.md': log(['New', 'n']) }, { 'CHANGELOG.md': log(['New', 'n', 'ours']) }, { 'CHANGELOG.md': cut }));
  assert.equal(m.status, 0);
  assert.equal(m.show('CHANGELOG.md'), log(['New', 'ours']).replace('## [0.1.0]', '## [0.2.0]\n\n### New\n\n- n\n\n## [0.1.0]'));
});

test('a released section both sides changed stays a conflict', () => {
  const r = repo(
    { 'CHANGELOG.md': log(['New', 'n']) },
    { 'CHANGELOG.md': log(['New', 'n']).replace('- first', '- first, ours') },
    { 'CHANGELOG.md': log(['New', 'n']).replace('- first', '- first, theirs') },
  );
  const m = mergeTree(r);
  assert.equal(m.status, 1);
  assert.match(m.show('CHANGELOG.md'), /^<<<<<<< ours\n- first, ours\n=======\n- first, theirs\n>>>>>>> theirs$/m);
});

test('an entry both sides reworded stays a conflict', () => {
  const m = mergeTree(repo({ 'CHANGELOG.md': log(['Fixed', 'x old']) }, { 'CHANGELOG.md': log(['Fixed', 'x by ours']) }, { 'CHANGELOG.md': log(['Fixed', 'x by theirs']) }));
  assert.equal(m.status, 1);
  assert.match(m.show('CHANGELOG.md'), /^<<<<<<< ours\n- x by ours\n=======\n- x by theirs\n>>>>>>> theirs$/m);
});

test('an entry one side reworded and the other deleted keeps the rewording, so no text is lost', () => {
  const m = mergeTree(repo({ 'CHANGELOG.md': log(['New', 'n'], ['Fixed', 'x old']) }, { 'CHANGELOG.md': log(['New', 'n'], ['Fixed', 'x by ours']) }, { 'CHANGELOG.md': log(['New', 'n']) }));
  assert.equal(m.status, 0);
  assert.equal(m.show('CHANGELOG.md'), log(['New', 'n'], ['Fixed', 'x by ours']));
});

test('a line before the first ### heading stays a conflict', () => {
  const loose = (t) => log(['New', 'n']).replace('## [Unreleased]\n\n', `## [Unreleased]\n\n${t}\n\n`);
  const m = mergeTree(repo({ 'CHANGELOG.md': log(['New', 'n']) }, { 'CHANGELOG.md': loose('ours note') }, { 'CHANGELOG.md': log(['New', 'n', 'theirs']) }));
  assert.equal(m.status, 1);
});

test("every changelog in this repo merges with itself unchanged", () => {
  const files = execFileSync('git', ['ls-files', '*CHANGELOG.md'], { cwd: ROOT, encoding: 'utf8' }).trim().split('\n');
  for (const f of files) {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'merge-changelog-'));
    const body = fs.readFileSync(path.join(ROOT, f), 'utf8');
    for (const n of ['o', 'a', 'b']) fs.writeFileSync(path.join(dir, n), body);
    const res = spawnSync('node', [path.join(ROOT, 'scripts/merge-changelog.mjs'), ...['o', 'a', 'b'].map((n) => path.join(dir, n))]);
    assert.equal(res.status, 0, f);
    assert.equal(fs.readFileSync(path.join(dir, 'a'), 'utf8'), body, f);
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
