// Tests for scripts/ratchet.mjs. Run: node --test scripts/__tests__/ratchet.test.mjs
//
// The script keeps its baseline beside itself and reads the tree from --root,
// so each test copies it into a throwaway tree with a baseline of its own and
// spawns it there. Nothing here can write to the real baseline.
//
// What is under test: row (a)'s banding (ratchet-spec.md §1 — the stored
// value is a ceiling that is always a multiple of 100), `accept` requiring
// --path (a pathless accept used to sweep every violation in a row, so a
// reason written for one file's growth silently accepted every other
// over-baseline file too — two landings, d16e5ff and 4fdb3cb, hand-edited
// their way around exactly that), and `verify-accepts`, the commit-msg hook's
// body that replaces the accepts[] log this baseline used to keep in JSON.

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync, execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'ratchet.mjs');
const ROW = 'prod_lines_per_file';
const A = 'crates/c/src/a.rs';
const B = 'crates/c/src/b.rs';
const C = 'crates/c/src/c.rs';

/**
 * A tree with `files` ({path: line count}) and a baseline of `rows`.
 *
 * realpath: on macOS os.tmpdir() is under /var, a symlink to /private/var, and
 * ratchet.mjs only runs main() when process.argv[1] equals its own realpathed
 * import.meta.url. Without it the script exits 0 having done nothing.
 */
function tree(files, rows) {
  const root = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'ratchet-'));
  for (const [rel, lines] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
    fs.writeFileSync(path.join(root, rel), typeof lines === 'string' ? lines : 'let x = 1;\n'.repeat(lines));
  }
  fs.mkdirSync(path.join(root, 'scripts'));
  fs.copyFileSync(SCRIPT, path.join(root, 'scripts', 'ratchet.mjs'));
  const baselineFile = path.join(root, 'scripts', 'ratchet-baseline.open.json');
  fs.writeFileSync(baselineFile, JSON.stringify({ rows }, null, 2) + '\n');
  return {
    root,
    baselineFile,
    baseline: () => JSON.parse(fs.readFileSync(baselineFile, 'utf8')),
    bytes: () => fs.readFileSync(baselineFile),
    run: (...argv) =>
      spawnSync(process.execPath, [path.join(root, 'scripts', 'ratchet.mjs'), ...argv, '--root', root], { encoding: 'utf8' }),
    done: () => fs.rmSync(root, { recursive: true, force: true }),
  };
}

/** A `tree()` that is also a real git checkout, one commit in, for verify-accepts. */
function gitTree(files, rows) {
  const t = tree(files, rows);
  const git = (...args) => execFileSync('git', args, { cwd: t.root, encoding: 'utf8' });
  git('init', '-q');
  git('config', 'user.email', 'test@example.com');
  git('config', 'user.name', 'Test');
  git('add', '-A');
  git('commit', '-q', '-m', 'initial');
  return {
    ...t,
    git, // exposed for tests that need real branches/merges/commits directly
    writeBaseline: (rowsNow) => fs.writeFileSync(t.baselineFile, JSON.stringify({ rows: rowsNow }, null, 2) + '\n'),
    stageBaseline: () => git('add', 'scripts/ratchet-baseline.open.json'),
    verifyAccepts: (message) => {
      const msgFile = path.join(t.root, 'COMMIT_MSG');
      fs.writeFileSync(msgFile, message);
      return t.run('verify-accepts', 'COMMIT_MSG');
    },
  };
}

const perPath = (value) => ({ [ROW]: { letter: 'a', disposition: 'permanent', armed: true, value } });

/** A baseline with every row present — `check` self-check 1 requires it. */
const fullBaseline = (value, deps = {}) => ({
  ...perPath(value),
  children_per_dir: { letter: 'b', disposition: 'permanent', armed: true, value: {} },
  mirror_markers: { letter: 'e', disposition: 'permanent', armed: true, value: 0 },
  direct_deps_per_crate: { letter: 'f', disposition: 'permanent', armed: true, value: deps },
  lines_per_function: { letter: 'g', disposition: 'permanent', armed: true, value: {}, oversized: {} },
  string_slices_per_crate: { letter: 'h', disposition: 'permanent', armed: true, value: { 'moss-build': 0, 'moss-cli': 0 } },
});

/** a and b are over their baselines, c sits exactly at its own. */
function twoOverOneAtBaseline() {
  return tree({ [A]: 900, [B]: 950, [C]: 1000 }, perPath({ [A]: 850, [B]: 900, [C]: 1000 }));
}

function within(t, fn) {
  try {
    fn(t);
  } finally {
    t.done();
  }
}

describe('row (a) banding', () => {
  it('check passes at the band ceiling and fails one line over it', () => {
    within(tree({ [A]: 900 }, fullBaseline({ [A]: 900 })), (t) => {
      const ok = t.run('check');
      assert.equal(ok.status, 0, ok.stdout + ok.stderr);
    });
    within(tree({ [A]: 901 }, fullBaseline({ [A]: 900 })), (t) => {
      const red = t.run('check');
      assert.notEqual(red.status, 0);
      assert.match(red.stdout, /grew to 901/);
    });
  });

  it('tighten only lowers a file once it crosses a 100 boundary, not on every shrink', () => {
    // 850 still bands to 900 — same ceiling, nothing to tighten.
    within(tree({ [A]: 850 }, perPath({ [A]: 900 })), (t) => {
      const r = t.run('tighten', '--path', A);
      assert.equal(r.status, 0, r.stderr);
      assert.equal(t.baseline().rows[ROW].value[A], 900);
      assert.match(r.stdout, /already tight/);
    });
    // 799 drops below the 800 floor entirely — the row stops tracking it.
    within(tree({ [A]: 799 }, perPath({ [A]: 900 })), (t) => {
      const r = t.run('tighten', '--path', A);
      assert.equal(r.status, 0, r.stderr);
      assert.equal(t.baseline().rows[ROW].value[A], undefined);
    });
    // 801 crosses down into the 900 band from 1000 — this one does lower.
    within(tree({ [A]: 801 }, perPath({ [A]: 1000 })), (t) => {
      const r = t.run('tighten', '--path', A);
      assert.equal(r.status, 0, r.stderr);
      assert.equal(t.baseline().rows[ROW].value[A], 900);
    });
  });

  it('tighten never touches a file left out of --path, even one it could lower', () => {
    within(tree({ [A]: 801, [B]: 801 }, perPath({ [A]: 1000, [B]: 1000 })), (t) => {
      const r = t.run('tighten', '--path', A);
      assert.equal(r.status, 0, r.stderr);
      const value = t.baseline().rows[ROW].value;
      assert.equal(value[A], 900, 'A was named, so it lowers');
      assert.equal(value[B], 1000, 'B was not named, so it must not move even though it could lower too');
    });
  });

  it('accept bands to the ceiling above current lines, not the raw count', () => {
    within(tree({ [A]: 901 }, perPath({ [A]: 850 })), (t) => {
      const r = t.run('accept', ROW, 'grew past its old ceiling', '--path', A);
      assert.equal(r.status, 0, r.stderr);
      assert.equal(t.baseline().rows[ROW].value[A], 1000);
      assert.match(r.stdout, /Ratchet-Accept: prod_lines_per_file crates\/c\/src\/a\.rs — grew past its old ceiling/);
    });
  });
});

const code = (n) => 'let x = 1;\n'.repeat(n);

describe('row (a) counts code lines, not comments or blanks', () => {
  const TS = 'packages/p/src/a.ts';

  it('a file long only in comments and blank lines is under the 800 floor', () => {
    const body = code(700) + '// c\n'.repeat(100) + '/// d\n'.repeat(100) + '//! e\n'.repeat(50) + '\n'.repeat(50)
      + '/*\n * block\n'.repeat(1) + ' * more\n'.repeat(98) + ' */\n';
    within(tree({ [A]: body }, fullBaseline({})), (t) => {
      const ok = t.run('check');
      assert.equal(ok.status, 0, ok.stdout + ok.stderr);
    });
  });

  it('a line with code and a trailing comment counts', () => {
    within(tree({ [A]: 'let x = 1; // note\n'.repeat(801) }, fullBaseline({})), (t) => {
      const red = t.run('check');
      assert.notEqual(red.status, 0);
      assert.match(red.stdout, /crossed 800 \(801 code lines/);
    });
  });

  it('code before or after a block comment on one line counts; a block comment alone does not', () => {
    const body = code(799) + '/* a */ let y = 2;\nlet z = 3; /* b */\n/* c */\n';
    within(tree({ [A]: body }, fullBaseline({})), (t) => {
      const red = t.run('check');
      assert.match(red.stdout, /crossed 800 \(801 code lines/);
    });
  });

  it('a comment opener inside a string literal does not swallow the lines after it', () => {
    within(tree({ [A]: 'let g = "src/**/*.rs";\n' + code(800) }, fullBaseline({})), (t) => {
      const red = t.run('check');
      assert.match(red.stdout, /crossed 800 \(801 code lines/);
    });
  });

  it('lines of a multi-line string literal are code', () => {
    within(tree({ [A]: 'let s = "\n' + 'text\n'.repeat(800) + '";\n' }, fullBaseline({})), (t) => {
      const red = t.run('check');
      assert.match(red.stdout, /crossed 800 \(802 code lines/);
    });
  });

  it('a Rust lifetime or char literal does not open a string', () => {
    const body = "fn f<'a>(x: &'a str) -> char { '\"' }\n" + code(800);
    within(tree({ [A]: body }, fullBaseline({})), (t) => {
      assert.match(t.run('check').stdout, /crossed 800 \(801 code lines/);
    });
  });

  it('#[cfg(test)] items are still excluded on top of the comment rule', () => {
    const body = code(700) + '#[cfg(test)]\nmod tests {\n' + code(300) + '}\n';
    within(tree({ [A]: body }, fullBaseline({})), (t) => {
      assert.equal(t.run('check').status, 0);
    });
  });

  it('TypeScript is counted the same way', () => {
    within(tree({ [TS]: code(700) + '// c\n'.repeat(300) }, fullBaseline({})), (t) => {
      assert.equal(t.run('check').status, 0);
    });
    within(tree({ [TS]: code(801) }, fullBaseline({})), (t) => {
      assert.match(t.run('check').stdout, /crossed 800 \(801 code lines/);
    });
  });

  it('tighten drops a ceiling whose file fell under 800 code lines and a disposition whose file fell to 1000 or under', () => {
    const dropped = tree({ [A]: code(700) + '// c\n'.repeat(400), [B]: code(950) + '// c\n'.repeat(400) },
      withOversized({ [A]: 1100, [B]: 1500 }, { [B]: 'a written reason of some length' }));
    within(dropped, (t) => {
      const r = t.run('tighten');
      assert.equal(r.status, 0, r.stdout + r.stderr);
      const row = t.baseline().rows[ROW];
      assert.deepEqual(row.value, { [B]: 1000 });
      assert.deepEqual(row.oversized, {});
    });
  });
});

const FN_ROW = 'lines_per_function';
/** A Rust fn of exactly `n` code lines: signature, n-2 statements, closing brace. */
const rustFn = (name, n, pre = '') => `${pre}fn ${name}() {\n${code(n - 2)}}\n`;
const withFns = (value, oversized = {}) => ({
  ...fullBaseline({}),
  [FN_ROW]: { letter: 'g', disposition: 'permanent', armed: true, value, oversized },
});

describe('row (g) lines_per_function', () => {
  const KEY = `${A}::big`;
  const TS = 'packages/p/src/a.ts';

  it('a function of 200 code lines is under the bar; 201 is a NEW entry', () => {
    within(tree({ [A]: rustFn('big', 200) }, withFns({})), (t) => {
      assert.equal(t.run('check').status, 0);
    });
    within(tree({ [A]: rustFn('big', 201) }, withFns({})), (t) => {
      const red = t.run('check');
      assert.notEqual(red.status, 0);
      assert.match(red.stdout, /NEW 'crates\/c\/src\/a\.rs::big' crossed 200 \(201 code lines/);
    });
  });

  it('comments and blank lines inside a function are not counted', () => {
    const body = 'fn big() {\n' + code(150) + '    // c\n'.repeat(80) + '\n'.repeat(80) + '}\n';
    within(tree({ [A]: body }, withFns({})), (t) => {
      assert.equal(t.run('check').status, 0);
    });
  });

  it('is banded to 50: a ceiling holds at the band and goes red one line over', () => {
    within(tree({ [A]: rustFn('big', 250) }, withFns({ [KEY]: 250 })), (t) => {
      assert.equal(t.run('check').status, 0);
    });
    within(tree({ [A]: rustFn('big', 251) }, withFns({ [KEY]: 250 })), (t) => {
      assert.match(t.run('check').stdout, /'crates\/c\/src\/a\.rs::big' grew to 251 code lines \(baseline 250\)/);
    });
  });

  it('a nested function counts toward its outer one and is not listed on its own', () => {
    const body = 'fn outer() {\n' + code(100) + rustFn('inner', 210) + code(10) + '}\n';
    within(tree({ [A]: body }, withFns({})), (t) => {
      const red = t.run('check');
      assert.match(red.stdout, /NEW 'crates\/c\/src\/a\.rs::outer' crossed 200 \(322 code lines/);
      assert.doesNotMatch(red.stdout, /::inner/);
    });
  });

  it('methods count, bodiless declarations and #[cfg(test)] functions do not', () => {
    const body = 'trait U {\n    fn big(&self);\n}\nimpl T {\n' + rustFn('big', 210, '    pub ') + '}\n#[cfg(test)]\n' + rustFn('t', 400);
    within(tree({ [A]: body }, withFns({})), (t) => {
      const red = t.run('check');
      assert.match(red.stdout, /::big' crossed 200 \(210 code lines/);
      assert.doesNotMatch(red.stdout, /::big#2|::t'/);
    });
  });

  it('a Rust where clause ending in a comma does not hide the body or the next function', () => {
    const body = 'fn small<F>(f: F)\nwhere\n    F: Fn(),\n{\n' + code(3) + '}\n' + rustFn('big', 210);
    within(tree({ [A]: body }, withFns({})), (t) => {
      const red = t.run('check');
      assert.match(red.stdout, /NEW 'crates\/c\/src\/a\.rs::big' crossed 200 \(210 code lines/);
      assert.doesNotMatch(red.stdout, /::small/);
    });
  });

  it('a second function of the same name in a file gets a #2 key', () => {
    within(tree({ [A]: rustFn('big', 210) + rustFn('big', 220) }, withFns({})), (t) => {
      const red = t.run('check');
      assert.match(red.stdout, /::big' crossed 200 \(210/);
      assert.match(red.stdout, /::big#2' crossed 200 \(220/);
    });
  });

  it('TypeScript functions, arrow functions with a block body and methods are measured', () => {
    const body = 'export function decl(a: number): { n: number } {\n' + code(210) + '}\n'
      + 'export const arrow = async (x: number) => {\n' + code(220) + '};\n'
      + 'const expr = (x: number) => x + 1;\n'
      + 'class K {\n  method(a: string): void {\n' + code(230) + '  }\n}\n';
    within(tree({ [TS]: body }, withFns({})), (t) => {
      const out = t.run('check').stdout;
      assert.match(out, /a\.ts::decl' crossed 200 \(212 code lines/);
      assert.match(out, /a\.ts::arrow' crossed 200 \(222 code lines/);
      assert.match(out, /a\.ts::method' crossed 200 \(232 code lines/);
      assert.doesNotMatch(out, /::expr|::K/);
    });
  });

  it('a function over 500 code lines needs a written disposition; one at 500 does not', () => {
    within(tree({ [A]: rustFn('big', 500) }, withFns({ [KEY]: 500 })), (t) => {
      assert.equal(t.run('check').status, 0);
    });
    within(tree({ [A]: rustFn('big', 501) }, withFns({ [KEY]: 550 })), (t) => {
      const red = t.run('check');
      assert.match(red.stdout, /'crates\/c\/src\/a\.rs::big' has 501 code lines > 500/);
      assert.match(red.stdout, /lines_per_function\.oversized\{\}/);
    });
    within(tree({ [A]: rustFn('big', 501) }, withFns({ [KEY]: 550 }, { [KEY]: 'a phase per step, retires once each returns a result' })), (t) => {
      assert.equal(t.run('check').status, 0);
    });
  });

  it('a disposition whose function fell to 500 or under is stale', () => {
    within(tree({ [A]: rustFn('big', 480) }, withFns({ [KEY]: 500 }, { [KEY]: 'a written reason that no longer applies' })), (t) => {
      assert.match(t.run('check').stdout, /lines_per_function\.oversized entry '.*::big' is down to 480 code lines/);
    });
  });

  it('a ceiling more than a band above the real size is stale-high', () => {
    within(tree({ [A]: rustFn('big', 240) }, withFns({ [KEY]: 300 })), (t) => {
      assert.match(t.run('check').stdout, /stale-high\): lines_per_function '.*::big' ceiling 300 is >=50 above its actual 240/);
    });
  });

  it('tighten lowers to the band, then drops the entry and its disposition once the function is 200 or under', () => {
    within(tree({ [A]: rustFn('big', 320) }, withFns({ [KEY]: 600 }, { [KEY]: 'a written reason that no longer applies' })), (t) => {
      assert.equal(t.run('tighten', '--path', KEY).status, 0);
      const row = t.baseline().rows[FN_ROW];
      assert.equal(row.value[KEY], 350);
      assert.deepEqual(row.oversized, {});
    });
    within(tree({ [A]: rustFn('big', 150) }, withFns({ [KEY]: 250 })), (t) => {
      assert.equal(t.run('tighten').status, 0);
      assert.deepEqual(t.baseline().rows[FN_ROW].value, {});
    });
    within(tree({ [A]: rustFn('big', 320) }, withFns({ [KEY]: 300 })), (t) => {
      const r = t.run('tighten', '--path', KEY);
      assert.notEqual(r.status, 0);
      assert.equal(t.baseline().rows[FN_ROW].value[KEY], 300);
    });
  });

  it('a renamed function is a NEW entry and a deleted file is stale', () => {
    within(tree({ [A]: rustFn('renamed', 250) }, withFns({ [KEY]: 250 })), (t) => {
      assert.match(t.run('check').stdout, /NEW 'crates\/c\/src\/a\.rs::renamed'/);
    });
    within(tree({ [B]: code(1) }, withFns({ [KEY]: 250 })), (t) => {
      assert.match(t.run('check').stdout, /lines_per_function baseline entry '.*::big' no longer exists on disk/);
    });
  });

  it('accept prints a trailer keyed path::fn, and verify-accepts takes it back', () => {
    within(gitTree({ [A]: rustFn('big', 300) }, withFns({ [KEY]: 250 })), (t) => {
      const r = t.run('accept', FN_ROW, 'grew with the new import path', '--path', KEY);
      assert.equal(r.status, 0, r.stdout + r.stderr);
      assert.equal(t.baseline().rows[FN_ROW].value[KEY], 300);
      const trailer = r.stdout.split('\n').find((l) => l.startsWith('Ratchet-Accept:'));
      assert.equal(trailer, `Ratchet-Accept: ${FN_ROW} ${KEY} — grew with the new import path`);
      t.stageBaseline();
      assert.equal(t.verifyAccepts(`raise\n\n${trailer}\n`).status, 0);
      assert.notEqual(t.verifyAccepts('raise, no trailer\n').status, 0);
    });
  });

  it('accept refuses a function that is not over its ceiling', () => {
    within(tree({ [A]: rustFn('big', 240) }, withFns({ [KEY]: 250 })), (t) => {
      assert.notEqual(t.run('accept', FN_ROW, 'nothing actually grew here', '--path', KEY).status, 0);
    });
  });
});

describe('accept --path', () => {
  it('raises only the named file and leaves the other over-baseline file alone', () => {
    within(twoOverOneAtBaseline(), (t) => {
      const r = t.run('accept', ROW, 'a grew on purpose', '--path', A);

      assert.equal(r.status, 0, r.stderr);
      const value = t.baseline().rows[ROW].value;
      assert.equal(value[A], 900);
      assert.equal(value[B], 900, "b's growth was never accepted, so its baseline must not move");
      assert.equal(value[C], 1000);
    });
  });

  it('prints one Ratchet-Accept trailer line per named path, and nothing is written to the baseline JSON', () => {
    within(twoOverOneAtBaseline(), (t) => {
      const r = t.run('accept', ROW, 'because reasons', '--path', A);

      assert.match(r.stdout, /^Ratchet-Accept: prod_lines_per_file crates\/c\/src\/a\.rs — because reasons$/m);
      assert.equal(t.baseline().accepts, undefined, 'the reason lives in the commit message now, not in accepts[]');
    });
  });

  it('takes several --path flags and raises exactly those, one trailer line each', () => {
    within(tree({ [A]: 900, [B]: 950, [C]: 1010 }, perPath({ [A]: 850, [B]: 900, [C]: 1000 })), (t) => {
      const r = t.run('accept', ROW, 'two of three grew this cycle', '--path', A, '--path', B);

      assert.equal(r.status, 0, r.stderr);
      const value = t.baseline().rows[ROW].value;
      assert.deepEqual([value[A], value[B], value[C]], [900, 1000, 1000]);
      assert.match(r.stdout, /Ratchet-Accept: prod_lines_per_file crates\/c\/src\/a\.rs — two of three grew this cycle/);
      assert.match(r.stdout, /Ratchet-Accept: prod_lines_per_file crates\/c\/src\/b\.rs — two of three grew this cycle/);
    });
  });

  it('records a new file over the threshold, banded, as from: null equivalent (no prior entry)', () => {
    within(tree({ [A]: 900, [B]: 950 }, perPath({ [A]: 900 })), (t) => {
      const r = t.run('accept', ROW, 'new file crossed the threshold', '--path', B);

      assert.equal(r.status, 0, r.stderr);
      assert.equal(t.baseline().rows[ROW].value[B], 1000);
    });
  });

  it('folds ./ and absolute spellings to the baseline key', () => {
    for (const spelling of [`./${A}`, (root) => path.join(root, A)]) {
      within(twoOverOneAtBaseline(), (t) => {
        const arg = typeof spelling === 'function' ? spelling(t.root) : spelling;
        const r = t.run('accept', ROW, 'spelling of the path varies', '--path', arg);

        assert.equal(r.status, 0, r.stderr);
        assert.equal(t.baseline().rows[ROW].value[A], 900);
        assert.equal(t.baseline().rows[ROW].value[B], 900);
      });
    }
  });

  it('errors, naming the path and writing nothing, when the file is not over its baseline', () => {
    within(twoOverOneAtBaseline(), (t) => {
      const before = t.bytes();
      const r = t.run('accept', ROW, 'nothing grew here at all', '--path', C);

      assert.notEqual(r.status, 0);
      assert.ok(r.stderr.includes(C), `stderr should name the path: ${r.stderr}`);
      assert.match(r.stderr, /not over its/);
      assert.ok(t.bytes().equals(before), 'the baseline must be byte-identical');
    });
  });

  it('errors and writes nothing when one of several named files is not over, even though another is', () => {
    within(twoOverOneAtBaseline(), (t) => {
      const before = t.bytes();
      const r = t.run('accept', ROW, 'typo in the second', '--path', A, '--path', C);

      assert.notEqual(r.status, 0);
      assert.ok(r.stderr.includes(C), r.stderr);
      assert.ok(t.bytes().equals(before), 'a is over, but accepting it alone would half-apply the call');
    });
  });

  it('errors on a path that is not a file of the tree at all', () => {
    within(twoOverOneAtBaseline(), (t) => {
      const r = t.run('accept', ROW, 'typo in the path argument', '--path', 'crates/c/src/nope.rs');

      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /nope\.rs.*absent/);
    });
  });

  it('refuses --path on a scalar row', () => {
    within(
      tree({ [A]: 10 }, { mirror_markers: { letter: 'e', disposition: 'permanent', armed: true, value: 0 } }),
      (t) => {
        const r = t.run('accept', 'mirror_markers', 'a --path does not apply here', '--path', A);

        assert.notEqual(r.status, 0);
        assert.match(r.stderr, /one number/);
      },
    );
  });

  it('needs a value after --path', () => {
    within(twoOverOneAtBaseline(), (t) => {
      const r = spawnSync(
        process.execPath,
        [path.join(t.root, 'scripts', 'ratchet.mjs'), 'accept', ROW, 'why', '--root', t.root, '--path'],
        { encoding: 'utf8' },
      );
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /--path needs a file/);
    });
  });
});

describe('accept requires --path on a per-path row', () => {
  // A pathless accept used to sweep every over-baseline entry of the row —
  // this is the behavior ratchet-spec.md §1 explicitly removes.
  it('errors and writes nothing when no --path is given', () => {
    within(twoOverOneAtBaseline(), (t) => {
      const before = t.bytes();
      const r = t.run('accept', ROW, 'meant to raise the whole row');

      assert.notEqual(r.status, 0, 'a pathless accept on a per-path row must fail');
      assert.match(r.stderr, /--path/);
      assert.ok(t.bytes().equals(before), 'nothing should be written when the call is refused');
    });
  });
});

describe('tighten --path', () => {
  it('lowers only the named file although another could be lowered too', () => {
    within(tree({ [A]: 801, [B]: 820 }, perPath({ [A]: 1000, [B]: 1000 })), (t) => {
      const r = t.run('tighten', '--path', A);

      assert.equal(r.status, 0, r.stderr);
      const value = t.baseline().rows[ROW].value;
      assert.equal(value[A], 900);
      assert.equal(value[B], 1000);
    });
  });

  it('errors, writing nothing, on a path no baseline row holds', () => {
    within(tree({ [A]: 810, [B]: 820 }, perPath({ [A]: 900, [B]: 900 })), (t) => {
      const before = t.bytes();
      const r = t.run('tighten', '--path', A, '--path', 'crates/c/src/nope.rs');

      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /nope\.rs.*no baseline row/);
      assert.ok(t.bytes().equals(before), 'the valid path must not be tightened when the other is refused');
    });
  });

  it('still refuses to raise a named file that is over its baseline', () => {
    within(twoOverOneAtBaseline(), (t) => {
      const before = t.bytes();
      const r = t.run('tighten', '--path', A);

      assert.notEqual(r.status, 0);
      assert.match(r.stdout, /REFUSED to raise/);
      assert.ok(t.bytes().equals(before));
    });
  });
});

describe('verify-accepts', () => {
  it('passes when the commit message carries a correct trailer for a raised key', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      t.writeBaseline(perPath({ [A]: 900 }));
      t.stageBaseline();
      const r = t.verifyAccepts('bump A\n\nRatchet-Accept: prod_lines_per_file crates/c/src/a.rs — grew on purpose\n');
      assert.equal(r.status, 0, r.stdout + r.stderr);
    });
  });

  it('fails when the commit message has no trailer at all', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      t.writeBaseline(perPath({ [A]: 900 }));
      t.stageBaseline();
      const r = t.verifyAccepts('bump A, forgot the trailer\n');
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /prod_lines_per_file crates\/c\/src\/a\.rs/);
    });
  });

  it('fails when the trailer has an empty reason', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      t.writeBaseline(perPath({ [A]: 900 }));
      t.stageBaseline();
      const r = t.verifyAccepts('bump A\n\nRatchet-Accept: prod_lines_per_file crates/c/src/a.rs — \n');
      assert.notEqual(r.status, 0);
    });
  });

  it('ignores a lowered value — no trailer needed', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 900 })), (t) => {
      t.writeBaseline(perPath({ [A]: 800 }));
      t.stageBaseline();
      const r = t.verifyAccepts('tighten A, no trailer needed\n');
      assert.equal(r.status, 0, r.stdout + r.stderr);
    });
  });

  it('requires a trailer for a key newly added to a shrink-only row', () => {
    within(gitTree({ [A]: 900, [B]: 900 }, perPath({ [A]: 900 })), (t) => {
      t.writeBaseline(perPath({ [A]: 900, [B]: 900 }));
      t.stageBaseline();
      const missing = t.verifyAccepts('add B, no trailer\n');
      assert.notEqual(missing.status, 0);

      t.writeBaseline(perPath({ [A]: 900, [B]: 900 })); // same content, re-stage for the second half
      t.stageBaseline();
      const withTrailer = t.verifyAccepts('add B\n\nRatchet-Accept: prod_lines_per_file crates/c/src/b.rs — new file crossed 800 lines\n');
      assert.equal(withTrailer.status, 0, withTrailer.stdout + withTrailer.stderr);
    });
  });

  it('does nothing when the baseline file is not staged', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      // Change the baseline on disk but never stage it.
      t.writeBaseline(perPath({ [A]: 900 }));
      const r = t.verifyAccepts('unrelated commit, no trailer, baseline not staged\n');
      assert.equal(r.status, 0, r.stdout + r.stderr);
    });
  });
});

// 2026-09-21 joint review parity fixes: tests for items 1, 4, 5, 6, 8.

describe('scalar row trailer key (item 1)', () => {
  it('accept prints (total) as a scalar row\'s key, not the row name repeated, and verify-accepts matches it', () => {
    within(
      gitTree({}, { mirror_markers: { letter: 'e', disposition: 'permanent', armed: true, value: 0 } }),
      (t) => {
        fs.mkdirSync(path.join(t.root, 'crates', 'c', 'src'), { recursive: true });
        fs.writeFileSync(path.join(t.root, 'crates', 'c', 'src', 'm.rs'), '// must match x\n// must match y\n');

        const accept = t.run('accept', 'mirror_markers', 'two new mirror markers added here');
        assert.equal(accept.status, 0, accept.stderr);
        assert.match(accept.stdout, /^Ratchet-Accept: mirror_markers \(total\) — two new mirror markers added here$/m);

        t.stageBaseline();
        const r = t.verifyAccepts(
          'bump mirror_markers\n\nRatchet-Accept: mirror_markers (total) — two new mirror markers added here\n',
        );
        assert.equal(r.status, 0, r.stdout + r.stderr);
      },
    );
  });
});

describe('verify-accepts skips an ordinary merge commit (item 4)', () => {
  it('does not block a merge landing a peer branch\'s already-trailered raise, even with no trailer of its own', () => {
    within(gitTree({}, { mirror_markers: { letter: 'e', disposition: 'permanent', armed: true, value: 0 } }), (t) => {
      const base = t.git('branch', '--show-current').trim();

      t.git('checkout', '-b', 'peer');
      t.writeBaseline({ mirror_markers: { letter: 'e', disposition: 'permanent', armed: true, value: 5 } });
      t.git('add', 'scripts/ratchet-baseline.open.json');
      t.git(
        'commit', '-q', '-m',
        'accept mirror_markers\n\nRatchet-Accept: mirror_markers (total) — five markers landed on the peer branch\n',
      );

      t.git('checkout', base);
      t.writeBaseline({ mirror_markers: { letter: 'e', disposition: 'permanent', armed: true, value: 3 } });
      t.git('add', 'scripts/ratchet-baseline.open.json');
      t.git('commit', '-q', '-m', 'unrelated change on the base branch, conflicts with peer on the same line');

      // Same line of the same file touched on both sides — a real conflict,
      // so `git merge` pauses instead of auto-committing, leaving MERGE_HEAD
      // in place exactly like it would mid-hook on a real conflicted merge.
      let conflicted = false;
      try {
        t.git('merge', 'peer');
      } catch {
        conflicted = true;
      }
      assert.ok(conflicted, 'the two branches must conflict on the same baseline line for this test to be meaningful');

      // Resolve by taking peer's five, as a maintainer would, and stage the
      // resolution — but do NOT commit yet, so MERGE_HEAD is still present.
      t.writeBaseline({ mirror_markers: { letter: 'e', disposition: 'permanent', armed: true, value: 5 } });
      t.git('add', 'scripts/ratchet-baseline.open.json');

      // The merge's own message (what git would hand the hook) carries no
      // trailer at all, and the staged resolution raises mirror_markers all
      // the way from the base branch's HEAD (3) to 5 — a real, untrailered
      // rise on this commit's own diff. Only isMidMerge should be saving it.
      const r = t.verifyAccepts('Merge branch \'peer\'\n');
      assert.equal(r.status, 0, r.stdout + r.stderr);
    });
  });
});

describe('a brand-new scalar row (item 5)', () => {
  it('starting at exactly 0 needs no trailer; starting above 0 does', () => {
    within(gitTree({}, {}), (t) => {
      t.writeBaseline({ mirror_markers: { letter: 'e', disposition: 'permanent', armed: true, value: 0 } });
      t.stageBaseline();
      const atZero = t.verifyAccepts('add the mirror_markers row, starts at zero\n');
      assert.equal(atZero.status, 0, atZero.stdout + atZero.stderr);

      t.writeBaseline({ mirror_markers: { letter: 'e', disposition: 'permanent', armed: true, value: 5 } });
      t.stageBaseline();
      const aboveZeroNoTrailer = t.verifyAccepts('add the mirror_markers row, already at five\n');
      assert.notEqual(aboveZeroNoTrailer.status, 0);

      t.writeBaseline({ mirror_markers: { letter: 'e', disposition: 'permanent', armed: true, value: 5 } });
      t.stageBaseline();
      const aboveZeroWithTrailer = t.verifyAccepts(
        'add the mirror_markers row, already at five\n\nRatchet-Accept: mirror_markers (total) — grandfathering five pre-existing markers\n',
      );
      assert.equal(aboveZeroWithTrailer.status, 0, aboveZeroWithTrailer.stdout + aboveZeroWithTrailer.stderr);
    });
  });
});

describe('reasonIsWeak floor (item 6)', () => {
  it('accept refuses a reason under 15 characters', () => {
    within(twoOverOneAtBaseline(), (t) => {
      const before = t.bytes();
      const r = t.run('accept', ROW, 'short', '--path', A);
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /reason too weak/);
      assert.ok(t.bytes().equals(before));
    });
  });

  it('accept refuses a reason that is exactly the row name, even at full length', () => {
    within(twoOverOneAtBaseline(), (t) => {
      const r = t.run('accept', ROW, ROW, '--path', A); // 'prod_lines_per_file' is 19 chars, clears the length floor
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /reason too weak/);
    });
  });

  it('accept refuses a reason that is exactly the path being raised', () => {
    within(twoOverOneAtBaseline(), (t) => {
      const r = t.run('accept', ROW, A, '--path', A); // 'crates/c/src/a.rs' is 18 chars, clears the length floor
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /reason too weak/);
    });
  });

  it('verify-accepts treats a present-but-weak trailer the same as a missing one', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      t.writeBaseline(perPath({ [A]: 900 }));
      t.stageBaseline();
      const r = t.verifyAccepts('bump A\n\nRatchet-Accept: prod_lines_per_file crates/c/src/a.rs — short\n');
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /too weak/);
    });
  });
});

describe('verify-accepts --range (item 8)', () => {
  function adoptPolicy(t) {
    fs.mkdirSync(path.join(t.root, '.githooks'));
    fs.copyFileSync(path.join(path.dirname(SCRIPT), '..', '.githooks', 'commit-msg'), path.join(t.root, '.githooks', 'commit-msg'));
    t.git('add', '.githooks/commit-msg');
    t.git('commit', '-q', '-m', 'enforce acceptance trailers');
  }

  it('does not retroactively require trailers before the commit-time policy existed', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 800 })), (t) => {
      const start = t.git('rev-parse', 'HEAD').trim();
      t.writeBaseline(perPath({ [A]: 850 }));
      t.stageBaseline();
      t.git('commit', '-q', '-m', 'historical raise before enforcement');
      t.writeBaseline(perPath({ [A]: 900 }));
      t.stageBaseline();
      adoptPolicy(t);

      const r = t.run('verify-accepts', '--range', `${start}..HEAD`);
      assert.equal(r.status, 0, r.stdout + r.stderr);
      assert.match(r.stdout, /OK/);
    });
  });

  it('flags a post-adoption commit whose raise has no trailer', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      adoptPolicy(t);
      t.writeBaseline(perPath({ [A]: 900 }));
      t.git('add', 'scripts/ratchet-baseline.open.json');
      t.git('commit', '-q', '-m', 'bump A, no trailer');

      const r = t.run('verify-accepts', '--range', 'HEAD~1..HEAD');
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /prod_lines_per_file crates\/c\/src\/a\.rs/);
    });
  });

  it('requires the verifier tool as well as the hook to establish adoption', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 800 })), (t) => {
      const scriptPath = path.join(t.root, 'scripts', 'ratchet.mjs');
      fs.writeFileSync(scriptPath, '// verifier not implemented yet\n');
      t.git('add', 'scripts/ratchet.mjs');
      adoptPolicy(t);
      const start = t.git('rev-parse', 'HEAD').trim();
      t.writeBaseline(perPath({ [A]: 850 }));
      t.stageBaseline();
      fs.copyFileSync(SCRIPT, scriptPath);
      t.git('add', 'scripts/ratchet.mjs');
      t.git('commit', '-q', '-m', 'add the verifier with an existing baseline raise');

      const before = t.run('verify-accepts', '--range', `${start}..HEAD`);
      assert.equal(before.status, 0, before.stdout + before.stderr);
      t.writeBaseline(perPath({ [A]: 900 }));
      t.stageBaseline();
      t.git('commit', '-q', '-m', 'later raise without acceptance');
      const after = t.run('verify-accepts', '--range', `${start}..HEAD`);
      assert.notEqual(after.status, 0);
      assert.match(after.stderr, /from 850 to 900/);
      assert.doesNotMatch(after.stderr, /from 800 to 850/);
    });
  });

  it('passes when the historical commit carries a matching trailer', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      adoptPolicy(t);
      t.writeBaseline(perPath({ [A]: 900 }));
      t.git('add', 'scripts/ratchet-baseline.open.json');
      t.git(
        'commit', '-q', '-m',
        'bump A\n\nRatchet-Accept: prod_lines_per_file crates/c/src/a.rs — grew past its old ceiling\n',
      );

      const r = t.run('verify-accepts', '--range', 'HEAD~1..HEAD');
      assert.equal(r.status, 0, r.stdout + r.stderr);
      assert.match(r.stdout, /OK/);
    });
  });

  it('still requires trailers after the adopted hook is removed', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      adoptPolicy(t);
      t.git('rm', '.githooks/commit-msg');
      t.git('commit', '-q', '-m', 'remove hook');
      t.writeBaseline(perPath({ [A]: 900 }));
      t.stageBaseline();
      t.git('commit', '-q', '-m', 'raise without acceptance');

      const r = t.run('verify-accepts', '--range', 'HEAD~1..HEAD');
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /prod_lines_per_file crates\/c\/src\/a\.rs/);
    });
  });

  function historicalRaise(t) {
    adoptPolicy(t);
    const start = t.git('rev-parse', 'HEAD').trim();
    t.writeBaseline(perPath({ [A]: 900 }));
    t.stageBaseline();
    t.git('commit', '-q', '-m', 'raise with missing metadata');
    return { start, original: t.git('rev-parse', 'HEAD').trim() };
  }

  const historyTrailer = (original, row = ROW, key = A, reason = 'keeps the measured source-name sorting behavior') =>
    `Ratchet-Accept-History: ${original} ${row} ${key} — ${reason}`;

  function recovery(t, ...trailers) {
    t.git('commit', '--allow-empty', '-q', '-m', `record historical acceptance\n\n${trailers.join('\n')}\n`);
  }

  it('accepts a verified forward historical record without rewriting the original commit', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      const { start, original } = historicalRaise(t);
      recovery(t, historyTrailer(original));
      const r = t.run('verify-accepts', '--range', `${start}..HEAD`);
      assert.equal(r.status, 0, r.stdout + r.stderr);
      assert.doesNotMatch(t.git('log', '-1', '--format=%B', original), /Ratchet-Accept/);
    });
  });

  it('does not let a historical record satisfy a new staged raise', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      const { original } = historicalRaise(t);
      t.writeBaseline(perPath({ [A]: 1000 }));
      t.stageBaseline();
      const r = t.verifyAccepts(`raise again\n\n${historyTrailer(original)}\n`);
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /from 900 to 1000/);
    });
  });

  for (const [label, make] of [
    ['abbreviated commit', (sha) => historyTrailer(sha.slice(0, 9))],
    ['fabricated commit', () => historyTrailer('0'.repeat(40))],
    ['unrelated row', (sha) => historyTrailer(sha, 'children_per_dir')],
    ['unrelated key', (sha) => historyTrailer(sha, ROW, B)],
    ['weak reason', (sha) => historyTrailer(sha, ROW, A, 'wip')],
    ['malformed record', (sha) => `Ratchet-Accept-History: ${sha} ${ROW} ${A}`],
  ]) {
    it(`rejects a historical acceptance with an ${label}`, () => {
      within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
        const { start, original } = historicalRaise(t);
        recovery(t, make(original));
        const r = t.run('verify-accepts', '--range', `${start}..HEAD`);
        assert.notEqual(r.status, 0);
        assert.match(r.stderr, /Ratchet-Accept-History/);
      });
    });
  }

  it('rejects an original commit that did not raise the named baseline', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      adoptPolicy(t);
      const original = t.git('rev-parse', 'HEAD').trim();
      recovery(t, historyTrailer(original));
      const r = t.run('verify-accepts', '--range', 'HEAD~1..HEAD');
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /not a baseline raise/);
    });
  });

  it('rejects a malformed historical trailer name even with no outstanding raise', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      adoptPolicy(t);
      const original = t.git('rev-parse', 'HEAD').trim();
      recovery(t, historyTrailer(original).replace('History:', 'History'));
      const r = t.run('verify-accepts', '--range', 'HEAD~1..HEAD');
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /invalid Ratchet-Accept-History record/);
    });
  });

  it('requires a commit object rather than an annotated tag that Git can peel', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      const { original } = historicalRaise(t);
      t.git('tag', '-a', 'raised', original, '-m', 'tag the original increase');
      const tag = t.git('rev-parse', 'raised').trim();
      recovery(t, historyTrailer(tag));
      const r = t.run('verify-accepts', '--range', 'HEAD~1..HEAD');
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /original is not a commit object/);
    });
  });

  it('requires the original to be an ancestor of the recovery commit itself', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      adoptPolicy(t);
      const start = t.git('rev-parse', 'HEAD').trim();
      t.git('checkout', '-q', '-b', 'other');
      t.writeBaseline(perPath({ [A]: 900 }));
      t.stageBaseline();
      t.git('commit', '-q', '-m', 'raise on other branch');
      const original = t.git('rev-parse', 'HEAD').trim();
      t.git('checkout', '-q', '-b', 'recovery', start);
      recovery(t, historyTrailer(original));
      t.git('merge', '-q', '--no-edit', 'other');
      const r = t.run('verify-accepts', '--range', `${start}..HEAD`);
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /not an ancestor of the recovery commit/);
    });
  });

  for (const conflicting of [false, true]) {
    it(`rejects ${conflicting ? 'conflicting' : 'duplicate'} historical records`, () => {
      within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
        const { start, original } = historicalRaise(t);
        recovery(t, historyTrailer(original));
        recovery(t, historyTrailer(original, ROW, A, conflicting ? 'another reason for the same baseline increase' : undefined));
        const r = t.run('verify-accepts', '--range', `${start}..HEAD`);
        assert.notEqual(r.status, 0);
        assert.match(r.stderr, /duplicate or conflicting/);
      });
    });
  }

  it('needs an A..B range, not a bare ref', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      const r = t.run('verify-accepts', '--range', 'HEAD');
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /usage/);
    });
  });
});

// prod_lines_per_file.oversized{} — the 1000-line disposition tier. Same
// `{path: reason}` shape as children_per_dir.oversized{} (self-check 2b,
// exercised above under 'row (a) banding' and 'accept --path'), reusing
// `reasonIsWeak` rather than a second notion of "a real reason".

/** A baseline with a real oversized{} entry for A, for the tests below that
 *  don't care about its exact wording. */
const withOversized = (value, oversized) => ({
  ...fullBaseline(value),
  prod_lines_per_file: { letter: 'a', disposition: 'permanent', armed: true, value, oversized },
});

describe('prod_lines_per_file.oversized{} (the 1000-line tier)', () => {
  it('check is red for a file over 1000 code lines with no oversized{} entry', () => {
    within(tree({ [A]: 1001 }, fullBaseline({ [A]: 1100 })), (t) => {
      const red = t.run('check');
      assert.notEqual(red.status, 0);
      assert.match(red.stdout, /'crates\/c\/src\/a\.rs' has 1001 code lines > 1000/);
      assert.match(red.stdout, /oversized\{\}/);
    });
  });

  it('check is green for the same file once oversized{} carries a real reason', () => {
    within(
      tree({ [A]: 1001 }, withOversized({ [A]: 1100 }, { [A]: 'grew past 1000 lines for a real reason, retires once X ships' })),
      (t) => {
        const ok = t.run('check');
        assert.equal(ok.status, 0, ok.stdout + ok.stderr);
      },
    );
  });

  it('a file at exactly 1000 lines needs no oversized{} entry — the tier is strictly over 1000', () => {
    within(tree({ [A]: 1000 }, fullBaseline({ [A]: 1000 })), (t) => {
      const ok = t.run('check');
      assert.equal(ok.status, 0, ok.stdout + ok.stderr);
    });
  });

  it('check reuses reasonIsWeak: a weak oversized{} reason counts as no disposition at all', () => {
    within(tree({ [A]: 1001 }, withOversized({ [A]: 1100 }, { [A]: 'wip' })), (t) => {
      const red = t.run('check');
      assert.notEqual(red.status, 0);
      assert.match(red.stdout, /'crates\/c\/src\/a\.rs' has 1001 code lines > 1000/);
    });
  });

  it('an oversized{} entry whose file dropped back to <=1000 lines is reported stale, not silently kept', () => {
    within(
      tree({ [A]: 950 }, withOversized({ [A]: 1000 }, { [A]: 'used to be over 1000; no longer true' })),
      (t) => {
        const red = t.run('check');
        assert.notEqual(red.status, 0);
        assert.match(red.stdout, /oversized entry 'crates\/c\/src\/a\.rs' is down to 950 code lines/);
      },
    );
  });

  it('an oversized{} entry for a file that shrank below the 800 floor entirely is also reported stale', () => {
    within(
      tree({ [A]: 700 }, withOversized({}, { [A]: 'used to be over 1000; long since rewritten' })),
      (t) => {
        const red = t.run('check');
        assert.notEqual(red.status, 0);
        assert.match(red.stdout, /oversized entry 'crates\/c\/src\/a\.rs' is now <= 800 code lines/);
      },
    );
  });
});

describe('prod_lines_per_file.data_tables{} (exempt only while still a table)', () => {
  const REASON = 'one row per vocabulary entry, a table not logic';
  const withTables = (tables) => ({
    ...fullBaseline({}),
    prod_lines_per_file: { letter: 'a', disposition: 'permanent', armed: true, value: {}, oversized: {}, data_tables: tables },
  });
  const rows = (n) => '    ("key", "value"),\n'.repeat(n);

  it('a listed table over 1000 lines needs neither a ceiling nor a disposition', () => {
    within(tree({ [A]: rows(1200) }, withTables({ [A]: REASON })), (t) => {
      const ok = t.run('check');
      assert.equal(ok.status, 0, ok.stdout + ok.stderr);
    });
    within(tree({ [A]: rows(1200) }, withTables({})), (t) => {
      assert.notEqual(t.run('check').status, 0);
    });
  });

  it('turns red naming the function bound once a table defines more than 5 functions', () => {
    const fns = (n) => Array.from({ length: n }, (_, i) => `fn f${i}() {}\n`).join('');
    within(tree({ [A]: rows(1200) + fns(5) }, withTables({ [A]: REASON })), (t) => {
      assert.equal(t.run('check').status, 0);
    });
    within(tree({ [A]: rows(1200) + fns(6) }, withTables({ [A]: REASON })), (t) => {
      const red = t.run('check');
      assert.notEqual(red.status, 0);
      assert.match(red.stdout, /'crates\/c\/src\/a\.rs' defines 6 functions > 5/);
    });
  });

  it('turns red naming the branch bound above 2 branch points per 100 code lines', () => {
    const branches = (n) => 'if a && b { x }\n'.repeat(n);
    // each line is 2 branch points: 10 lines = 20 over 1000 code lines = 2.00
    within(tree({ [A]: rows(990) + branches(10) }, withTables({ [A]: REASON })), (t) => {
      assert.equal(t.run('check').status, 0);
    });
    within(tree({ [A]: rows(989) + branches(11) }, withTables({ [A]: REASON })), (t) => {
      const red = t.run('check');
      assert.notEqual(red.status, 0);
      assert.match(red.stdout, /22 branch points in 1000 code lines/);
    });
  });

  it('keywords inside strings and comments are not branches; `?` and ternaries are', () => {
    const quiet = '    ("if else match for while", "a && b || c"), // if while\n'.repeat(1000);
    within(tree({ [A]: quiet }, withTables({ [A]: REASON })), (t) => {
      assert.equal(t.run('check').status, 0);
    });
    within(tree({ [A]: rows(900) + 'let v = read()?;\n'.repeat(100) }, withTables({ [A]: REASON })), (t) => {
      assert.match(t.run('check').stdout, /100 branch points/);
    });
    const TS = 'packages/p/src/a.ts';
    within(tree({ [TS]: rows(900) + 'const v = a ? b : c;\nconst w = a?.b ?? c;\n'.repeat(50) }, withTables({ [TS]: REASON })), (t) => {
      assert.match(t.run('check').stdout, /50 branch points/);
    });
  });

  it('a TS table counts top-level function declarations and arrow functions', () => {
    const TS = 'packages/p/src/a.ts';
    const defs = 'export function a() {}\nexport const b = (x: number) => x;\nconst c = async () => {};\nfunction d() {}\nfunction e() {}\nfunction f() {}\n';
    within(tree({ [TS]: rows(1200) + defs }, withTables({ [TS]: REASON })), (t) => {
      assert.match(t.run('check').stdout, /defines 6 functions > 5/);
    });
  });

  it('is red for a listed file that is gone, a weak reason, or an oversized{} entry kept beside it', () => {
    within(tree({}, withTables({ [A]: REASON })), (t) => {
      assert.match(t.run('check').stdout, /data_tables entry 'crates\/c\/src\/a\.rs' no longer exists/);
    });
    within(tree({ [A]: rows(1200) }, withTables({ [A]: 'wip' })), (t) => {
      assert.match(t.run('check').stdout, /needs a written reason/);
    });
    const both = withTables({ [A]: REASON });
    both.prod_lines_per_file.oversized = { [A]: 'a disposition left behind by mistake' };
    within(tree({ [A]: rows(1200) }, both), (t) => {
      assert.match(t.run('check').stdout, /oversized entry 'crates\/c\/src\/a\.rs' is listed in data_tables/);
    });
  });
});

describe('a waiting disposition names a path check verifies is absent', () => {
  const REASON = 'a written reason of some length, retires with its wait';
  const GATE = 'crates/c/src/gate.rs';
  const oversized = (d) => withOversized({ [A]: 1100 }, { [A]: d });
  const run = (files, d, fn) => within(tree({ [A]: 1001, ...files }, oversized(d)), fn);

  it('is red once the waits_on path exists, saying the wait is over', () => {
    run({ [GATE]: 1 }, { reason: REASON, waits_on: GATE }, (t) => {
      const red = t.run('check');
      assert.notEqual(red.status, 0);
      assert.match(red.stdout, /oversized entry 'crates\/c\/src\/a\.rs' waits on 'crates\/c\/src\/gate\.rs', which now exists/);
      assert.match(red.stdout, /rewrite the disposition/);
    });
  });

  it('is green while the path is absent, and the object form still counts as the disposition', () => {
    run({}, { reason: REASON, waits_on: GATE }, (t) => {
      const ok = t.run('check');
      assert.equal(ok.status, 0, ok.stdout + ok.stderr);
    });
  });

  it('the reason inside an object is held to the same floor as a string', () => {
    run({}, { reason: 'wip', waits_on: GATE }, (t) => {
      assert.match(t.run('check').stdout, /has 1001 code lines > 1000/);
    });
  });

  it('a string that says "waits on" or "once ... exists" without waits_on is red', () => {
    run({}, 'The pipeline waits on the ops module before it can split.', (t) => {
      assert.match(t.run('check').stdout, /says it waits on something/);
    });
    run({}, 'Retires once the shared helper exists in its own file.', (t) => {
      assert.match(t.run('check').stdout, /says it waits on something/);
    });
    run({}, 'Mixed concerns that split once a second caller shows up.', (t) => {
      const ok = t.run('check');
      assert.equal(ok.status, 0, ok.stdout + ok.stderr);
    });
  });

  it('waits_on must be a path, not an empty value', () => {
    run({}, { reason: REASON, waits_on: '' }, (t) => {
      assert.match(t.run('check').stdout, /waits_on must be a repo-relative path/);
    });
  });

  it('applies to every row with an oversized{} map, children_per_dir included', () => {
    const files = Object.fromEntries(Array.from({ length: 30 }, (_, i) => [`crates/c/src/f${i}.rs`, 1]));
    const rows = (d) => ({
      ...fullBaseline({}),
      children_per_dir: { letter: 'b', disposition: 'permanent', armed: true, value: { 'crates/c/src': 30 }, oversized: { 'crates/c/src': d } },
    });
    within(tree({ ...files, [GATE]: 1 }, rows({ reason: REASON, waits_on: GATE })), (t) => {
      assert.match(t.run('check').stdout, /children_per_dir\.oversized entry 'crates\/c\/src' waits on/);
    });
    within(tree(files, rows({ reason: REASON, waits_on: GATE })), (t) => {
      const ok = t.run('check');
      assert.equal(ok.status, 0, ok.stdout + ok.stderr);
    });
  });
});

describe('docs — ARCHITECTURE.md Known-debt block', () => {
  const START = '<!-- ratchet:known-debt:start -->';
  const END = '<!-- ratchet:known-debt:end -->';

  function withDoc(t) {
    const p = path.join(t.root, 'ARCHITECTURE.md');
    fs.writeFileSync(p, `# Architecture\n\n## Known debt\n\n${START}\n${END}\n`);
    return p;
  }

  it('docs renders a table row per oversized{} entry that is still actually over 1000', () => {
    within(
      tree({ [A]: 1001 }, withOversized({ [A]: 1100 }, { [A]: 'grew past 1000 lines for a real reason, retires once X ships' })),
      (t) => {
        const docPath = withDoc(t);
        const r = t.run('docs');
        assert.equal(r.status, 0, r.stdout + r.stderr);
        const doc = fs.readFileSync(docPath, 'utf8');
        assert.match(doc, /crates\/c\/src\/a\.rs/);
        assert.match(doc, /1001/);
        assert.match(doc, /grew past 1000 lines for a real reason/);
      },
    );
  });

  it('docs lists the function dispositions before the file dispositions, and reads the object form', () => {
    const KEY = `${B}::big`;
    const rows = {
      ...withOversized({ [A]: 1100 }, { [A]: { reason: 'file reason, written as an object', waits_on: 'crates/c/src/absent.rs' } }),
      [FN_ROW]: { letter: 'g', disposition: 'permanent', armed: true, value: { [KEY]: 550 }, oversized: { [KEY]: 'function reason, one phase per step' } },
    };
    within(tree({ [A]: 1001, [B]: rustFn('big', 501) }, rows), (t) => {
      const docPath = withDoc(t);
      assert.equal(t.run('docs').status, 0);
      const doc = fs.readFileSync(docPath, 'utf8');
      assert.ok(doc.indexOf('function reason') < doc.indexOf('file reason'), doc);
      assert.match(doc, /Functions over 500 code lines/);
      assert.match(doc, /Files over 1000 code lines/);
      assert.equal(t.run('check').status, 0);
    });
  });

  it('docs --check reports stale without writing, and check itself fails the same way', () => {
    within(
      tree({ [A]: 1001 }, withOversized({ [A]: 1100 }, { [A]: 'grew past 1000 lines for a real reason, retires once X ships' })),
      (t) => {
        const docPath = withDoc(t);
        assert.equal(t.run('docs').status, 0);
        assert.equal(t.run('check').status, 0, 'block is fresh right after docs, so check must be green');

        // Drift: hand-edit the generated block after the fact, as if the
        // baseline had changed without anyone re-running `docs`.
        fs.writeFileSync(docPath, fs.readFileSync(docPath, 'utf8').replace('1001', '1'));
        const drifted = fs.readFileSync(docPath, 'utf8');

        const checkOnly = t.run('docs', '--check');
        assert.notEqual(checkOnly.status, 0);
        assert.match(checkOnly.stderr, /stale/);
        assert.equal(fs.readFileSync(docPath, 'utf8'), drifted, 'docs --check must not write');

        const red = t.run('check');
        assert.notEqual(red.status, 0);
        assert.match(red.stdout, /Known-debt block is stale/);
      },
    );
  });

  it('a repo tree with no ARCHITECTURE.md is not a check failure — nothing to compare against', () => {
    within(
      tree({ [A]: 1001 }, withOversized({ [A]: 1100 }, { [A]: 'grew past 1000 lines for a real reason, retires once X ships' })),
      (t) => {
        const ok = t.run('check');
        assert.equal(ok.status, 0, ok.stdout + ok.stderr);
      },
    );
  });
});

describe('row (f) direct_deps_per_crate', () => {
  const MANIFEST = 'crates/c/Cargo.toml';
  const CARGO = [
    '[package]',
    'name = "c"',
    'version = "0.1.0"',
    'edition = "2021"',
    '',
    "[dependencies] # it's the production table",
    'serde = { version = "1", features = ["derive"] }',
    'regex.workspace = true',
    '"quoted-key" = "1"',
    'renamed = { package = "url", version = "2" }',
    'url1 = { package = "url", version = "1" }',
    '',
    "[target.'cfg(unix)'.dependencies]",
    'libc = "0.2"',
    'serde = "1"',
    '',
    '[dev-dependencies]',
    'proptest = "1"',
    '',
    '[build-dependencies]',
    'cc = "1"',
    '',
  ].join('\n');

  // serde, regex, quoted-key, renamed, url1, libc: serde's second table is one
  // dependency, and two renames of one package are two.
  const COUNT = 6;

  function depTree(cargo, deps) {
    const t = tree({ 'crates/c/src/lib.rs': 1 }, fullBaseline({}, deps));
    fs.writeFileSync(path.join(t.root, 'Cargo.toml'), '[workspace]\nmembers = ["crates/c"]\nresolver = "2"\n\n[workspace.dependencies]\nregex = "1"\n');
    fs.writeFileSync(path.join(t.root, MANIFEST), cargo);
    return t;
  }

  it('is green at the baseline and red when a crate gains a dependency', () => {
    within(depTree(CARGO, { [MANIFEST]: COUNT }), (t) => {
      const ok = t.run('check');
      assert.equal(ok.status, 0, ok.stdout + ok.stderr);
    });
    within(depTree(CARGO.replace('[dev-dependencies]', 'anyhow = "1"\n\n[dev-dependencies]'), { [MANIFEST]: COUNT }), (t) => {
      const red = t.run('check');
      assert.notEqual(red.status, 0);
      assert.match(red.stdout, new RegExp(`'crates/c/Cargo\\.toml' grew to ${COUNT + 1} direct dependencies \\(baseline ${COUNT}\\)`));
    });
  });

  it('holds a new crate to an accept, whose trailer names the manifest', () => {
    within(depTree(CARGO, {}), (t) => {
      const red = t.run('check');
      assert.notEqual(red.status, 0);
      assert.match(red.stdout, new RegExp(`NEW 'crates/c/Cargo\\.toml' has ${COUNT} direct dependencies, not in baseline`));
      const ok = t.run('accept', 'direct_deps_per_crate', 'the new crate needs serde for its manifest format', '--path', MANIFEST);
      assert.equal(ok.status, 0, ok.stdout + ok.stderr);
      assert.match(ok.stdout, /Ratchet-Accept: direct_deps_per_crate crates\/c\/Cargo\.toml — the new crate needs serde/);
      assert.equal(t.baseline().rows.direct_deps_per_crate.value[MANIFEST], COUNT);
    });
  });

  it('tighten lowers the count when a dependency is dropped', () => {
    within(depTree(CARGO.replace('libc = "0.2"\n', ''), { [MANIFEST]: COUNT }), (t) => {
      const r = t.run('tighten');
      assert.equal(r.status, 0, r.stdout + r.stderr);
      assert.equal(t.baseline().rows.direct_deps_per_crate.value[MANIFEST], COUNT - 1);
    });
  });

  it('fails closed when cargo cannot read the manifests', () => {
    within(depTree('[package]\nname = "c"\nnot toml at all [\n', { [MANIFEST]: COUNT }), (t) => {
      const red = t.run('check');
      assert.notEqual(red.status, 0);
      assert.match(red.stderr, /cargo metadata.* failed/);
    });
  });
});

const SLICE_ROW = 'string_slices_per_crate';

/** One clippy JSON message line; `code` null is a different lint's neighbour. */
const clippyLine = (code, file, line, col = 1) =>
  JSON.stringify({
    reason: 'compiler-message',
    message: { code: code && { code }, spans: [{ file_name: file, line_start: line, column_start: col, is_primary: true }] },
  });
const STRING_SLICE = 'clippy::string_slice';

describe('row (h) string_slices_per_crate', () => {
  /** A tree whose baseline holds `value`, and a saved clippy file of `lines`. */
  function sliceTree(value, lines) {
    const t = tree({}, { ...fullBaseline({}), [SLICE_ROW]: { letter: 'h', disposition: 'permanent', armed: true, value } });
    const from = path.join(t.root, 'clippy.json');
    fs.writeFileSync(from, lines.join('\n') + '\n');
    return { ...t, slices: (...argv) => t.run('slices', '--from', from, ...argv) };
  }
  const build = (n, line = 1) => clippyLine(STRING_SLICE, `crates/moss-build/src/f${n}.rs`, line);

  it('counts only string_slice, per crate', () => {
    const t = sliceTree({ 'moss-build': 2, 'moss-cli': 1 }, [
      build(1), build(2), clippyLine(STRING_SLICE, 'crates/moss-cli/src/main.rs', 3),
      clippyLine('clippy::too_many_lines', 'crates/moss-build/src/g.rs', 1), clippyLine(null, 'crates/moss-build/src/h.rs', 1), 'not json',
    ]);
    within(t, () => {
      const r = t.slices();
      assert.equal(r.status, 0, r.stdout + r.stderr);
      assert.match(r.stdout, /moss-build\s+2 string slices \(baseline 2\)/);
      assert.match(r.stdout, /moss-cli\s+1 string slices \(baseline 1\)/);
    });
  });

  it('counts a span reported by both the lib and bin builds once', () => {
    const t = sliceTree({ 'moss-build': 1, 'moss-cli': 0 }, [build(1), build(1), build(1, 2)]);
    within(t, () => assert.match(t.slices().stdout, /moss-build\s+2 string slices/));
  });

  it('is red over the baseline, naming the crate and spans, and green at or under it', () => {
    const t = sliceTree({ 'moss-build': 1, 'moss-cli': 0 }, [build(1), build(2)]);
    within(t, () => {
      const red = t.slices();
      assert.equal(red.status, 1);
      assert.match(red.stdout, /RED moss-build\s+2 string slices \(baseline 1\); first: crates\/moss-build\/src\/f1\.rs:1:1, crates\/moss-build\/src\/f2\.rs:1:1/);
      assert.match(red.stdout, /moss-build over baseline/);
    });
    const under = sliceTree({ 'moss-build': 5, 'moss-cli': 0 }, [build(1)]);
    within(under, () => assert.equal(under.slices().status, 0));
  });

  it('--tighten lowers the baseline and never raises it', () => {
    const t = sliceTree({ 'moss-build': 5, 'moss-cli': 0 }, [build(1), build(2)]);
    within(t, () => {
      assert.equal(t.slices('--tighten').status, 0);
      assert.deepEqual(t.baseline().rows[SLICE_ROW].value, { 'moss-build': 2, 'moss-cli': 0 });
    });
    const up = sliceTree({ 'moss-build': 1, 'moss-cli': 0 }, [build(1), build(2)]);
    within(up, () => {
      const before = up.bytes();
      const r = up.slices('--tighten');
      assert.equal(r.status, 1);
      assert.match(r.stdout, /REFUSED to raise moss-build/);
      assert.deepEqual(up.bytes(), before);
    });
    const mixed = sliceTree({ 'moss-build': 1, 'moss-cli': 3 }, [build(1), build(2), clippyLine(STRING_SLICE, 'crates/moss-cli/src/main.rs', 3)]);
    within(mixed, () => {
      assert.equal(mixed.slices('--tighten').status, 1);
      assert.deepEqual(mixed.baseline().rows[SLICE_ROW].value, { 'moss-build': 1, 'moss-cli': 1 });
    });
  });

  it('--accept raises to the measured count and prints the trailer; a weak reason is refused', () => {
    const t = sliceTree({ 'moss-build': 1, 'moss-cli': 0 }, [build(1), build(2)]);
    within(t, () => {
      const before = t.bytes();
      const weak = t.slices('--accept', 'wip');
      assert.equal(weak.status, 1);
      assert.match(weak.stderr, /reason too weak/);
      assert.deepEqual(t.bytes(), before);
      const ok = t.slices('--accept', 'a new slice on checked ASCII input');
      assert.equal(ok.status, 0, ok.stderr);
      assert.match(ok.stdout, new RegExp(`Ratchet-Accept: ${SLICE_ROW} moss-build — a new slice on checked ASCII input`));
      assert.deepEqual(t.baseline().rows[SLICE_ROW].value, { 'moss-build': 2, 'moss-cli': 0 });
    });
  });

  it('check validates the row shape and does not run cargo', () => {
    const bad = sliceTree({ 'moss-build': -1 }, []);
    within(bad, () => {
      const r = bad.run('check');
      assert.equal(r.status, 1);
      assert.match(r.stdout, /'moss-build' must be a non-negative integer/);
      assert.match(r.stdout, /'moss-cli' must be a non-negative integer/);
    });
    const good = sliceTree({ 'moss-build': 3, 'moss-cli': 0 }, []);
    within(good, () => {
      // No cargo project here: a run of clippy would fail the check.
      const r = good.run('check');
      assert.equal(r.status, 0, r.stdout + r.stderr);
      assert.match(r.stdout, /\(h\) string_slices_per_crate\s+measured by `ratchet\.mjs slices`/);
    });
  });
});
