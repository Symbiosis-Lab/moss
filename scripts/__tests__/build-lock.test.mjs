// Gates share one scratch-site build through a lock directory; a waiter must not time out on a lock nobody holds.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { takeOrAwait } from '../../tests/e2e/helpers/build-lock.mjs';

const fast = { deadlineMs: 1_500, graceMs: 200, pollMs: 20 };

function lockWith(pid) {
  const lock = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'build-lock-')), 'site', 'stamp.json.lock');
  fs.mkdirSync(lock, { recursive: true });
  if (pid !== undefined) fs.writeFileSync(path.join(lock, 'pid'), String(pid));
  return lock;
}

const exitedPid = () => spawnSync(process.execPath, ['-e', '']).pid;

test('a free lock is taken and names its holder', () => {
  const lock = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'build-lock-')), 'a', 'b.lock');
  assert.deepEqual(takeOrAwait(lock, () => null, fast), { held: true });
  assert.equal(fs.readFileSync(path.join(lock, 'pid'), 'utf8'), String(process.pid));
});

test('a lock left by a killed holder is taken, not waited out', () => {
  const lock = lockWith(exitedPid());
  const t0 = Date.now();
  assert.deepEqual(takeOrAwait(lock, () => null, fast), { held: true });
  assert.ok(Date.now() - t0 < 500, `took ${Date.now() - t0} ms`);
  assert.equal(fs.readFileSync(path.join(lock, 'pid'), 'utf8'), String(process.pid));
});

test('a lock with no pid is taken once it is older than the grace period', () => {
  const lock = lockWith();
  const old = new Date(Date.now() - 1_000);
  fs.utimesSync(lock, old, old);
  assert.deepEqual(takeOrAwait(lock, () => null, fast), { held: true });
});

test("a live holder's build is waited for and its result returned", () => {
  const lock = lockWith(process.pid);
  let calls = 0;
  assert.deepEqual(takeOrAwait(lock, () => (++calls > 3 ? '/built' : null), fast), { held: false, value: '/built' });
});

test('a live holder that never publishes runs out the deadline', () => {
  const lock = lockWith(process.pid);
  assert.throws(() => takeOrAwait(lock, () => null, { ...fast, deadlineMs: 200, tag: '[g]' }),
    /^Error: \[g\] timed out waiting for another process to build the scratch site$/);
});

test('a lock released without a result is taken by the waiter', () => {
  const lock = lockWith(process.pid);
  let calls = 0;
  const ready = () => {
    if (++calls === 2) fs.rmSync(lock, { recursive: true, force: true });
    return null;
  };
  assert.deepEqual(takeOrAwait(lock, ready, fast), { held: true });
});
