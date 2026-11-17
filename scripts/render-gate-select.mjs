#!/usr/bin/env node
// Skip a render gate whose inputs are byte-for-byte those of a green run.
//
//   node scripts/render-gate-select.mjs check <gate> [project]          exit 0: skip (prints why), 1: run
//   node scripts/render-gate-select.mjs record <gate> [project] <log>   store a green run's input set
//
// The store, MOSS_RENDER_GATE_CACHE (default ~/.cache/moss/render-gates), is
// shared by worktrees: paths are relative and the digest covers content only.
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const KEEP = 20;
const SITES = 'target/test-tmp/';
const SPECS = 'tests/render-gates/';

/** The spec name a gate's config matches, as define-gate-config builds `testMatch`. */
export function specPattern(root, gate) {
  let name = gate;
  try {
    const m = fs.readFileSync(path.join(root, 'playwright', `${gate}.config.ts`), 'utf8').match(/specName:\s*['"]([^'"]+)['"]/);
    if (m) name = m[1];
  } catch {
    // No config: the gate name is the spec name.
  }
  return `${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\.spec\\.ts$`;
}

export function cacheDir(env = process.env) {
  return env.MOSS_RENDER_GATE_CACHE || path.join(os.homedir(), '.cache/moss/render-gates');
}

const sha = (data) => createHash('sha256').update(data).digest('hex');

function fileHash(abs) {
  try {
    return sha(fs.readFileSync(abs));
  } catch {
    return 'absent';
  }
}

function treeHash(dir) {
  const h = createHash('sha256');
  const walk = (rel) => {
    for (const name of fs.readdirSync(path.join(dir, rel)).sort()) {
      const child = path.join(rel, name);
      if (fs.statSync(path.join(dir, child)).isDirectory()) walk(child);
      else h.update(child).update('\0').update(fs.readFileSync(path.join(dir, child)));
    }
  };
  walk('');
  return h.digest('hex');
}

/** The built tree a scratch site's stamp points at. */
function siteBuiltDir(root, site) {
  try {
    const stamp = JSON.parse(fs.readFileSync(path.join(root, SITES, site, '.moss/build-stamp.json'), 'utf8'));
    return stamp.builtDir;
  } catch {
    return null;
  }
}

/**
 * The input set from a recorder log: `[kind, relpath]` pairs, sorted and unique.
 * Kinds: read (content), exists (present or not), dir (names), site (a scratch
 * site's built tree), bin (the moss-cli binary), specs (a test folder's listing,
 * as far as the gate's spec pattern reads it; the pattern is the third field).
 */
export function inputsFromLog(root, logText, mossBin = '', gate = '') {
  const pattern = gate ? specPattern(root, gate) : '';
  const set = new Set();
  const add = (kind, rel) => set.add(`${kind}\t${rel}`);
  for (const line of logText.split('\n')) {
    const tab = line.indexOf('\t');
    if (tab < 0) continue;
    const kind = line.slice(0, tab);
    const target = line.slice(tab + 1);
    if (kind === 'exec') {
      if (mossBin && target.includes(mossBin) && (!/ build /.test(target) || /--serve/.test(target))) add('bin', '');
      continue;
    }
    const abs = path.resolve(root, target);
    const rel = path.relative(root, abs);
    if (rel.startsWith('..') || path.isAbsolute(rel) || rel.split(path.sep).includes('node_modules')) continue;
    if (rel.startsWith(SITES)) {
      const site = rel.slice(SITES.length).split(path.sep)[0];
      if (site && fs.existsSync(path.join(root, SITES, site, '.moss/build-stamp.json'))) {
        add('site', site);
        continue;
      }
    }
    if (kind === 'dir' && pattern && (rel + path.sep).startsWith(SPECS)) {
      add('specs', `${rel}\t${pattern}`);
      continue;
    }
    add(kind, rel);
  }
  return [...set].sort().map((l) => l.split('\t'));
}

/** What every gate depends on whatever it reads: the browsers, Playwright, Node, the lockfile. */
function environment(root) {
  const pw = path.join(root, 'node_modules/playwright-core');
  return [
    process.version,
    process.platform,
    process.arch,
    fileHash(path.join(pw, 'package.json')),
    fileHash(path.join(pw, 'browsers.json')),
    fileHash(path.join(root, 'pnpm-lock.yaml')),
  ].join('\n');
}

/** A folder's names, those `keep` accepts; 'absent' when it cannot be listed. */
function listing(abs, keep = () => true) {
  try {
    return fs.readdirSync(abs, { withFileTypes: true }).filter(keep).map((d) => d.name).sort().join('/');
  } catch {
    return 'absent';
  }
}

function valueOf(root, kind, rel, pattern, mossBin) {
  const abs = path.join(root, rel);
  switch (kind) {
    case 'read': return fileHash(abs);
    case 'exists': return fs.existsSync(abs) ? '1' : '0';
    case 'dir': return listing(abs);
    case 'specs': {
      const accepts = new RegExp(pattern);
      return listing(abs, (d) => d.isDirectory() || accepts.test(path.join(abs, d.name)));
    }
    case 'site': {
      const built = siteBuiltDir(root, rel);
      return built && fs.existsSync(built) ? treeHash(built) : 'absent';
    }
    case 'bin': return mossBin ? fileHash(mossBin) : 'absent';
    default: return 'unknown';
  }
}

/** `memo` carries values across the entries one check compares, so a site tree is hashed once. */
export function digest(root, inputs, mossBin = '', memo = new Map()) {
  const h = createHash('sha256').update(environment(root));
  for (const [kind, rel, pattern = ''] of inputs) {
    const key = `${kind}\t${rel}\t${pattern}`;
    if (!memo.has(key)) memo.set(key, valueOf(root, kind, rel, pattern, mossBin));
    h.update(`${key}\t${memo.get(key)}\n`);
  }
  return h.digest('hex');
}

function storePath(gate, project, env) {
  return path.join(cacheDir(env), `${gate}@${project || 'all'}.json`);
}

function load(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    return [];
  }
}

/** The green entry whose inputs hash the same in `root` now, or null. */
export function check(root, gate, project, { env = process.env, mossBin = env.MOSS_BIN || '' } = {}) {
  const memo = new Map();
  for (const entry of load(storePath(gate, project, env))) {
    if (digest(root, entry.inputs, mossBin, memo) === entry.digest) return entry;
  }
  return null;
}

export function record(root, gate, project, logText, { env = process.env, mossBin = env.MOSS_BIN || '', head = '' } = {}) {
  const inputs = inputsFromLog(root, logText, mossBin, gate);
  // A run whose recorder never loaded would store an empty set that matches forever.
  const config = path.join('playwright', `${gate}.config.ts`);
  if (!inputs.some(([kind, rel]) => kind === 'read' && rel === config)) {
    throw new Error(`${gate}: the recorded run never read ${config}, so its inputs are unknown; not stored`);
  }
  const entry = { digest: digest(root, inputs, mossBin), head, at: new Date().toISOString(), inputs };
  const file = storePath(gate, project, env);
  const kept = load(file).filter((e) => e.digest !== entry.digest);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const tmp = `${file}.${process.pid}`;
  fs.writeFileSync(tmp, JSON.stringify([entry, ...kept].slice(0, KEEP)));
  fs.renameSync(tmp, file);
  return entry;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [cmd, gate, ...rest] = process.argv.slice(2);
  const root = process.cwd();
  if (cmd === 'check' && gate) {
    const hit = check(root, gate, rest[0] || '');
    if (hit) console.log(`SKIP ${gate}: its ${hit.inputs.length} inputs are those of a green run (${hit.head || 'unknown head'}, ${hit.at})`);
    process.exit(hit ? 0 : 1);
  } else if (cmd === 'record' && gate && rest.length) {
    const log = rest.pop();
    const head = process.env.GATE_HEAD || '';
    const e = record(root, gate, rest[0] || '', fs.readFileSync(log, 'utf8'), { head });
    console.log(`recorded ${gate}: ${e.inputs.length} inputs`);
  } else {
    console.error('usage: render-gate-select.mjs check <gate> [project] | record <gate> [project] <log>');
    process.exit(2);
  }
}
