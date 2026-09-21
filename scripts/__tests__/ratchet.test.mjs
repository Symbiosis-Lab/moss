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
    fs.writeFileSync(path.join(root, rel), '// filler\n'.repeat(lines));
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

/** A baseline with all three rows present — `check` self-check 1 requires it. */
const fullBaseline = (value) => ({
  ...perPath(value),
  children_per_dir: { letter: 'b', disposition: 'permanent', armed: true, value: {} },
  mirror_markers: { letter: 'e', disposition: 'permanent', armed: true, value: 0 },
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
  it('flags a historical commit whose raise has no trailer', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      t.writeBaseline(perPath({ [A]: 900 }));
      t.git('add', 'scripts/ratchet-baseline.open.json');
      t.git('commit', '-q', '-m', 'bump A, no trailer');

      const r = t.run('verify-accepts', '--range', 'HEAD~1..HEAD');
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /prod_lines_per_file crates\/c\/src\/a\.rs/);
    });
  });

  it('passes when the historical commit carries a matching trailer', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
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

  it('needs an A..B range, not a bare ref', () => {
    within(gitTree({ [A]: 900 }, perPath({ [A]: 850 })), (t) => {
      const r = t.run('verify-accepts', '--range', 'HEAD');
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /usage/);
    });
  });
});
