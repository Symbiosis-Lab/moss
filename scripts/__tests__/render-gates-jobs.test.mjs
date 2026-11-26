// render-gates.sh --jobs: how many browsers run at once, and which gates never share the machine.
// A stub `npx` stands in for Playwright and logs when each gate starts and ends; a stub `getconf` sets the CPU count.
// The memory grant is fixed so the lane count does not depend on the machine the tests run on.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT = fileURLToPath(new URL('../render-gates.sh', import.meta.url));
const ROOT = path.dirname(path.dirname(SCRIPT));
const SERIAL = ['marquee', 'places-explorer-ring', 'browser-editor', 'focus-point', 'address-bar'];
const SPREAD = ['nav-island', 'edge-clamp', 'comments-cascade', 'map-tiles', 'figure-grid', 'hero-slide'];

function setup(red = '', sleep = '0.3') {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'render-gates-jobs-'));
  const log = path.join(dir, 'events');
  fs.writeFileSync(path.join(dir, 'npx'), `#!/bin/bash
for a in "$@"; do case $a in playwright/*) g=$(basename "$a" .config.ts);; esac; done; w=$RENDER_GATES_WORKERS
echo "start $g $w $$" >> "${log}"; [ "$g" = "$KILL_LANE" ] && kill -9 $PPID; sleep ${sleep}; echo "end $g" >> "${log}"
[ "$g" = "${red}" ] && exit 1; exit 0
`, { mode: 0o755 });
  fs.writeFileSync(path.join(dir, 'getconf'), '#!/bin/bash\necho "${STUB_CPUS:-16}"\n', { mode: 0o755 });
  const env = {
    ...process.env, PATH: `${dir}:${process.env.PATH}`, MOSS_BIN: '/usr/bin/true', RENDER_GATES_MEM_GB: '32',
    RENDER_GATES_TIMES: path.join(dir, 'times.tsv'), RENDER_GATES_LOGS: path.join(dir, 'logs'),
  };
  const events = () => (fs.existsSync(log) ? fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => l.split(' ')) : []);
  return { dir, env, events, done: () => fs.rmSync(dir, { recursive: true, force: true }) };
}

function run(args, env = {}, red = '') {
  const s = setup(red);
  const r = spawnSync('/bin/bash', [SCRIPT, ...args], { encoding: 'utf8', env: { ...s.env, ...env } });
  const events = s.events();
  s.done();
  return { status: r.status, stdout: r.stdout, stderr: r.stderr, events };
}

// The most gates running at once, overall and among `only`.
function peak(events, only = null) {
  let now = 0, max = 0;
  for (const [kind, gate] of events) {
    if (only && !only.includes(gate)) continue;
    now += kind === 'start' ? 1 : -1;
    max = Math.max(max, now);
  }
  return max;
}

test('--jobs N holds at most N one-worker browsers, and the timing gates run alone after the rest', () => {
  const { status, events } = run(['--jobs', '3', ...SPREAD, ...SERIAL]);
  assert.equal(status, 0);
  const starts = events.filter(([k]) => k === 'start');
  assert.equal(starts.length, SPREAD.length + SERIAL.length);
  assert.ok(starts.filter(([, g]) => SPREAD.includes(g)).every(([, , w]) => w === '1'), 'every sharded run has one worker');
  assert.ok(starts.filter(([, g]) => SERIAL.includes(g)).every(([, , w]) => w === '4'), 'a gate run alone keeps four workers');
  const lastSpreadEnd = events.findLastIndex(([k, g]) => k === 'end' && SPREAD.includes(g));
  const firstSerialStart = events.findIndex(([k, g]) => k === 'start' && SERIAL.includes(g));
  assert.ok(lastSpreadEnd < firstSerialStart, 'no timing gate runs beside another gate');
  assert.equal(peak(events, SERIAL), 1, 'timing gates never overlap');
  assert.ok(peak(events) <= 3, `at most 3 at once, saw ${peak(events)}`);
  assert.ok(peak(events) > 1, 'the run is parallel');
});

test('a red gate turns the run red and is named', () => {
  const { status, stdout } = run(['--jobs', '2', 'nav-island', 'edge-clamp'], {}, 'edge-clamp');
  assert.equal(status, 1);
  assert.match(stdout, /^RED edge-clamp$/m);
  assert.doesNotMatch(stdout, /^RED nav-island$/m);
});

test('RENDER_GATES_MEM_GB, by default half the machine, caps --jobs N and sizes --jobs auto', () => {
  const capped = run(['--jobs', '6', ...SPREAD], { RENDER_GATES_MEM_GB: '17' });
  assert.match(capped.stderr, /running 3/);
  assert.ok(peak(capped.events) <= 3, `at most 3 at once, saw ${peak(capped.events)}`);
  assert.match(run(['--jobs', '6', ...SPREAD], { RENDER_GATES_MEM_GB: '8' }).stderr, /running 1/, 'under one heavy lane, one lane');
  assert.match(run(['--jobs', 'auto', ...SPREAD], { RENDER_GATES_MEM_GB: '25' }).stdout, /on 5 lanes/);
  assert.match(run(['--jobs', 'auto', ...SPREAD], { RENDER_GATES_MEM_GB: '29' }).stdout, /on 6 lanes/);
  const auto = run(['--jobs', 'auto', ...SPREAD], { RENDER_GATES_MEM_GB: '13' });
  assert.match(auto.stdout, /on 2 lanes/);
  const unset = run(['--jobs', '100000', 'nav-island'], { RENDER_GATES_MEM_GB: '' });
  assert.match(unset.stderr, /running \d+$/m, 'with no memory set, half the machine caps --jobs N');
});

test('--jobs auto takes no more lanes than CPUs', () => {
  assert.match(run(['--jobs', 'auto', ...SPREAD], { RENDER_GATES_MEM_GB: '29', STUB_CPUS: '4' }).stdout, /on 4 lanes/);
});

test('a fractional grant sizes workers like its whole part', () => {
  const { status, events } = run(['nav-island'], { RENDER_GATES_MEM_GB: '5.5' });
  assert.equal(status, 0);
  assert.deepEqual(events.filter(([k]) => k === 'start').map(([, g, w]) => `${g}:${w}`), ['nav-island:2']);
});

test('one lane runs the gates in order with a capped worker count', () => {
  const { status, events } = run(['nav-island', 'edge-clamp']);
  assert.equal(status, 0);
  assert.equal(peak(events), 1);
  assert.deepEqual(events.filter(([k]) => k === 'start').map(([, g, w]) => `${g}:${w}`), ['nav-island:4', 'edge-clamp:4']);
});

test('a grant of one lane runs one browser, whatever --jobs asks', () => {
  for (const args of [['--jobs', '4'], []]) {
    const { status, events } = run([...args, 'nav-island', 'marquee'], { RENDER_GATES_MEM_GB: '2' });
    assert.equal(status, 0);
    assert.deepEqual(events.filter(([k]) => k === 'start').map(([, g, w]) => `${g}:${w}`), ['nav-island:1', 'marquee:1'], args.join(' '));
  }
});

test('the grant sizes a gate run alone by worker, apart from the lanes', () => {
  const { status, events } = run(['nav-island'], { RENDER_GATES_MEM_GB: '8' });
  assert.equal(status, 0);
  assert.deepEqual(events.filter(([k]) => k === 'start').map(([, g, w]) => `${g}:${w}`), ['nav-island:4']);
});

test('the pool starts the gates the last run timed longest first', () => {
  const s = setup();
  fs.writeFileSync(s.env.RENDER_GATES_TIMES, 'nav-island\t10\nedge-clamp\t50\nmap-tiles\t30\n');
  const r = spawnSync('/bin/bash', [SCRIPT, '--jobs', '2', 'nav-island', 'map-tiles', 'figure-grid', 'edge-clamp'], { encoding: 'utf8', env: s.env });
  const queue = fs.readFileSync(path.join(r.stdout.match(/logs in (\S+)/)[1], 'queue'), 'utf8').trim().split('\n');
  s.done();
  assert.deepEqual(queue, ['figure-grid', 'edge-clamp', 'map-tiles', 'nav-island'], 'an untimed gate counts 60 s');
});

test('a lane killed outright still leaves every gate named and the run summed up', () => {
  const s = setup();
  const r = spawnSync('/bin/bash', [SCRIPT, '--jobs', '2', 'nav-island', 'edge-clamp', 'map-tiles', 'figure-grid'], { encoding: 'utf8', env: { ...s.env, KILL_LANE: 'nav-island' } });
  const times = fs.existsSync(s.env.RENDER_GATES_TIMES) ? fs.readFileSync(s.env.RENDER_GATES_TIMES, 'utf8') : '';
  s.done();
  assert.equal(r.status, 1);
  assert.match(r.stdout, /^RED nav-island$/m);
  assert.match(r.stdout, /── \d+ ran in/, 'the run printed its summary');
  assert.doesNotMatch(times, /^nav-island\t/m, 'a gate with no result records no time');
  assert.doesNotMatch(r.stderr, /No such file/);
  for (const g of ['edge-clamp', 'map-tiles', 'figure-grid']) {
    const named = new RegExp(`^RED ${g}$`, 'm').test(r.stdout), timed = new RegExp(`^${g}\t`, 'm').test(times);
    assert.ok(named !== timed, `${g} is either named red or timed green`);
  }
});

for (const [phase, gates] of [['the pool', ['nav-island', 'edge-clamp']], ['the timing gates', ['marquee']]]) {
  test(`an interrupt during ${phase} ends every Playwright it started`, async () => {
    const s = setup('', '30');
    const child = spawn('/bin/bash', [SCRIPT, '--jobs', '2', ...gates], { env: s.env, stdio: ['ignore', 'pipe', 'ignore'] });
    let out = '';
    child.stdout.on('data', (d) => { out += d; });
    while (s.events().length < gates.length) await new Promise((r) => setTimeout(r, 100));
    const sent = Date.now();
    child.kill('SIGTERM');
    const code = await new Promise((r) => child.on('exit', r));
    const took = Date.now() - sent;
    const logs = out.match(/logs in (\S+)/)?.[1];
    const kept = logs && fs.readdirSync(logs).filter((f) => f.endsWith('.log')).sort();
    await new Promise((r) => setTimeout(r, 300));
    const alive = s.events().map(([, , , pid]) => Number(pid)).filter((pid) => {
      try { process.kill(pid, 0); return true; } catch { return false; }
    });
    s.done();
    assert.equal(code, 143);
    assert.ok(took < 5000, `the run ended ${took} ms after the signal, not when its gates finished`);
    assert.deepEqual(alive, [], 'nothing outlives the run');
    assert.deepEqual(kept, gates.map((g) => `${g}.log`).sort(), 'the run said where it kept the started gates\' logs');
  });
}

test('each run keeps its logs in a folder of its own and deletes nothing else', () => {
  const s = setup();
  const logs = s.env.RENDER_GATES_LOGS;
  fs.mkdirSync(logs, { recursive: true });
  fs.writeFileSync(path.join(logs, 'keep'), '');
  const folder = () => spawnSync('/bin/bash', [SCRIPT, '--jobs', '2', 'nav-island'], { encoding: 'utf8', env: s.env }).stdout.match(/logs in (\S+)/)[1];
  const [a, b] = [folder(), folder()];
  const kept = fs.existsSync(path.join(logs, 'keep')) && fs.existsSync(path.join(a, 'nav-island.result'));
  s.done();
  assert.notEqual(a, b);
  assert.ok(kept, 'a later run deleted an earlier one, or a file of the caller');
});

test('a serial run never asks how much memory the machine has', () => {
  const s = setup();
  const asked = path.join(s.dir, 'asked');
  fs.writeFileSync(path.join(s.dir, 'sysctl'), `#!/bin/sh\ntouch "${asked}"\nexit 1\n`, { mode: 0o755 });
  const r = spawnSync('/bin/bash', [SCRIPT, 'nav-island'], { encoding: 'utf8', env: { ...s.env, RENDER_GATES_MEM_GB: '' } });
  const wasAsked = fs.existsSync(asked);
  s.done();
  assert.equal(r.status, 0, r.stderr);
  assert.ok(!wasAsked, 'sysctl was called');
});

// Real Playwright reads the cap from playwright/define-gate-config.ts: it caps only a fullyParallel config.
test('RENDER_GATES_WORKERS caps a parallel config and leaves a serialized one at one worker', () => {
  const workers = (gate) => {
    const r = spawnSync('npx', ['playwright', 'test', '-c', `playwright/${gate}.config.ts`, '--list', '--reporter=json'],
      { cwd: ROOT, encoding: 'utf8', env: { ...process.env, RENDER_GATES_WORKERS: '3' } });
    assert.equal(r.status, 0, r.stderr);
    return JSON.parse(r.stdout).config.workers;
  };
  assert.equal(workers('ambient-video'), 3);
  assert.equal(workers('term-index-layout'), 1);
});
