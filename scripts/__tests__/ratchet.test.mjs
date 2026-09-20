// Tests for scripts/ratchet.mjs. Run: node --test scripts/__tests__/ratchet.test.mjs
//
// The script keeps its baseline beside itself and reads the tree from --root,
// so each test copies it into a throwaway tree with a baseline of its own and
// spawns it there. Nothing here can write to the real baseline.
//
// What is under test is `accept`/`tighten --path`: without it, `accept` raised
// every over-baseline entry of a row to its current count, so a reason written
// for one file's growth also accepted the growth of every other file that was
// over. Two landings (d16e5ff, 4fdb3cb) hand-edited accepts[] to get around it.

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
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
    fs.writeFileSync(path.join(root, rel), '// filler\n'.repeat(lines));
  }
  fs.mkdirSync(path.join(root, 'scripts'));
  fs.copyFileSync(SCRIPT, path.join(root, 'scripts', 'ratchet.mjs'));
  const baselineFile = path.join(root, 'scripts', 'ratchet-baseline.open.json');
  fs.writeFileSync(baselineFile, JSON.stringify({ rows, accepts: [] }, null, 2) + '\n');
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

const perPath = (value) => ({ [ROW]: { letter: 'a', disposition: 'permanent', armed: true, value } });

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

describe('accept --path', () => {
  it('raises only the named file and leaves the other over-baseline file alone', () => {
    within(twoOverOneAtBaseline(), (t) => {
      const r = t.run('accept', ROW, 'a grew on purpose', '--path', A);

      assert.equal(r.status, 0, r.stderr);
      const value = t.baseline().rows[ROW].value;
      assert.equal(value[A], 900);
      assert.equal(value[B], 900, "b's growth was never accepted, so its baseline must not move");
      assert.equal(value[C], 1000);
      const entry = t.baseline().accepts.at(-1);
      assert.deepEqual(entry.from, { [A]: 850 });
      assert.deepEqual(entry.to, { [A]: 900 });
    });
  });

  it('keeps the {row, from, to, reason, date} entry format', () => {
    within(twoOverOneAtBaseline(), (t) => {
      t.run('accept', ROW, 'because', 'reasons', '--path', A);

      const entry = t.baseline().accepts.at(-1);
      assert.deepEqual(Object.keys(entry), ['row', 'from', 'to', 'reason', 'date']);
      assert.equal(entry.row, ROW);
      assert.equal(entry.reason, 'because reasons');
      assert.match(entry.date, /^\d{4}-\d{2}-\d{2}$/);
    });
  });

  it('takes several --path flags and raises exactly those', () => {
    within(tree({ [A]: 900, [B]: 950, [C]: 1010 }, perPath({ [A]: 850, [B]: 900, [C]: 1000 })), (t) => {
      const r = t.run('accept', ROW, 'two of three', '--path', A, '--path', B);

      assert.equal(r.status, 0, r.stderr);
      const value = t.baseline().rows[ROW].value;
      assert.deepEqual([value[A], value[B], value[C]], [900, 950, 1000]);
    });
  });

  it('records a new file over the threshold as from: null', () => {
    within(tree({ [A]: 900, [B]: 950 }, perPath({ [A]: 900 })), (t) => {
      const r = t.run('accept', ROW, 'new file', '--path', B);

      assert.equal(r.status, 0, r.stderr);
      assert.equal(t.baseline().rows[ROW].value[B], 950);
      assert.deepEqual(t.baseline().accepts.at(-1).from, { [B]: null });
    });
  });

  it('folds ./ and absolute spellings to the baseline key', () => {
    for (const spelling of [`./${A}`, (root) => path.join(root, A)]) {
      within(twoOverOneAtBaseline(), (t) => {
        const arg = typeof spelling === 'function' ? spelling(t.root) : spelling;
        const r = t.run('accept', ROW, 'spelling', '--path', arg);

        assert.equal(r.status, 0, r.stderr);
        assert.equal(t.baseline().rows[ROW].value[A], 900);
        assert.equal(t.baseline().rows[ROW].value[B], 900);
      });
    }
  });

  it('errors, naming the path and writing nothing, when the file is not over its baseline', () => {
    within(twoOverOneAtBaseline(), (t) => {
      const before = t.bytes();
      const r = t.run('accept', ROW, 'nothing grew', '--path', C);

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
      const r = t.run('accept', ROW, 'typo', '--path', 'crates/c/src/nope.rs');

      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /nope\.rs.*absent/);
    });
  });

  it('refuses --path on a scalar row', () => {
    within(
      tree({ [A]: 10 }, { mirror_markers: { letter: 'e', disposition: 'permanent', armed: true, value: 0 } }),
      (t) => {
        const r = t.run('accept', 'mirror_markers', 'why', '--path', A);

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

describe('accept without --path', () => {
  // Kept as it was: a whole-row accept is what the initial baseline used. This
  // pins what `--path` is the alternative to.
  it('still raises every over-baseline entry of the row', () => {
    within(twoOverOneAtBaseline(), (t) => {
      const r = t.run('accept', ROW, 'whole row');

      assert.equal(r.status, 0, r.stderr);
      const value = t.baseline().rows[ROW].value;
      assert.deepEqual([value[A], value[B], value[C]], [900, 950, 1000]);
    });
  });
});

describe('tighten --path', () => {
  it('lowers only the named file although another could be lowered too', () => {
    within(tree({ [A]: 810, [B]: 820 }, perPath({ [A]: 900, [B]: 900 })), (t) => {
      const r = t.run('tighten', '--path', A);

      assert.equal(r.status, 0, r.stderr);
      const value = t.baseline().rows[ROW].value;
      assert.equal(value[A], 810);
      assert.equal(value[B], 900);
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
