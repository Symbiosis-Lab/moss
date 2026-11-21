// A gate is skipped only when every input its green run recorded is unchanged.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { check, record, inputsFromLog, specPattern } from '../render-gate-select.mjs';

const RECORDER = fileURLToPath(new URL('../../tests/support/gate-inputs.cjs', import.meta.url));

function tree() {
  // Real, as process.cwd() and the static server's getcwd() see it (macOS's /var is a link).
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'gate-select-')));
  const w = (rel, text) => {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
    fs.writeFileSync(path.join(root, rel), text);
  };
  w('spec.ts', 'spec');
  w('assets/site.css', 'a { color: red }');
  w('assets/icons/a.svg', '<svg/>');
  const built = path.join(root, 'target/test-tmp/demo/.moss/build');
  w('target/test-tmp/demo/.moss/build/index.html', '<p>one</p>');
  w('target/test-tmp/demo/.moss/build-stamp.json', JSON.stringify({ stamp: 'x', builtDir: built }));
  w('bin/moss-cli', 'binary one');
  w('playwright/demo-gate.config.ts', "defineGateConfig({ gate: 'demo-gate' })");
  const env = { MOSS_RENDER_GATE_CACHE: path.join(root, 'cache') };
  const mossBin = path.join(root, 'bin/moss-cli');
  return { root, w, env, mossBin };
}

// What the recorder writes for a gate that reads its spec and a stylesheet,
// lists a folder, probes a file that does not exist, opens a scratch site and
// asks moss-cli for its token table.
function greenLog(root, mossBin) {
  return [
    `read\t${root}/playwright/demo-gate.config.ts`,
    `read\t${root}/spec.ts`,
    `read\t${root}/assets/site.css`,
    `dir\t${root}/assets/icons`,
    `exists\t${root}/assets/override.css`,
    `read\t${root}/target/test-tmp/demo/.moss/build-stamp.json`,
    `read\t${root}/target/test-tmp/demo/.moss/build/index.html`,
    `read\t${root}/node_modules/some-dep/index.js`,
    `read\t/somewhere/else/entirely.txt`,
    `exec\t"${mossBin}" build "${root}/target/test-tmp/demo" --no-plugins`,
    `exec\t"${mossBin}" describe --json`,
  ].join('\n');
}

test('the input set keeps tree files, folds a scratch site into its built tree, drops node_modules and outside paths', () => {
  const { root, mossBin } = tree();
  assert.deepEqual(inputsFromLog(root, greenLog(root, mossBin), mossBin), [
    ['bin', ''],
    ['dir', 'assets/icons'],
    ['exists', 'assets/override.css'],
    ['read', 'assets/site.css'],
    ['read', 'playwright/demo-gate.config.ts'],
    ['read', 'spec.ts'],
    ['site', 'demo'],
  ]);
});

test('a walk under target/ and a Node builtin are not inputs; a file read under target/ still is', () => {
  // A Vite harness lists the whole tree, cargo's target/ included, and Node probes `node:` names it loads lazily.
  const { root, w } = tree();
  w('Cargo.toml', '[workspace]');
  w('crates/core/Cargo.toml', '[package]');
  const log = [
    `exists\tnode:events`,
    `dir\t${root}/docs/target`,
    `dir\t${root}/target/debug/.fingerprint`,
    `exists\t${root}/target/debug/deps/a.d`,
    `dir\t${root}/target/test-tmp.old/demo`,
    `exists\t${root}/crates/core/target/test-tmp/scratch/x`,
    `read\t${root}/target/debug/notes.txt`,
  ].join('\n');
  // Only cargo's target folders, the ones beside a Cargo.toml: a docs folder named target is listed as usual.
  assert.deepEqual(inputsFromLog(root, log), [['dir', 'docs/target'], ['read', 'target/debug/notes.txt']]);
});

test('a scratch build alone does not make the gate depend on the binary', () => {
  const { root, mossBin } = tree();
  const log = `exec\t"${mossBin}" build "${root}/target/test-tmp/demo" --no-plugins`;
  assert.deepEqual(inputsFromLog(root, log, mossBin), []);
  const serve = `exec\t"${mossBin}" build "${root}/site" --serve --no-plugins`;
  assert.deepEqual(inputsFromLog(root, serve, mossBin), [['bin', '']]);
});

const CHANGES = {
  'a file it read changes': ({ w }) => w('assets/site.css', 'a { color: blue }'),
  'a folder it listed gains a file': ({ w }) => w('assets/icons/b.svg', '<svg/>'),
  'a file it found missing appears': ({ w }) => w('assets/override.css', ''),
  'its scratch site builds differently': ({ w }) => w('target/test-tmp/demo/.moss/build/index.html', '<p>two</p>'),
  'its scratch site gains a page': ({ w }) => w('target/test-tmp/demo/.moss/build/more.html', ''),
  'the binary it starts changes': ({ w }) => w('bin/moss-cli', 'binary two'),
  'the lockfile changes': ({ w }) => w('pnpm-lock.yaml', 'lockfileVersion: 9'),
};

for (const [what, change] of Object.entries(CHANGES)) {
  test(`a green gate runs again when ${what}`, () => {
    const t = tree();
    record(t.root, 'demo-gate', '', greenLog(t.root, t.mossBin), { env: t.env, mossBin: t.mossBin });
    assert.ok(check(t.root, 'demo-gate', '', { env: t.env, mossBin: t.mossBin }), 'unchanged inputs skip');
    change(t);
    assert.equal(check(t.root, 'demo-gate', '', { env: t.env, mossBin: t.mossBin }), null);
  });
}

test('a change to a file the gate never read leaves it skipped', () => {
  const t = tree();
  record(t.root, 'demo-gate', '', greenLog(t.root, t.mossBin), { env: t.env, mossBin: t.mossBin });
  t.w('unrelated.md', 'docs');
  assert.ok(check(t.root, 'demo-gate', '', { env: t.env, mossBin: t.mossBin }));
});

test('a green entry is per project, and a gate never recorded runs', () => {
  const t = tree();
  record(t.root, 'demo-gate', 'chromium', greenLog(t.root, t.mossBin), { env: t.env, mossBin: t.mossBin });
  assert.equal(check(t.root, 'demo-gate', '', { env: t.env, mossBin: t.mossBin }), null);
  assert.equal(check(t.root, 'other-gate', 'chromium', { env: t.env, mossBin: t.mossBin }), null);
  assert.ok(check(t.root, 'demo-gate', 'chromium', { env: t.env, mossBin: t.mossBin }));
});

test('an earlier green state still skips after the tree returns to it', () => {
  const t = tree();
  const opts = { env: t.env, mossBin: t.mossBin };
  record(t.root, 'demo-gate', '', greenLog(t.root, t.mossBin), opts);
  t.w('assets/site.css', 'a { color: blue }');
  record(t.root, 'demo-gate', '', greenLog(t.root, t.mossBin), opts);
  t.w('assets/site.css', 'a { color: red }');
  assert.ok(check(t.root, 'demo-gate', '', opts));
});

test('the recorder logs what a Node process reads, lists, probes and starts', () => {
  const t = tree();
  const out = path.join(t.root, 'inputs.log');
  const script = [
    "const fs = require('fs');",
    `fs.readFileSync(${JSON.stringify(path.join(t.root, 'spec.ts'))});`,
    `fs.readdirSync(${JSON.stringify(path.join(t.root, 'assets/icons'))});`,
    `fs.existsSync(${JSON.stringify(path.join(t.root, 'assets/override.css'))});`,
    "require('child_process').execFileSync('true', ['--flag']);",
  ].join('\n');
  execFileSync(process.execPath, ['-e', script], {
    env: { ...process.env, GATE_INPUTS_OUT: out, NODE_OPTIONS: `--require ${RECORDER}` },
  });
  const lines = fs.readFileSync(out, 'utf8').split('\n');
  for (const want of [
    `read\t${t.root}/spec.ts`,
    `dir\t${t.root}/assets/icons`,
    `exists\t${t.root}/assets/override.css`,
    'exec\ttrue --flag',
  ]) assert.ok(lines.includes(want), `missing ${want}`);
});

// Playwright walks the gate's whole test folder to find its spec.
function discoveryTree() {
  const t = tree();
  t.w('tests/render-gates/site/demo-gate.spec.ts', 'spec');
  t.w('tests/render-gates/site/other-gate.spec.ts', 'spec');
  const log = [`read\t${t.root}/playwright/demo-gate.config.ts`, `dir\t${t.root}/tests/render-gates/site`, `read\t${t.root}/tests/render-gates/site/demo-gate.spec.ts`].join('\n');
  const opts = { env: t.env, mossBin: t.mossBin };
  record(t.root, 'demo-gate', '', log, opts);
  return { ...t, opts };
}

test('another gate\'s new spec in the same folder leaves a green gate skipped', () => {
  const t = discoveryTree();
  t.w('tests/render-gates/site/third-gate.spec.ts', 'spec');
  t.w('tests/render-gates/site/helper.ts', 'export {}');
  assert.ok(check(t.root, 'demo-gate', '', t.opts));
});

test('a new file the gate\'s testMatch accepts, or a new subfolder, runs it again', () => {
  const a = discoveryTree();
  a.w('tests/render-gates/site/x-demo-gate.spec.ts', 'spec');
  assert.equal(check(a.root, 'demo-gate', '', a.opts), null);
  const b = discoveryTree();
  b.w('tests/render-gates/site/nested/readme.md', '');
  assert.equal(check(b.root, 'demo-gate', '', b.opts), null);
});

test('a config naming its own spec is matched by that name', () => {
  const t = tree();
  t.w('playwright/site-elevation.config.ts', "defineGateConfig({ gate: 'site-elevation', specName: 'elevation' })");
  assert.equal(specPattern(t.root, 'site-elevation'), 'elevation\\.spec\\.ts$');
  assert.equal(specPattern(t.root, 'card-cover-fit'), 'card-cover-fit\\.spec\\.ts$');
});

test('a run that recorded nothing is not stored, so it cannot match forever', () => {
  const t = tree();
  assert.throws(() => record(t.root, 'demo-gate', '', '', { env: t.env, mossBin: t.mossBin }), /never read/);
  assert.equal(check(t.root, 'demo-gate', '', { env: t.env, mossBin: t.mossBin }), null);
});

test('a file the browser loads through the static server is an input', async () => {
  const t = tree();
  t.w('fixtures/page.html', '<script src="/assets/bridge.js"></script>');
  t.w('assets/bridge.js', 'one');
  const out = path.join(t.root, 'inputs.log');
  const server = fileURLToPath(new URL('../../tests/support/static-server.py', import.meta.url));
  const { spawn } = await import('node:child_process');
  const port = 20000 + (process.pid % 20000);
  const child = spawn('/usr/bin/python3', [server, String(port)], {
    cwd: t.root, env: { ...process.env, GATE_INPUTS_OUT: out }, stdio: 'ignore',
  });
  try {
    let page;
    for (let i = 0; i < 50 && !page; i++) {
      page = await fetch(`http://localhost:${port}/fixtures/page.html`).catch(() => new Promise((r) => setTimeout(r, 100)));
    }
    assert.equal(await page.text(), '<script src="/assets/bridge.js"></script>');
    assert.equal(await (await fetch(`http://localhost:${port}/assets/bridge.js`)).text(), 'one');
  } finally {
    child.kill();
  }
  const log = `read\t${t.root}/playwright/demo-gate.config.ts\n` + fs.readFileSync(out, 'utf8');
  record(t.root, 'demo-gate', '', log, { env: t.env });
  assert.ok(check(t.root, 'demo-gate', '', { env: t.env }), 'unchanged: skipped');
  t.w('assets/bridge.js', 'two');
  assert.equal(check(t.root, 'demo-gate', '', { env: t.env }), null, 'a served script changed: runs');
});

test('a site whose rebuild fails is not taken for its last green build', async () => {
  const t = tree();
  // Fails when the spec names a file called `fail`, else builds one page and reports it as `build --json` does.
  t.w('bin/fake-moss', '#!/bin/sh\n[ -e "$2/fail" ] && exit 1\nmkdir -p "$2/.moss/build.nosync/staging"\necho built > "$2/.moss/build.nosync/staging/index.html"\necho "{\\"output\\":\\"$2/.moss/build.nosync/staging\\"}"\n');
  fs.chmodSync(path.join(t.root, 'bin/fake-moss'), 0o755);
  const { build } = await import('esbuild');
  // Bundled into the temporary tree, so the helper scaffolds its sites there.
  const helper = path.join(t.root, 'tests/e2e/helpers/scratch-site.mjs');
  await build({
    entryPoints: [fileURLToPath(new URL('../../tests/e2e/helpers/scratch-site.ts', import.meta.url))],
    bundle: true, platform: 'node', format: 'esm', outfile: helper, logLevel: 'silent',
  });
  process.env.MOSS_BIN = path.join(t.root, 'bin/fake-moss');
  const { buildScratchSite } = await import(`file://${helper}`);
  delete process.env.MOSS_BIN;

  buildScratchSite({ name: 'rebuilt', files: { 'index.md': '# one' } });
  const log = `read\t${t.root}/playwright/demo-gate.config.ts\nread\t${t.root}/target/test-tmp/rebuilt/.moss/build-stamp.json`;
  record(t.root, 'demo-gate', '', log, { env: t.env });
  assert.ok(check(t.root, 'demo-gate', '', { env: t.env }), 'unchanged: skipped');
  assert.throws(() => buildScratchSite({ name: 'rebuilt', files: { 'index.md': '# two', fail: '' } }));
  assert.equal(check(t.root, 'demo-gate', '', { env: t.env }), null, 'the rebuild failed: runs');
});
