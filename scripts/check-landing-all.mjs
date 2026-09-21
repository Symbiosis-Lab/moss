#!/usr/bin/env node
// The one command for the whole regression net: starts a single server for
// one build (or reuses a URL passed on the command line, same as every
// individual check), then runs each check against it in sequence — never in
// parallel, which has corrupted timing results before — and prints one
// table instead of fifteen separate runs.
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { resolveBaseURL } from './landing-harness.mjs';

const HERE = fileURLToPath(new URL('.', import.meta.url));

const CHECKS = [
  'check-landing-cold-bottom.mjs',
  'check-landing-intro-title.mjs',
  'check-landing-mobile-handoff.mjs',
  'check-landing-mobile.mjs',
  'check-landing-pin.mjs',
  'check-landing-readiness.mjs',
  'check-landing-subscription.mjs',
  'check-landing-text-track.mjs',
  'check-landing-transitions.mjs',
  'check-landing-wheel-tail.mjs',
  'check-site-preview.mjs',
  'check-docs-footer-icon.mjs',
  'check-docs-media.mjs',
  'check-favicon-theme.mjs',
  'check-landing-structure.mjs',
  'check-landing-invariants.mjs',
];

const ENGINE_RE = /\b(chromium|webkit)\b/g;
function engineMentions(text) {
  const set = new Set();
  for (const m of text.matchAll(ENGINE_RE)) set.add(m[1].toLowerCase());
  return set;
}
function engineLineMentions(text) {
  const set = new Set();
  for (const line of text.split('\n')) {
    const m = /^(chromium|webkit)\b/.exec(line.trim());
    if (m) set.add(m[1].toLowerCase());
  }
  return set;
}

function runOne(script, baseURL) {
  return new Promise((resolveRun) => {
    const start = Date.now();
    const child = spawn(process.execPath, [`${HERE}${script}`, baseURL], {
      env: process.env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let stdout = '', stderr = '';
    child.stdout.on('data', (d) => { stdout += d; });
    child.stderr.on('data', (d) => { stderr += d; });
    child.on('close', (code) => {
      resolveRun({ script, code, seconds: (Date.now() - start) / 1000, stdout, stderr });
    });
  });
}

// Best-effort attribution, not a guaranteed one: reads what the script
// itself printed rather than re-deriving engines from its source. A script
// that names an engine at the start of a line (every check-landing-*.mjs
// script's own pass message) counts as that engine passing; the
// JSON-report scripts (check-docs-*, check-favicon-theme) name engines only
// inside their JSON body, so on success every engine mentioned anywhere in
// the output counts. On failure, only stderr decides the failing engine —
// an engine already credited with its own pass line must not also be
// re-flagged failed just because its name still appears somewhere in
// stdout (the JSON-report scripts repeat every engine's name there
// regardless of outcome); a failure that names no new engine gets one '-'
// row rather than pinning the blame on whichever engine happened to pass.
function attributeEngines({ code, stdout, stderr }) {
  const passedLines = engineLineMentions(stdout);
  if (code === 0) {
    const mentioned = engineMentions(stdout + '\n' + stderr);
    const engines = passedLines.size ? passedLines : mentioned.size ? mentioned : new Set(['-']);
    return [...engines].map((engine) => ({ engine, status: 'pass' }));
  }
  const failedMentions = engineMentions(stderr);
  const newlyFailed = new Set([...failedMentions].filter((e) => !passedLines.has(e)));
  const failEngines = newlyFailed.size ? newlyFailed : new Set(['-']);
  return [
    ...[...passedLines].map((engine) => ({ engine, status: 'pass' })),
    ...[...failEngines].map((engine) => ({ engine, status: 'fail' })),
  ];
}

const { baseURL, close } = await resolveBaseURL(process.argv[2]);
console.log(`landing-all: ${baseURL}`);
const rows = [];
let anyFailed = false;
try {
  for (const script of CHECKS) {
    const result = await runOne(script, baseURL);
    if (result.code !== 0) {
      anyFailed = true;
      process.stderr.write(`\n--- ${script} (exit ${result.code}) ---\n${result.stderr || result.stdout}\n`);
    }
    // Printed as each script finishes, not only in the final table: a
    // fifteen-script run takes minutes, and a table that only appears at
    // the very end gives no sign the command is making progress.
    console.log(`  ${result.code === 0 ? 'pass' : 'FAIL'}  ${script}  ${result.seconds.toFixed(1)}s`);
    for (const { engine, status } of attributeEngines(result)) {
      rows.push({ script: script.replace(/\.mjs$/, ''), engine, status, seconds: result.seconds.toFixed(1) });
    }
  }
} finally {
  await close();
}

const widths = { script: Math.max(6, ...rows.map((r) => r.script.length)), engine: Math.max(6, ...rows.map((r) => r.engine.length)) };
const pad = (s, w) => String(s).padEnd(w);
console.log('');
console.log(`${pad('script', widths.script)}  ${pad('engine', widths.engine)}  status  seconds`);
for (const r of rows) console.log(`${pad(r.script, widths.script)}  ${pad(r.engine, widths.engine)}  ${r.status.padEnd(6)}  ${r.seconds}`);

const perScriptSeconds = [...new Map(rows.map((r) => [r.script, r.seconds])).values()].reduce((a, b) => a + Number(b), 0);
console.log(`\ntotal wall time: ${perScriptSeconds.toFixed(1)}s`);

process.exitCode = anyFailed ? 1 : 0;
