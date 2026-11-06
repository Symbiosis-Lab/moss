// Tests for scripts/check-public-comments.mjs. Run: node --test scripts/__tests__/check-public-comments.test.mjs
//
// Each test writes a throwaway tree and runs the checker over it with the tree
// as the root argument.

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'check-public-comments.mjs');

function run(rel, text) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'public-comments-'));
  const file = path.join(root, rel);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text);
  const r = spawnSync('node', [SCRIPT, root], { encoding: 'utf8' });
  fs.rmSync(root, { recursive: true, force: true });
  return r;
}

const RS = 'crates/c/src/a.rs';

describe('flags private process vocabulary in comments', () => {
  const cases = {
    'ticket id': 'LOG-8D03-T1529-08-06',
    'phase label': 'Phase 4',
    'stage label': 'Stage 5b',
    'hyphenated stage label': 'Stage-1b',
    'M6a': 'M6a',
    'M6b': 'M6b',
    'PR label': 'PR7a',
    'NORTH-STAR': 'NORTH-STAR rule',
    'private spec': 'shape-spec § 1',
    'ADR id': 'ADR-018',
    'report reference': 'report 5 §1d',
    'ratchet row': 'ratchet row (o)',
    'task label': 'T8a',
  };
  for (const [name, phrase] of Object.entries(cases)) {
    it(`${name} in a line comment`, () => {
      const r = run(RS, `fn f() {}\n// see ${phrase} for why\n`);
      assert.equal(r.status, 1, r.stderr);
      assert.match(r.stderr, /a\.rs:2:/);
    });
  }

  it('block and doc comments, with the right line number', () => {
    const r = run(RS, `/*\n * fine\n * Phase 2 here\n */\n//! also Stage 3\n`);
    assert.equal(r.status, 1);
    assert.match(r.stderr, /a\.rs:3:/);
    assert.match(r.stderr, /a\.rs:5:/);
  });

  it('TypeScript comments', () => {
    const r = run('packages/p/src/x.ts', `const a = 1; // T6 toast\n`);
    assert.equal(r.status, 1);
  });
});

describe('leaves legitimate text alone', () => {
  it('the same phrase inside a string literal', () => {
    const r = run(RS, `fn f() { let _ = "Phase 4 and LOG-8D03-T1-0 and // PR7a"; }\n`);
    assert.equal(r.status, 0, r.stderr);
  });

  it('a Rust raw string and a char literal holding a quote', () => {
    const r = run(RS, `fn f() { let _ = r#"// Stage 2"#; let _ = '"'; let _ = "// T6"; }\n`);
    assert.equal(r.status, 0, r.stderr);
  });

  it('a TypeScript template literal', () => {
    const r = run('packages/p/src/x.ts', 'const a = `// Phase 1 ${x}`;\n');
    assert.equal(r.status, 0, r.stderr);
  });

  it('an ISO time', () => {
    const r = run(RS, `// built at 2026-01-02T10:30:00Z and T00:00:00Z\n`);
    assert.equal(r.status, 0, r.stderr);
  });

  it('a technical word that merely resembles a label', () => {
    const r = run(RS, `// the build phase, a Stage-relative path, PRNG seed, ADRESS\n`);
    assert.equal(r.status, 0, r.stderr);
  });

  it('an allow-listed phrase at its allow-listed path', () => {
    const r = run('crates/moss-build/src/seta/domains/wire.rs', `/// missing, XX, or T1 (Tor).\n`);
    assert.equal(r.status, 0, r.stderr);
  });

  it('the same phrase at another path is still flagged', () => {
    const r = run(RS, `/// missing, XX, or T1 (Tor).\n`);
    assert.equal(r.status, 1);
  });

  it('files outside crates/ and packages/*/src are not scanned', () => {
    const r = run('docs/x.rs', `// Phase 4\n`);
    assert.equal(r.status, 0);
  });
});
