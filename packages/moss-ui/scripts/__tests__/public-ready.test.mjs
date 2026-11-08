import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { check, commentBlocks, commentLines, loadPrivateRules, pathResolver, scanFile } from '../public-ready.mjs';

let root;
const put = (rel, text = '') => {
  mkdirSync(dirname(join(root, rel)), { recursive: true });
  writeFileSync(join(root, rel), text);
};
const TRACKED = [
  'packages/moss-ui/src/ui/shared/toast.ts',
  'docs/README.md',
  'crates/moss-build/src/js-src/site/x.ts',
  'README.md',
  'fixtures/a/style.css',
  'fixtures/b/style.css',
];
const rules = (file, source, priv = null) =>
  scanFile(file, source, priv, pathResolver(TRACKED)).map((h) => `${h.rule} ${h.match}`);

beforeEach(() => { root = mkdtempSync(join(tmpdir(), 'public-ready-')); });
afterEach(() => rmSync(root, { recursive: true, force: true }));

describe('commentLines', () => {
  it('reads line and block comments, not code or strings', () => {
    const src = ["const url = 'https://x.test/a'; // a note", '/* one', ' * two */ const y = 1;', 'const s = "// not a comment";'].join('\n');
    expect(commentLines(src, '.ts').map(([n, t]) => [n, t.trim()])).toEqual([[1, 'a note'], [2, 'one'], [3, '* two']]);
  });
  it('skips a regex literal, whose quotes would hide the comments after it', () => {
    const src = ["const re = /([\"\\\\])/g; // first", "const t = 'x'; // second"].join('\n');
    expect(commentLines(src, '.ts').map(([, t]) => t.trim())).toEqual(['first', 'second']);
  });
  it('keeps a template literal open across lines', () => {
    const src = ['const html = `', "  <a href='//x'>don't</a>", '`; // after'].join('\n');
    expect(commentLines(src, '.ts').map(([, t]) => t.trim())).toEqual(['after']);
  });
  it('reads only block comments in a stylesheet', () => {
    expect(commentLines('a { background: url(//cdn/x.png); } /* why */', '.css').map(([, t]) => t.trim())).toEqual(['why']);
  });
});

describe('history in comments and test titles', () => {
  it.each([
    // Split so the repo's doc-link check does not take the fixture for a link.
    [`// see docs${'/'}archive/x.md`, [`doc-path docs${'/'}archive/x.md`, `unknown-path docs${'/'}archive/x.md`]],
    ['// fixed on 2026-01-01', ['date 2026-01-01']],
    ['// Ref: https://codemirror.net/docs/ref/#view.EditorView', []],
    ['// per ADR-022', ['adr ADR-022']],
    ['// fixed in #853', ['tracker #853']],
    ['// moss#986 symptom', ['tracker moss#986']],
    ['// upstream tauri#10194', []],
    ['// Law 1: the subscriber owns the error', ['plan-label Law 1']],
    ['// Step 1: parse the front matter', []],
    ['// Task 4.2: the panel', ['plan-label Task 4.2']],
    ['// C3: outline removed', ['plan-label C3']],
    ['// an <h1> is H1', []],
    ['// NEL, a C1 control', []],
    ['// plain words only', []],
    ['// second review round', ['work-history review round']],
    ['// T3 owns this', ['plan-label T3', 'work-history T3']],
    ['// Phase B keeps the seam visible', ['letter-plan-label Phase B']],
    ['// B.4: keep the seam visible', ['letter-plan-label B.4']],
    ['// split out (Task C5, step 3)', ['letter-plan-label Task C5']],
    ['// before Thread-5 the server sent none', ['plan-label Thread-5']],
    ['// per design §5a step 2', ['doc-section §5a']],
    ['// spec §Task-1.1', ['doc-section §Task-1.1']],
    ['// RFC 9110 §15.5 says so', []],
    ['// always 0 on this branch', ['history on this branch']],
    ['// out of scope for this task', ['plan-label this task']],
    [`// see src-${'tauri'}/src/events.rs`, [`unknown-path src-${'tauri'}/src/events.rs`]],
    ['// the caller in app.ts', []],
    ['// the site runtime, crates/moss-build/src/js-src/site/x.ts', []],
    ['// beside shared/toast.ts, and ../../../README.md', []],
    ['// toast.ts, named alone, is the one file of that name', []],
    ['// the rule in style.css', []],
    ['// fixtures/a/style.css', []],
    ["// a site's index.md and its config.toml", []],
    ['// @symbiosis-lab/moss-ui/ui/shared/toast.ts', []],
    ['// https://example.test/a/app.ts', []],
  ])('%s', (line, expected) => {
    expect(rules('src/ui/editor/x.ts', line)).toEqual(expected);
  });

  it('leaves example filenames, bare module names and function calls intact', () => {
    expect(rules('src/ui/editor/x.ts', "// buildActivePath('post.md'), foo(), editor-main.ts and style.css")).toEqual([]);
    expect(rules('src/ui/editor/x.ts', '// missing/module.ts')).toEqual(['unknown-path missing/module.ts']);
  });

  it('checks only what is shaped like a repository path, not a site example', () => {
    const site = "// buildActivePath('works/post.md') maps folder/index.md to folder/index.html";
    expect(rules('src/ui/editor/x.ts', site)).toEqual([]);
    expect(rules('src/ui/editor/x.ts', '// `.moss/data/deployed-article-map.json` and blog/feed.json')).toEqual([]);
    // A source extension, a top-level directory of the repository, a relative path.
    expect(rules('src/ui/editor/x.ts', `// src-${'tauri'}/src/foo.rs`)).toEqual([`unknown-path src-${'tauri'}/src/foo.rs`]);
    expect(rules('src/ui/editor/x.ts', '// scripts/x.mjs')).toEqual(['unknown-path scripts/x.mjs']);
    expect(rules('src/ui/editor/x.ts', '// crates/moss-build/missing.md')).toEqual(['unknown-path crates/moss-build/missing.md']);
    expect(rules('src/ui/editor/x.ts', '// see ./gone.md')).toEqual(['unknown-path ./gone.md']);
  });

  it('takes a markup attribute value for a URL', () => {
    expect(rules('src/ui/preview/x.ts', '// a nested `<iframe src="./sketch.html">` in a page')).toEqual([]);
  });

  it('reads a path wrapped across two comment lines', () => {
    const src = ['/**', ` * The middleware (\`src-${'tauri'}/src/preview/`, ' * iframe_bridge.rs`) rewrites the tag.', ' */'].join('\n');
    expect(scanFile('src/ui/x.ts', src, null, pathResolver(TRACKED)).map((h) => `${h.line} ${h.rule} ${h.match}`))
      .toEqual([`2 unknown-path src-${'tauri'}/src/preview/iframe_bridge.rs`]);
  });

  it('joins consecutive comment lines into one block and keeps each line number', () => {
    const [a, b] = commentBlocks([[1, ' second review'], [2, ' round, then'], [4, ' wrapped-across-'], [5, ' * lines']]);
    expect([a.text, a.lineAt(a.text.indexOf('round'))]).toEqual(['second review round, then', 2]);
    expect([b.text, b.lineAt(0)]).toEqual(['wrapped-across-lines', 4]);
  });

  it('reads a test title as well as its comments', () => {
    expect(rules('src/ui/editor/__tests__/x.test.ts', "describe('menu (Plan 3)', () => {});")).toEqual(['plan-label Plan 3']);
  });

  it('reads a title on the next line and an each-table title', () => {
    const src = "it(\n  'opens (Task 2)', () => {});\nit.each([[1, 2]])('adds (Plan 3) %s', () => {});";
    expect(rules('src/ui/editor/__tests__/x.test.ts', src)).toEqual(['plan-label Task 2', 'plan-label Plan 3']);
  });

  it('does not read a test title in source, where it is a string', () => {
    expect(rules('src/ui/editor/x.ts', "describe('menu (Plan 3)', () => {});")).toEqual([]);
  });

  it('resolves a relative path from the file it sits in', () => {
    const known = pathResolver(['packages/moss-ui/src/ui/host.ts', 'packages/moss-ui/src/ui/shared/host.ts']);
    expect(known('../../host.ts', 'packages/moss-ui/src/ui/__tests__/helpers/x.ts')).toBe(true);
    expect(known('../host.ts', 'packages/moss-ui/src/ui/__tests__/helpers/x.ts')).toBe(false);
  });

  it('checks a path against the files it is given', () => {
    put('src/ui/a.ts', '// see ./b.ts and ./gone.ts');
    expect(check(root, { tracked: ['src/ui/a.ts', 'src/ui/b.ts'] }).hits.map((h) => `${h.rule} ${h.match}`)).toEqual(['unknown-path ./gone.ts']);
  });

  it('takes a colour for a colour in a stylesheet', () => {
    expect(rules('src/ui/styles/x.css', '/* #333 on #000000 */')).toEqual([]);
  });
});

describe('private names', () => {
  const PRIV = [{ pattern: 'acme-client', flags: '' }];

  it('finds a listed name anywhere in the text, code included', () => {
    expect(rules('src/ui/editor/x.ts', "const site = 'acme-client';", PRIV)).toEqual(['private-name acme-client']);
  });

  it('finds one in any file of the package, not only the UI', () => {
    put('src/ui/ok.ts', '// plain');
    put('README.md', 'Built for acme-client.');
    put('scripts/x.mjs', "export const s = 'acme-client';");
    expect(check(root, { tracked: [], privateRules: PRIV }).hits.map((h) => `${h.file} ${h.rule}`)).toEqual([
      'README.md private-name',
      'scripts/x.mjs private-name',
    ]);
  });

  it('refuses a missing setting, an unreadable file and an empty list', () => {
    expect(() => loadPrivateRules(undefined)).toThrow(/names no rules file/);
    expect(() => loadPrivateRules(join(root, 'absent.json'))).toThrow(/cannot read/);
    put('empty.json', '[]');
    expect(() => loadPrivateRules(join(root, 'empty.json'))).toThrow(/holds no rules/);
    put('notes.json', JSON.stringify([{ _comment: 'no pattern here' }]));
    expect(() => loadPrivateRules(join(root, 'notes.json'))).toThrow(/holds no rules/);
  });
});

describe('the command', () => {
  const SCRIPT = fileURLToPath(new URL('../public-ready.mjs', import.meta.url));
  const run = (env) => {
    const { MOSS_SCRUB_RULES: _drop, ...base } = process.env;
    return spawnSync(process.execPath, [SCRIPT, root], { encoding: 'utf8', env: { ...base, ...env } });
  };
  // The rules live outside the package, as the real list does: inside it they would name themselves.
  let rulesDir;
  beforeEach(() => {
    execFileSync('git', ['init', '-q'], { cwd: root });
    put('src/ui/ok.ts', '// plain');
    rulesDir = mkdtempSync(join(tmpdir(), 'public-ready-rules-'));
    writeFileSync(join(rulesDir, 'rules.json'), JSON.stringify([{ pattern: 'acme-client', flags: 'i' }]));
  });
  afterEach(() => rmSync(rulesDir, { recursive: true, force: true }));

  it('fails without the rules, so a clean report always means they were read', () => {
    expect(run({}).status).toBe(2);
    put('empty.json', '[]');
    expect(run({ MOSS_SCRUB_RULES: join(root, 'empty.json') }).status).toBe(2);
  });

  it('fails on a planted name and passes once it is gone, saying how many rules it read', () => {
    put('src/ui/planted.ts', '// made for ACME-CLIENT');
    const planted = run({ MOSS_SCRUB_RULES: join(rulesDir, 'rules.json') });
    expect([planted.status, planted.stderr]).toEqual([1, expect.stringContaining('src/ui/planted.ts:1: private-name')]);
    rmSync(join(root, 'src/ui/planted.ts'));
    const clean = run({ MOSS_SCRUB_RULES: join(rulesDir, 'rules.json') });
    expect([clean.status, clean.stdout]).toEqual([0, expect.stringContaining('1 private-name rules checked')]);
  });
});

describe('licence and provenance', () => {
  it('flags a third-party licence header and code taken from a URL', () => {
    expect(rules('src/ui/editor/x.ts', '// SPDX-License-Identifier: GPL-3.0')).toEqual(['licence-header SPDX-License-Identifier: GPL-3.0']);
    expect(rules('src/ui/editor/x.ts', '// adapted from https://example.test/snippet')).toEqual(['taken-from adapted from https://example.test/snippet']);
  });

  it('flags an imported package whose licence the package may not depend on, and an asset', () => {
    put('src/ui/editor/x.ts', "import { a } from 'copyleft-lib';\nimport { b } from 'fine-lib';\nimport { readFileSync } from 'node:fs';");
    put('src/ui/editor/logo.png');
    put('node_modules/copyleft-lib/package.json', JSON.stringify({ license: 'GPL-3.0' }));
    put('node_modules/fine-lib/package.json', JSON.stringify({ license: 'MIT' }));
    expect(check(root, { tracked: [] }).hits.map((h) => `${h.file} ${h.rule} ${h.match}`)).toEqual([
      'src/ui/editor/logo.png asset needs a stated source and licence',
      'src/ui/editor/x.ts package-licence copyleft-lib: GPL-3.0',
    ]);
  });

  it('scans only the UI, not the generated contract beside it', () => {
    put('src/engine.generated.ts', '// Task 1');
    put('src/ui/ok.ts', '// plain');
    expect(check(root, { tracked: [] })).toMatchObject({ files: ['src/ui/ok.ts'], hits: [] });
  });
});

describe('this package', () => {
  it('is ready to publish', () => {
    const pkg = fileURLToPath(new URL('../..', import.meta.url));
    expect(check(pkg).hits).toEqual([]);
  });
});
