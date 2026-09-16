#!/usr/bin/env node
// ratchet.mjs — shrink-only architecture ratchet for THIS tree (the public
// open repo), ported from the desktop repo's scripts/ratchet.mjs.
//
// Landing-order step 8 (public half) of
// docs/archive/2026-09-16-boundary-gates-remeasured-for-dependency-model.md:
// once the desktop repo can no longer scan `open/crates` / `open/packages`
// (the flip retires the submodule), the size/marker debt those roots used to
// carry has to be ratcheted here instead, by this tree's own copy of the same
// three rows. Only the machinery those three rows need is ported — the
// desktop file also arms ~20 other rows (CSS selectors, raw `.emit(`, seam
// width, moss-seta payment call sites, …) that have no meaning in this repo
// and are not carried over.
//
// Row letters match the desktop file's own numbering, kept for continuity
// with docs that already cite them:
//   (a) prod_lines_per_file — files over 800 prod lines (Rust counted
//                             #[cfg(test)]-aware)
//   (b) children_per_dir    — direct children per directory (new dirs get a
//                             <=15 budget; any dir >=30 needs a written
//                             disposition in `oversized{}`)
//   (e) mirror_markers      — "keep in sync" / "must match" / "mirror of"
//                             comment markers
//
// Roots: the desktop rows scanned `open/crates` (+ `open/packages` for a/b
// only — mirror_markers never scanned packages) as one of several
// independently-checked roots. Here, with `open/` gone, that is simply this
// repo's own `crates/*/src` and `packages/*/src` — the `open/` prefix
// dropped, nothing else added (no `frontend/`, no `docs/`, no `src-tauri/`:
// those roots belong to the desktop repo's own copies of these rows).
//
// Commands (same shape as desktop's):
//   node scripts/ratchet.mjs check           exit 0 green / 1 red
//   node scripts/ratchet.mjs tighten         lower baseline; NEVER raises
//   node scripts/ratchet.mjs accept <row> <reason>   the ONLY way a baseline
//                                             grows; logs {row,from,to,reason,date}
//
// Baseline: scripts/ratchet-baseline.open.json, next to this script.
//
// Self-checks (run at the start of every `check`; any failure = red):
//   1. every baseline row has a disposition, matches the row table below;
//   2. stale baseline entries (path no longer on disk) go red — fix with `tighten`;
//   2b. children_per_dir: any dir at/over 30 direct children needs a written
//       reason in the baseline's children_per_dir.oversized{} (NORTH-STAR's
//       "mandatory split" line) — ported because it is row (b)'s own machinery,
//       not a separate row.
//
// Counting notes carried over verbatim from the desktop file:
//   - Rust prod lines = total lines minus brace-matched `#[cfg(test)]`-attributed
//     items. Braces counted per line, best-effort (no string-literal parsing).
//   - mirror_markers is a substring count; comment mentions count. That is a
//     feature for tripwires (forces a look), not a parser bug.
//   - children_per_dir ignores dotfiles, __tests__ dirs and *.test.ts files.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));

// ---------------------------------------------------------------------------
// small fs helpers (verbatim from desktop's ratchet.mjs)
// ---------------------------------------------------------------------------

const PRUNE_DIRS = new Set(['node_modules', '.git', 'dist', 'target', '.moss', '.worktrees']);

function isDir(p) {
  try { return fs.statSync(p).isDirectory(); } catch { return false; }
}
function isFile(p) {
  try { return fs.statSync(p).isFile(); } catch { return false; }
}
function read(p) {
  return fs.readFileSync(p, 'utf8');
}
function rel(root, p) {
  return path.relative(root, p).split(path.sep).join('/');
}

/** Recursively list files under dirAbs (pruning PRUNE_DIRS), returning absolute paths. */
function walkFiles(dirAbs, out = []) {
  let entries;
  try { entries = fs.readdirSync(dirAbs, { withFileTypes: true }); } catch { return out; }
  for (const e of entries) {
    if (e.name.startsWith('.')) continue;
    const p = path.join(dirAbs, e.name);
    if (e.isDirectory()) {
      if (!PRUNE_DIRS.has(e.name)) walkFiles(p, out);
    } else if (e.isFile()) {
      out.push(p);
    }
  }
  return out;
}

/** Recursively list directories under dirAbs (including dirAbs itself). */
function walkDirs(dirAbs, out = []) {
  out.push(dirAbs);
  let entries;
  try { entries = fs.readdirSync(dirAbs, { withFileTypes: true }); } catch { return out; }
  for (const e of entries) {
    if (e.name.startsWith('.')) continue;
    if (e.isDirectory() && !PRUNE_DIRS.has(e.name) && e.name !== '__tests__') {
      walkDirs(path.join(dirAbs, e.name), out);
    }
  }
  return out;
}

function countLines(text) {
  const n = text.split('\n').length;
  return text.endsWith('\n') ? n - 1 : n;
}

function countMatches(text, regex) {
  const m = text.match(regex);
  return m ? m.length : 0;
}

// TS prod-file exclusions, same set the desktop rows share.
function isExcludedTs(fileAbs) {
  const base = path.basename(fileAbs);
  if (base.endsWith('.test.ts') || base.endsWith('.d.ts') || base.endsWith('.generated.ts')) return true;
  if (base === 'bindings.ts') return true;
  if (fileAbs.split(path.sep).includes('__tests__')) return true;
  return false;
}

// Rust sibling test files — `<name>_tests.rs` — hold ONLY test code (see
// desktop file's comment); excluded from row (a) prod-line and row (b)
// child-count the same way.
function isRustTestSibling(fileAbs) {
  return path.basename(fileAbs).endsWith('_tests.rs');
}

// ---------------------------------------------------------------------------
// Rust prod-line counting: total lines minus #[cfg(test)]-attributed items,
// brace-matched (verbatim from desktop's ratchet.mjs).
// ---------------------------------------------------------------------------

const CFG_TEST_RE = /^\s*(?:#\[(?!cfg\(test\))[^\]]*\]\s*)*#\[cfg\(test\)\]/;

/** Inclusive `[firstLine, lastLine]` index ranges of `#[cfg(test)]`-attributed items. */
function rustCfgTestRanges(lines) {
  const ranges = [];
  let i = 0;
  while (i < lines.length) {
    if (!CFG_TEST_RE.test(lines[i])) { i++; continue; }
    let depth = 0;
    let seenBrace = false;
    let j = i;
    for (; j < lines.length; j++) {
      const l = lines[j];
      for (const ch of l) {
        if (ch === '{') { depth++; seenBrace = true; }
        else if (ch === '}') depth--;
      }
      if (seenBrace && depth <= 0) break;
      if (!seenBrace) {
        const code = l.replace(/\/\/.*$/, '').trimEnd();
        if (code.endsWith(';') && !/^\s*#\[/.test(lines[j])) break;
      }
    }
    ranges.push([i, Math.min(j, lines.length - 1)]);
    i = j + 1;
  }
  return ranges;
}

export function countRustProdLines(text) {
  const lines = text.split('\n');
  let excluded = 0;
  for (const [from, to] of rustCfgTestRanges(lines)) excluded += to - from + 1;
  return Math.max(0, countLines(text) - excluded);
}

function countAcross(root, files, regexes, transform) {
  let total = 0;
  const detail = {};
  for (const f of files) {
    let text;
    try { text = read(f); } catch { continue; }
    if (transform) text = transform(text);
    let n = 0;
    if (typeof regexes === 'function') n = regexes(text);
    else for (const re of regexes) n += countMatches(text, re);
    if (n > 0) {
      total += n;
      detail[rel(root, f)] = n;
    }
  }
  return { total, detail };
}

// Rust prod roots: each crates/<name>/src (open/ prefix already dropped —
// this repo IS the open half).
function rustProdRoots(root) {
  const roots = [];
  const cratesDir = path.join(root, 'crates');
  if (isDir(cratesDir)) {
    for (const e of fs.readdirSync(cratesDir, { withFileTypes: true })) {
      if (e.isDirectory() && isDir(path.join(cratesDir, e.name, 'src'))) {
        roots.push(path.join(cratesDir, e.name, 'src'));
      }
    }
  }
  return roots.filter(isDir);
}

// Workspace TS packages (packages/*/src) — rows (a) and (b) scan these so an
// extraction into packages/ can never become the way around a file-size or
// dir-child budget.
function packagesSrcRoots(root) {
  const pkgsDir = path.join(root, 'packages');
  const roots = [];
  if (isDir(pkgsDir)) {
    for (const e of fs.readdirSync(pkgsDir, { withFileTypes: true })) {
      if (e.isDirectory() && isDir(path.join(pkgsDir, e.name, 'src'))) {
        roots.push(path.join(pkgsDir, e.name, 'src'));
      }
    }
  }
  return roots;
}

// ---------------------------------------------------------------------------
// row collectors
// ---------------------------------------------------------------------------

const collectors = {
  // (a) files over 800 prod lines → map {path: prodLines}
  prod_lines_per_file(root) {
    const map = {};
    for (const dir of rustProdRoots(root)) {
      for (const f of walkFiles(dir)) {
        if (!f.endsWith('.rs') || isRustTestSibling(f)) continue;
        const n = countRustProdLines(read(f));
        if (n > 800) map[rel(root, f)] = n;
      }
    }
    for (const dir of packagesSrcRoots(root)) {
      for (const f of walkFiles(dir)) {
        if (!f.endsWith('.ts') || isExcludedTs(f)) continue;
        const n = countLines(read(f));
        if (n > 800) map[rel(root, f)] = n;
      }
    }
    return { kind: 'map', map, threshold: 800, unit: 'prod lines' };
  },

  // (b) direct children per directory → map {dir: count}; new dirs budget <=15
  children_per_dir(root) {
    const map = {};
    const roots = [...rustProdRoots(root), ...packagesSrcRoots(root)].filter(isDir);
    for (const r of roots) {
      for (const dir of walkDirs(r)) {
        let n = 0;
        for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
          if (e.name.startsWith('.')) continue;
          if (e.isDirectory() && (e.name === '__tests__' || PRUNE_DIRS.has(e.name))) continue;
          if (e.isFile() && (e.name.endsWith('.test.ts') || isRustTestSibling(e.name))) continue;
          n++;
        }
        map[rel(root, dir)] = n;
      }
    }
    return { kind: 'map', map, newBudget: 15, unit: 'direct children' };
  },

  // (e) mirror-comment markers across crates/ (packages/ never carried this
  // row on the desktop side either — see file header).
  mirror_markers(root) {
    const files = [];
    const cratesDir = path.join(root, 'crates');
    if (isDir(cratesDir)) {
      for (const f of walkFiles(cratesDir)) {
        if (f.endsWith('.rs')) {
          if (f.split(path.sep).includes('tests')) continue;
          files.push(f);
        } else if (f.endsWith('.ts') && !isExcludedTs(f)) {
          files.push(f);
        }
      }
    }
    return { kind: 'scalar', ...countAcross(root, files, [/keep in sync|must match|mirror of/gi]) };
  },
};

const ROWS = {
  prod_lines_per_file: { letter: 'a', disposition: 'permanent' },
  children_per_dir: { letter: 'b', disposition: 'permanent' },
  mirror_markers: { letter: 'e', disposition: 'permanent' },
};

export function collectCurrent(root) {
  const out = {};
  for (const key of Object.keys(ROWS)) out[key] = collectors[key](root);
  return out;
}

// ---------------------------------------------------------------------------
// baseline I/O
// ---------------------------------------------------------------------------

function baselinePath() {
  return path.join(SCRIPT_DIR, 'ratchet-baseline.open.json');
}

/**
 * Object keys that appear more than once in the same object — ported
 * verbatim (minus its multi-repo framing) from desktop's ratchet.mjs, whose
 * header explains why: a merge can fuse two `accepts[]` entries into one
 * object and still parse as valid JSON, silently discarding one's reason.
 */
export function findDuplicateJsonKeys(text) {
  const dups = [];
  const stack = [];
  let line = 1;
  let i = 0;

  while (i < text.length) {
    const c = text[i];
    if (c === '\n') { line++; i++; continue; }
    if (c === '{') { stack.push(new Map()); i++; continue; }
    if (c === '[') { stack.push(null); i++; continue; }
    if (c === '}' || c === ']') { stack.pop(); i++; continue; }
    if (c !== '"') { i++; continue; }

    const startLine = line;
    const rawStart = i;
    i++;
    while (i < text.length && text[i] !== '"') {
      if (text[i] === '\\') {
        if (text[i + 1] === '\n') line++;
        i += 2;
        continue;
      }
      if (text[i] === '\n') line++;
      i++;
    }
    i++;

    const frame = stack[stack.length - 1];
    if (!(frame instanceof Map)) continue;

    let j = i;
    while (j < text.length && /\s/.test(text[j])) j++;
    if (text[j] !== ':') continue;

    const raw = text.slice(rawStart, i);
    let name;
    try { name = JSON.parse(raw); } catch { name = raw; }

    const firstLine = frame.get(name);
    if (firstLine !== undefined) dups.push({ key: name, line: startLine, firstLine });
    else frame.set(name, startLine);
  }

  return dups;
}

function loadBaseline() {
  const p = baselinePath();
  if (!isFile(p)) {
    console.error(`RED baseline file missing: ${p}`);
    process.exit(1);
  }
  const text = read(p);

  const dups = findDuplicateJsonKeys(text);
  if (dups.length) {
    console.error(`RED ${p}: duplicate key(s) — the file does not say what it appears to say.`);
    for (const d of dups) {
      console.error(`  line ${d.line}: "${d.key}" — already set at line ${d.firstLine}`);
    }
    console.error('  Two records were probably fused by a merge. Read them apart by hand;');
    console.error('  do NOT run accept/tighten first — saving would discard one for good.');
    process.exit(1);
  }

  try {
    return JSON.parse(text);
  } catch (e) {
    console.error(`RED ${p}: not valid JSON — ${e.message}`);
    process.exit(1);
  }
}

function saveBaseline(baseline) {
  fs.writeFileSync(baselinePath(), JSON.stringify(baseline, null, 2) + '\n');
}

// ---------------------------------------------------------------------------
// self-checks
// ---------------------------------------------------------------------------

function runSelfChecks(root, baseline, current, violations) {
  // 1. row table <-> baseline agreement + every row has a disposition
  for (const [key, spec] of Object.entries(ROWS)) {
    const row = baseline.rows?.[key];
    if (!row) { violations.push(`self-check: row '${key}' missing from baseline`); continue; }
    if (!row.disposition) violations.push(`self-check: row '${key}' has no disposition`);
    else if (row.disposition !== spec.disposition) {
      violations.push(`self-check: row '${key}' disposition '${row.disposition}' != expected '${spec.disposition}'`);
    }
    if (row.letter !== spec.letter) {
      violations.push(`self-check: row '${key}' letter '${row.letter}' != expected '${spec.letter}'`);
    }
  }
  for (const key of Object.keys(baseline.rows ?? {})) {
    if (!ROWS[key]) violations.push(`self-check: baseline row '${key}' unknown to ratchet.mjs`);
  }

  // 2. stale entries (path no longer on disk) — fail-on-stale.
  for (const [key, row] of Object.entries(baseline.rows ?? {})) {
    if (!row.armed || typeof row.value !== 'object' || row.value === null) continue;
    for (const p of Object.keys(row.value)) {
      const abs = path.join(root, ...p.split('/'));
      if (!fs.existsSync(abs)) {
        violations.push(`self-check(stale): ${key} baseline entry '${p}' no longer exists on disk — run tighten`);
      }
    }
  }

  // 2b. children_per_dir: NORTH-STAR's "≥30 mandatory split" (ported because
  // it is row (b)'s own machinery, not a separate row).
  const cpd = baseline.rows?.children_per_dir;
  if (cpd?.armed && current.children_per_dir) {
    const SPLIT_LINE = 30;
    const disp = cpd.oversized ?? {};
    for (const [dir, n] of Object.entries(current.children_per_dir.map)) {
      if (n < SPLIT_LINE) continue;
      const reason = disp[dir];
      if (typeof reason !== 'string' || !reason.trim()) {
        violations.push(
          `(b) children_per_dir: '${dir}' has ${n} direct children >= ${SPLIT_LINE} (NORTH-STAR: mandatory split) — split it, or record why not in the baseline's children_per_dir.oversized{}`,
        );
      }
    }
    for (const dir of Object.keys(disp)) {
      const n = current.children_per_dir.map[dir];
      if (n === undefined || n < SPLIT_LINE) {
        violations.push(
          `self-check(stale): children_per_dir.oversized entry '${dir}' is ${n === undefined ? 'gone' : `down to ${n}`} — drop it from oversized{}`,
        );
      }
    }
  }
}

// ---------------------------------------------------------------------------
// check / tighten / accept
// ---------------------------------------------------------------------------

function checkRow(key, row, cur, violations) {
  if (cur.kind === 'scalar') {
    if (typeof row.value !== 'number') {
      violations.push(`(${row.letter}) ${key}: baseline value is not a number`);
      return 'bad baseline';
    }
    if (cur.total > row.value) {
      violations.push(`(${row.letter}) ${key}: ${cur.total} > baseline ${row.value}`);
      const entries = Object.entries(cur.detail);
      for (const [where, n] of entries.slice(0, 20)) violations.push(`    ${where}: ${n}`);
      if (entries.length > 20) violations.push(`    ... and ${entries.length - 20} more locations`);
      return `${cur.total} > ${row.value}`;
    }
    return `${cur.total} <= ${row.value}`;
  }

  const base = row.value ?? {};
  let bad = 0;
  for (const [p, n] of Object.entries(cur.map)) {
    const b = base[p];
    let msg = null;
    if (b === undefined) {
      if (cur.threshold !== undefined) {
        msg = `(${row.letter}) ${key}: NEW '${p}' crossed ${cur.threshold} (${n} ${cur.unit}, not in baseline) — split it, or 'accept' with a reason`;
      } else if (cur.newBudget !== undefined && n > cur.newBudget) {
        msg = `(${row.letter}) ${key}: NEW dir '${p}' has ${n} ${cur.unit} > budget ${cur.newBudget}`;
      }
    } else if (n > b) {
      msg = `(${row.letter}) ${key}: '${p}' grew to ${n} ${cur.unit} (baseline ${b})`;
    }
    if (msg === null) continue;
    violations.push(msg);
    bad++;
  }
  return `${Object.keys(base).length} entries baselined, ${bad} violation${bad === 1 ? '' : 's'}`;
}

function cmdCheck(root) {
  const baseline = loadBaseline();
  const violations = [];
  const current = collectCurrent(root);
  runSelfChecks(root, baseline, current, violations);

  console.log(`ratchet check — this tree @ ${root}`);
  for (const [key, spec] of Object.entries(ROWS)) {
    const row = baseline.rows?.[key];
    if (!row) continue; // already a self-check violation
    if (!row.armed) {
      console.log(`  --  (${spec.letter}) ${key.padEnd(24)} not armed: ${row.reason ?? '(no reason)'}`);
      continue;
    }
    const before = violations.length;
    const summary = checkRow(key, row, current[key], violations);
    const mark = violations.length > before ? 'RED' : 'OK ';
    console.log(`  ${mark} (${spec.letter}) ${key.padEnd(24)} ${summary}`);
  }

  if (violations.length) {
    console.log(`\nRED — ${violations.length} violation line${violations.length === 1 ? '' : 's'}:`);
    for (const v of violations) console.log(`  ${v}`);
    process.exit(1);
  }
  console.log('\nGREEN — all armed rows within baseline; self-checks passed.');
}

function cmdTighten(root) {
  const baseline = loadBaseline();
  const current = collectCurrent(root);
  const lowered = [];
  const refused = [];

  for (const [key, row] of Object.entries(baseline.rows ?? {})) {
    if (!row.armed || !collectors[key]) continue;
    const cur = current[key];
    if (cur.kind === 'scalar') {
      if (cur.total < row.value) {
        lowered.push(`${key}: ${row.value} -> ${cur.total}`);
        row.value = cur.total;
      } else if (cur.total > row.value) {
        refused.push(`${key}: current ${cur.total} > baseline ${row.value} — tighten NEVER raises; fix the code or use 'accept ${key} <reason>'`);
      }
    } else {
      const base = row.value ?? {};
      for (const [p, b] of Object.entries(base)) {
        const n = cur.map[p];
        const abs = path.join(root, ...p.split('/'));
        if (!fs.existsSync(abs)) {
          lowered.push(`${key}: dropped stale '${p}' (no longer on disk)`);
          delete base[p];
        } else if (n === undefined) {
          if (cur.threshold !== undefined) {
            lowered.push(`${key}: dropped '${p}' (now <= ${cur.threshold})`);
            delete base[p];
          }
        } else if (n < b) {
          lowered.push(`${key}: '${p}' ${b} -> ${n}`);
          base[p] = n;
        } else if (n > b) {
          refused.push(`${key}: '${p}' current ${n} > baseline ${b} — tighten NEVER raises; fix the code or use 'accept ${key} <reason>'`);
        }
      }
    }
  }

  if (lowered.length) {
    saveBaseline(baseline);
    console.log(`tightened baseline (${lowered.length} change${lowered.length === 1 ? '' : 's'}):`);
    for (const l of lowered) console.log(`  ${l}`);
  } else {
    console.log('baseline already tight — no changes written.');
  }
  if (refused.length) {
    console.log('\nREFUSED to raise (shrink-only is enforced by this tool):');
    for (const r of refused) console.log(`  ${r}`);
    process.exit(1);
  }
}

function cmdAccept(root, rowKey, reason) {
  if (!rowKey || !reason) {
    console.error('usage: ratchet.mjs accept <row-key> <reason> [--root <path>]');
    process.exit(1);
  }
  const baseline = loadBaseline();
  const row = baseline.rows?.[rowKey];
  if (!row) { console.error(`unknown row '${rowKey}'`); process.exit(1); }
  if (!row.armed) { console.error(`row '${rowKey}' is not armed — nothing to accept`); process.exit(1); }
  if (!collectors[rowKey]) { console.error(`row '${rowKey}' has no collector`); process.exit(1); }

  const cur = collectors[rowKey](root);
  const date = new Date().toISOString().slice(0, 10);
  let entry;

  if (cur.kind === 'scalar') {
    if (cur.total <= row.value) {
      console.error(`nothing to accept: current ${cur.total} <= baseline ${row.value}`);
      process.exit(1);
    }
    entry = { row: rowKey, from: row.value, to: cur.total, reason, date };
    row.value = cur.total;
  } else {
    const base = row.value ?? {};
    row.value = base;
    const from = {};
    const to = {};
    for (const [p, n] of Object.entries(cur.map)) {
      const b = base[p];
      const overNew =
        b === undefined &&
        ((cur.threshold !== undefined) || (cur.newBudget !== undefined && n > cur.newBudget));
      if (b !== undefined && n > b) {
        from[p] = b; to[p] = n; base[p] = n;
      } else if (overNew) {
        from[p] = null; to[p] = n; base[p] = n;
      }
    }
    if (Object.keys(to).length === 0) {
      console.error(`nothing to accept: no entry of '${rowKey}' exceeds its baseline`);
      process.exit(1);
    }
    entry = { row: rowKey, from, to, reason, date };
  }

  baseline.accepts = baseline.accepts ?? [];
  baseline.accepts.push(entry);
  saveBaseline(baseline);
  console.log(`accepted raise for '${rowKey}' — logged in accepts[]:`);
  console.log(`  ${JSON.stringify(entry)}`);
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

function main(argv) {
  const args = argv.slice(2);
  const positional = [];
  let root = process.cwd();
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--root') root = path.resolve(args[++i]);
    else positional.push(args[i]);
  }
  const cmd = positional[0];
  if (cmd === 'check') cmdCheck(root);
  else if (cmd === 'tighten') cmdTighten(root);
  else if (cmd === 'accept') cmdAccept(root, positional[1], positional.slice(2).join(' '));
  else {
    console.error('usage: ratchet.mjs <check|tighten|accept> [args] [--root <path>]');
    process.exit(1);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv);
}
