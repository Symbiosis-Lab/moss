#!/usr/bin/env node
// ratchet.mjs — shrink-only architecture ratchet for this repo. Three rows:
//
//   (a) prod_lines_per_file — files over 800 prod lines (Rust counted
//                             #[cfg(test)]-aware) need a banded ceiling; any
//                             file over 1000 additionally needs a written
//                             disposition in `oversized{}` (same shape as
//                             row (b)'s, below)
//   (b) children_per_dir    — direct children per directory (new dirs get a
//                             <=15 budget; any dir >=30 needs a written
//                             disposition in `oversized{}`)
//   (e) mirror_markers      — "keep in sync" / "must match" / "mirror of"
//                             comment markers
//
// Roots: this repo's own `crates/*/src` and `packages/*/src` (rows a/b);
// mirror_markers never scanned packages.
//
// Commands:
//   node scripts/ratchet.mjs check           exit 0 green / 1 red
//   node scripts/ratchet.mjs tighten [--path <file>]...   lower baseline; NEVER raises
//   node scripts/ratchet.mjs accept <row> <reason> --path <file> [--path <file>]...
//                                             the ONLY way a baseline grows;
//                                             prints the Ratchet-Accept trailer(s)
//                                             the commit message must carry
//   node scripts/ratchet.mjs verify-accepts <commit-msg-file>
//                                             commit-msg hook body: every
//                                             row/key that rose between HEAD's
//                                             and the staged baseline must have
//                                             a matching trailer in the message
//   node scripts/ratchet.mjs docs [--check]  regenerate (or, with --check,
//                                             only verify) ARCHITECTURE.md's
//                                             Known-debt block from row (a)'s
//                                             `oversized{}` map; `check` also
//                                             runs the --check form itself, so
//                                             an unregenerated block is red
//
// `--path` (repeatable) is required by `accept` for a per-path row (a, b): it
// names exactly which entries the reason covers, so a reason written for one
// file's growth cannot silently accept another file's growth in the same row.
// A named path that is not over its baseline is an error, and nothing is
// written. Paths are spelled as in the baseline (root-relative,
// `/`-separated); `./` and absolute spellings under --root are folded to it.
//
// Row (a) values are CEILINGS, not raw counts: always a multiple of 100.
// `accept`/`tighten` compute it as `ceil(lines / 100) * 100`.
//
// A banded ceiling only ever says "this file may not grow past here" — it
// never says anyone looked at a file that is already over 1000 lines and
// judged the size acceptable. That is what `prod_lines_per_file.oversized{}`
// (self-check 2c, below) is for: a written, one-line reason per file over
// 1000 prod lines, the same `{path: reason}` shape row (b) already uses for
// an oversized directory. A file that crosses 1000 with no entry there is
// red; an entry whose file has since dropped to 1000 lines or under is
// reported stale (self-check 2c also flags this) until it's removed. This is
// a disposition tier, not a second ceiling — it never bands and it never
// raises anything by itself; only a hand-edit of `oversized{}` (reviewed like
// any other baseline change) adds or removes an entry. ARCHITECTURE.md's
// "Known debt" list is generated from this map (`docs` command, below) so it
// reports exactly what the baseline says, never a hand-copied snapshot of it.
//
// Baseline: scripts/ratchet-baseline.open.json, next to this script. The
// reason for a raise lives in the commit message's trailer (see
// `verify-accepts` above).
//
// A reason `accept`/`verify-accepts` will actually take must clear
// `reasonIsWeak`'s floor: >=15 characters, and not just the row name, the
// key being raised, or one of wip/accept/fix/todo/n/a. A scalar row (no
// per-path map, e.g. mirror_markers) has no path to key its trailer on, so
// its trailer uses the literal key `(total)` (`SCALAR_TRAILER_KEY`).
//
// `verify-accepts` does nothing on an ordinary `git merge` commit
// (`isMidMerge`) — a peer's raise was already justified on their own
// branch, and the auto-generated "Merge branch …" message is never the
// place to re-justify it. This machinery (the constants and every function
// from `SCALAR_TRAILER_KEY` through `verifyAccepts`, plus `isMidMerge` and
// `baselineFileNames`) is shared with the other repos' copies of this script, so an
// agent moving between repos meets one tool.
//
// Self-checks (run at the start of every `check`; any failure = red):
//   1. every baseline row has a disposition, matches the row table below;
//   2. stale baseline entries (path no longer on disk) go red — fix with `tighten`;
//   2b. children_per_dir: any dir at/over 30 direct children needs a written
//       reason in the baseline's children_per_dir.oversized{} (NORTH-STAR's
//       "mandatory split" line) — it is row (b)'s own machinery, not a
//       separate row.
//   2c. prod_lines_per_file: any file over 1000 prod lines needs a written
//       reason in the baseline's prod_lines_per_file.oversized{}, the same
//       shape as check 2b. An oversized{} entry whose file has dropped to
//       1000 lines or under, or no longer exists, is reported stale — fix by
//       deleting the entry.
//   3. row (a) stale-high: a baseline ceiling >=100 above a file's actual
//      current lines means banding headroom has drifted into unreviewed
//      slack — fix with `tighten`. A gap under 100 is ordinary banding
//      headroom (the ceiling rounds UP to the next hundred), not staleness.
//   4. ARCHITECTURE.md's generated Known-debt block (see `docs`, above) must
//      match what prod_lines_per_file.oversized{} says right now — fix with
//      `node scripts/ratchet.mjs docs`. Skipped when there is no
//      ARCHITECTURE.md next to this script's repo root (e.g. the test
//      suite's throwaway trees) — nothing to check against.
//
// Counting notes:
//   - Rust prod lines = total lines minus brace-matched `#[cfg(test)]`-attributed
//     items. Braces counted per line, best-effort (no string-literal parsing).
//   - mirror_markers is a substring count; comment mentions count. That is a
//     feature for tripwires (forces a look), not a parser bug.
//   - children_per_dir ignores dotfiles, __tests__ dirs and *.test.ts files.
//
// Row (b)'s append-only-directory exemption is NOT implemented here: neither
// scanned root is an append-only directory.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

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

/** Production line count for one row-(a)-eligible file, `.rs` or `.ts` alike. */
function countProdLinesFor(fileAbs) {
  const text = read(fileAbs);
  return fileAbs.endsWith('.rs') ? countRustProdLines(text) : countLines(text);
}

/** `ceil(n / 100) * 100` — row (a)'s banding (ratchet-spec.md §1). */
function band100(n) {
  return Math.ceil(n / 100) * 100;
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
        const n = countProdLinesFor(f);
        if (n > 800) map[rel(root, f)] = n;
      }
    }
    for (const dir of packagesSrcRoots(root)) {
      for (const f of walkFiles(dir)) {
        if (!f.endsWith('.ts') || isExcludedTs(f)) continue;
        const n = countProdLinesFor(f);
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
 * verbatim (minus its multi-repo framing) from desktop's ratchet.mjs. Two
 * branches that independently `accept` the SAME newly-over-baseline file
 * each append one new `"path": ceiling` line to a row's `value` map at the
 * same insertion point (right after the same prior last entry); a merge can
 * keep both additions without git ever flagging a textual conflict, leaving
 * that key written twice. `JSON.parse` silently keeps only the last one, so
 * one branch's raise — and the trailer that justified it — would disappear
 * without this check ever looking at it.
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
// ARCHITECTURE.md's generated "Known debt" block — the single reader-facing
// view of prod_lines_per_file.oversized{}, so the list a stranger reads
// there can never drift from what the baseline actually disposes (see
// self-check 4 and the `docs` command). Next to this script's own repo root,
// same way baselinePath() finds the baseline regardless of --root.
// ---------------------------------------------------------------------------

function architectureDocPath() {
  return path.join(SCRIPT_DIR, '..', 'ARCHITECTURE.md');
}

const KNOWN_DEBT_START = '<!-- ratchet:known-debt:start -->';
const KNOWN_DEBT_END = '<!-- ratchet:known-debt:end -->';

/**
 * `{path, lines, reason}` for every prod_lines_per_file.oversized{} entry
 * whose file is still actually over 1000 lines right now — sorted largest
 * first, matching the old hand-written list's order. An entry self-check 2c
 * would flag as stale (file shrank, or is gone) is left out here too: the
 * doc block and the self-check must agree on what counts as current debt.
 */
function knownDebtRows(baseline, current) {
  const disp = baseline.rows?.prod_lines_per_file?.oversized ?? {};
  const map = current.prod_lines_per_file?.map ?? {};
  const rows = [];
  for (const [p, reason] of Object.entries(disp)) {
    const n = map[p];
    if (typeof n !== 'number' || n <= 1000) continue;
    if (typeof reason !== 'string' || !reason.trim()) continue;
    rows.push({ path: p, lines: n, reason: reason.trim() });
  }
  rows.sort((a, b) => b.lines - a.lines);
  return rows;
}

function renderKnownDebtTable(rows) {
  const lines = ['| File | Prod lines | Disposition |', '|---|---:|---|'];
  for (const r of rows) lines.push(`| \`${r.path}\` | ${r.lines} | ${r.reason} |`);
  return lines.join('\n');
}

/**
 * ARCHITECTURE.md's text with the block between KNOWN_DEBT_START/END
 * replaced by a freshly rendered table for `current`'s line counts — or
 * `null` if `doc` doesn't carry both markers (nothing to splice into).
 * Used both to write the file (`docs`) and, unmodified, to detect staleness
 * (self-check 4, `docs --check`): the same render must equal what's on disk.
 */
function renderArchitectureDoc(doc, baseline, current) {
  const startIdx = doc.indexOf(KNOWN_DEBT_START);
  const endIdx = doc.indexOf(KNOWN_DEBT_END);
  if (startIdx === -1 || endIdx === -1 || endIdx < startIdx) return null;
  const table = renderKnownDebtTable(knownDebtRows(baseline, current));
  const before = doc.slice(0, startIdx + KNOWN_DEBT_START.length);
  const after = doc.slice(endIdx);
  return `${before}\n${table}\n${after}`;
}

function cmdDocs(root, { check } = {}) {
  const docPath = architectureDocPath();
  if (!isFile(docPath)) {
    console.log(`docs: no ARCHITECTURE.md at ${docPath} — nothing to generate`);
    return;
  }
  const baseline = loadBaseline();
  const current = collectCurrent(root);
  const doc = read(docPath);
  const rendered = renderArchitectureDoc(doc, baseline, current);
  if (rendered === null) {
    console.error(`docs: ARCHITECTURE.md is missing the ${KNOWN_DEBT_START} / ${KNOWN_DEBT_END} markers`);
    process.exit(1);
  }
  if (check) {
    if (rendered !== doc) {
      console.error("RED ARCHITECTURE.md's Known-debt block is stale — run `node scripts/ratchet.mjs docs` to regenerate it.");
      process.exit(1);
    }
    console.log("OK ARCHITECTURE.md's Known-debt block matches the baseline.");
    return;
  }
  if (rendered === doc) {
    console.log("ARCHITECTURE.md's Known-debt block is already up to date.");
    return;
  }
  fs.writeFileSync(docPath, rendered);
  console.log('regenerated ARCHITECTURE.md\'s Known-debt block.');
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

  // 2c. prod_lines_per_file: a file over 1000 prod lines needs a written
  // disposition in the baseline's prod_lines_per_file.oversized{} — the same
  // `{path: reason}` shape as row (b)'s oversized{} (2b, above), reused
  // rather than reinvented. A banded ceiling in `value` only ever says "may
  // not grow past here"; it is never a signoff that the file is already over
  // 1000 lines. Weak-reason validation reuses `reasonIsWeak`, the same floor
  // `accept` holds every ceiling raise to (>=15 chars, not just the row name
  // or the path being raised) — one validator for what counts as a reason,
  // not a second one invented for this tier.
  const pl = baseline.rows?.prod_lines_per_file;
  if (pl?.armed && current.prod_lines_per_file) {
    const OVERSIZED_LINE = 1000;
    const disp = pl.oversized ?? {};
    for (const [p, n] of Object.entries(current.prod_lines_per_file.map)) {
      if (n <= OVERSIZED_LINE) continue;
      const reason = disp[p];
      if (typeof reason !== 'string' || reasonIsWeak(reason, 'prod_lines_per_file', p)) {
        violations.push(
          `(a) prod_lines_per_file: '${p}' has ${n} prod lines > ${OVERSIZED_LINE} — split it, or record why not (and what would retire it) in the baseline's prod_lines_per_file.oversized{}`,
        );
      }
    }
    for (const p of Object.keys(disp)) {
      const n = current.prod_lines_per_file.map[p];
      if (n === undefined) {
        const abs = path.join(root, ...p.split('/'));
        violations.push(
          `self-check(stale): prod_lines_per_file.oversized entry '${p}' is ${fs.existsSync(abs) ? 'now <= 800 prod lines (below row (a)\'s own floor)' : 'gone'} — drop it from oversized{}`,
        );
      } else if (n <= OVERSIZED_LINE) {
        violations.push(
          `self-check(stale): prod_lines_per_file.oversized entry '${p}' is down to ${n} prod lines (<= ${OVERSIZED_LINE}) — drop it from oversized{}`,
        );
      }
    }
  }

  // 3. row (a) stale-high: a ceiling far above the file's real current size
  // is unreviewed slack, not headroom. A gap under 100 is ordinary banding
  // (the ceiling rounds UP to the next hundred); 100 or more means the file
  // shrank since the ceiling was last set and `tighten` is overdue.
  if (pl?.armed) {
    for (const [p, ceiling] of Object.entries(pl.value ?? {})) {
      if (typeof ceiling !== 'number') continue; // self-check 1/bad-baseline territory
      const abs = path.join(root, ...p.split('/'));
      if (!fs.existsSync(abs)) continue; // self-check 2 already flags this
      const lines = countProdLinesFor(abs);
      if (ceiling - lines >= 100) {
        violations.push(
          `self-check(stale-high): prod_lines_per_file '${p}' ceiling ${ceiling} is >=100 above its actual ${lines} lines — run tighten`,
        );
      }
    }
  }

  // 4. ARCHITECTURE.md's generated Known-debt block must match what
  // prod_lines_per_file.oversized{} says right now (see `docs`, below).
  // Skipped when there is no ARCHITECTURE.md next to this script's repo root
  // — the test suite's throwaway trees carry no such file, and there is
  // nothing to check staleness against.
  const docPath = architectureDocPath();
  if (isFile(docPath)) {
    const rendered = renderArchitectureDoc(read(docPath), baseline, current);
    if (rendered === null) {
      violations.push(
        `self-check: ARCHITECTURE.md is missing the ${KNOWN_DEBT_START} / ${KNOWN_DEBT_END} markers — the Known-debt block can't be generated into it`,
      );
    } else if (rendered !== read(docPath)) {
      violations.push(
        "ARCHITECTURE.md's Known-debt block is stale — run `node scripts/ratchet.mjs docs` to regenerate it from the baseline",
      );
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
        msg = `(${row.letter}) ${key}: NEW '${p}' crossed ${cur.threshold} (${n} ${cur.unit}, not in baseline) — split it, or run: ratchet.mjs accept ${key} "<reason>" --path ${p}`;
      } else if (cur.newBudget !== undefined && n > cur.newBudget) {
        msg = `(${row.letter}) ${key}: NEW dir '${p}' has ${n} ${cur.unit} > budget ${cur.newBudget} — run: ratchet.mjs accept ${key} "<reason>" --path ${p}`;
      }
    } else if (n > b) {
      msg = `(${row.letter}) ${key}: '${p}' grew to ${n} ${cur.unit} (baseline ${b}) — run: ratchet.mjs accept ${key} "<reason>" --path ${p}`;
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

function cmdTighten(root, only = []) {
  const baseline = loadBaseline();
  const current = collectCurrent(root);
  const lowered = [];
  const refused = [];
  const wanted = only.length ? new Set(only.map((p) => normalizePathArg(root, p))) : null;

  // Checked before anything is touched, so a typo never half-applies.
  for (const p of wanted ?? []) {
    const baselined = Object.values(baseline.rows ?? {}).some(
      (r) => r.armed && typeof r.value === 'object' && r.value !== null && p in r.value,
    );
    if (!baselined) {
      console.error(`'${p}' is in no baseline row — nothing to tighten for it`);
      process.exit(1);
    }
  }

  for (const [key, row] of Object.entries(baseline.rows ?? {})) {
    if (!row.armed || !collectors[key]) continue;
    const cur = current[key];
    if (cur.kind === 'scalar') {
      if (wanted) continue; // a scalar row has no path to name
      if (cur.total < row.value) {
        lowered.push(`${key}: ${row.value} -> ${cur.total}`);
        row.value = cur.total;
      } else if (cur.total > row.value) {
        refused.push(`${key}: current ${cur.total} > baseline ${row.value} — tighten NEVER raises; fix the code or use 'accept ${key} <reason>'`);
      }
    } else if (key === 'prod_lines_per_file') {
      // Row (a) is banded: ceil(lines / 100) * 100, never the raw count
      // (ratchet-spec.md §1). A file at/under 800 drops out of the row
      // entirely — it's no longer this row's concern, not merely "tight".
      const base = row.value ?? {};
      for (const [p, b] of Object.entries(base)) {
        if (wanted && !wanted.has(p)) continue;
        const n = cur.map[p];
        const abs = path.join(root, ...p.split('/'));
        if (!fs.existsSync(abs)) {
          lowered.push(`${key}: dropped stale '${p}' (no longer on disk)`);
          delete base[p];
          continue;
        }
        if (n === undefined) {
          lowered.push(`${key}: dropped '${p}' (now <= 800, at/under the floor)`);
          delete base[p];
          continue;
        }
        const c = band100(n);
        if (c < b) {
          lowered.push(`${key}: '${p}' ${b} -> ${c}`);
          base[p] = c;
        } else if (n > b) {
          refused.push(`${key}: '${p}' current ${n} > baseline ${b} — tighten NEVER raises; fix the code or use 'accept ${key} <reason>'`);
        }
        // n <= b and c >= b: already tight to its band — nothing to do.
      }
    } else {
      const base = row.value ?? {};
      for (const [p, b] of Object.entries(base)) {
        if (wanted && !wanted.has(p)) continue;
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

/**
 * What accepting `p` would record for a per-path row: `{from, to}` if its
 * current count is over its baseline (or it is a new entry over the row's new
 * threshold/budget), else null.
 */
function raiseOf(cur, base, p) {
  const n = cur.map[p];
  const b = base[p];
  if (n === undefined) return null;
  if (b !== undefined) return n > b ? { from: b, to: n } : null;
  const overNew = cur.threshold !== undefined || (cur.newBudget !== undefined && n > cur.newBudget);
  return overNew ? { from: null, to: n } : null;
}

/**
 * Print the trailer line(s) the committer must include, exactly:
 * `Ratchet-Accept: <row> <key> — <reason>` — one per raised key
 * (ratchet-spec.md §3). No baseline write happens here; the reason lives in
 * the commit message from now on, not in the JSON.
 */
function printTrailers(rowKey, keys, reason) {
  console.log('accepted — include this in the commit message:');
  for (const key of keys) console.log(`Ratchet-Accept: ${rowKey} ${key} — ${reason}`);
}

function cmdAccept(root, rowKey, reason, only = []) {
  if (!rowKey || !reason) {
    console.error('usage: ratchet.mjs accept <row-key> <reason> --path <file> [--path <file>]... [--root <path>]');
    process.exit(1);
  }
  const baseline = loadBaseline();
  if (reasonIsWeak(reason, rowKey)) {
    console.error(
      `reason too weak: needs >= ${WEAK_REASON_MIN_LENGTH} characters, and can't be just the row name ` +
      `or one of ${[...WEAK_REASON_WORDS].join(', ')} — say what changed and why.`,
    );
    process.exit(1);
  }
  const row = baseline.rows?.[rowKey];
  if (!row) { console.error(`unknown row '${rowKey}'`); process.exit(1); }
  if (!row.armed) { console.error(`row '${rowKey}' is not armed — nothing to accept`); process.exit(1); }
  if (!collectors[rowKey]) { console.error(`row '${rowKey}' has no collector`); process.exit(1); }

  const cur = collectors[rowKey](root);

  if (cur.kind === 'scalar') {
    if (only.length) {
      console.error(`row '${rowKey}' is one number, not per-path — --path does not apply to it`);
      process.exit(1);
    }
    if (cur.total <= row.value) {
      console.error(`nothing to accept: current ${cur.total} <= baseline ${row.value}`);
      process.exit(1);
    }
    row.value = cur.total;
    saveBaseline(baseline);
    printTrailers(rowKey, [SCALAR_TRAILER_KEY], reason);
    return;
  }

  // Per-path rows (a, b) take --path as an argument, always — a pathless
  // accept that sweeps every violation in the row is removed (ratchet-spec.md
  // §1): the reason it prints would then cover growth it never looked at.
  if (!only.length) {
    console.error(`row '${rowKey}' takes one or more --path <file> — a pathless accept is not allowed`);
    process.exit(1);
  }

  const base = row.value ?? {};
  row.value = base;
  const named = only.map((p) => normalizePathArg(root, p));

  // Weak-reason check runs first, over every named path, before anything is
  // checked against the baseline — matches the reference's order: a bad
  // reason is worth reporting before a caller even learns whether the path
  // itself checks out.
  for (const p of named) {
    if (reasonIsWeak(reason, rowKey, p)) {
      console.error(`reason too weak for '${p}': it can't just restate the path being raised — say what changed and why.`);
      process.exit(1);
    }
  }
  // Validated before anything is raised: one named path that is not over its
  // baseline fails the whole call, so a typo cannot quietly accept the rest.
  for (const p of named) {
    if (!raiseOf(cur, base, p)) {
      const n = cur.map[p];
      console.error(
        `'${p}' is not over its '${rowKey}' baseline (current ${n ?? 'absent'}, baseline ${base[p] ?? 'none'}) — nothing to accept for it`,
      );
      process.exit(1);
    }
  }
  for (const p of named) {
    const n = cur.map[p];
    base[p] = rowKey === 'prod_lines_per_file' ? band100(n) : n;
  }

  saveBaseline(baseline);
  printTrailers(rowKey, named, reason);
}

// ---------------------------------------------------------------------------
// verify-accepts — the commit-msg hook's body (ratchet-spec.md §3). The pure
// core below (SCALAR_TRAILER_KEY through verifyAccepts, plus isMidMerge and
// baselineFileNames) is copied verbatim from the private repo's own copy of
// this script, so the two repos' agents meet one tool regardless of which
// repo they're in (2026-09-21 joint review). Only the git/rel/path/SCRIPT_DIR
// primitives it calls are this repo's own.
// ---------------------------------------------------------------------------

function git(root, args) {
  const r = spawnSync('git', args, { cwd: root, encoding: 'utf8' });
  if (r.status !== 0 || r.error) return null;
  return r.stdout;
}

/** The commit-message trailer's placeholder key for a scalar row, whose
 *  baseline is a single number rather than a path-keyed map — there is no
 *  finer-grained "key" to name, so the row's own name stands in for it. Used
 *  identically by `cmdAccept` (prints it) and `diffRaisedEntries`/verify-
 *  accepts (expects it), via this one shared constant. */
const SCALAR_TRAILER_KEY = '(total)';

// Reasons agents reach for under pressure that say nothing at all. Short and
// closed on purpose (2026-09-21 joint review) — this is a floor, not a
// score: a human reviewer reads the actual sentence, and this only blocks
// the handful of reasons that give them literally nothing to read.
const WEAK_REASON_WORDS = new Set(['wip', 'accept', 'fix', 'todo', 'n/a']);
const WEAK_REASON_MIN_LENGTH = 15;

/** A reason too weak to justify a raise: under WEAK_REASON_MIN_LENGTH
 *  characters, or exactly the row name / the key being raised (ignoring case
 *  and surrounding whitespace — restating what already grew is not a
 *  reason), or one of WEAK_REASON_WORDS. `key` is optional (a scalar row's
 *  check via `cmdAccept` runs before any per-path key is known). */
export function reasonIsWeak(reason, rowKey, key) {
  const r = (reason ?? '').trim().toLowerCase();
  if (r.length < WEAK_REASON_MIN_LENGTH) return true;
  if (r === String(rowKey).toLowerCase()) return true;
  if (key !== undefined && r === String(key).toLowerCase()) return true;
  if (WEAK_REASON_WORDS.has(r)) return true;
  return false;
}

function normalizePathArg(root, p) {
  const abs = path.isAbsolute(p) ? p : path.resolve(root, p);
  return rel(root, abs);
}

/** A row's baseline `value` as a flat {key: number} map — already that shape
 *  for a map row; a scalar row's single number is wrapped under
 *  SCALAR_TRAILER_KEY so both shapes can be diffed by the same code. */
function normalizeRowValue(value) {
  if (typeof value === 'number') return { [SCALAR_TRAILER_KEY]: value };
  if (value && typeof value === 'object') return value;
  return {};
}

/** Every {row, key, from, to} whose STAGED value is greater than HEAD's, or
 *  that is new in a shrink-only row (from: null) — i.e. every raise the
 *  commit-msg hook must see a trailer for. A decrease or a removed key is
 *  never in this list: "lowered or removed values need no trailer" (spec). */
export function diffRaisedEntries(headJson, stagedJson) {
  const raised = [];
  for (const [rowKey, sRow] of Object.entries(stagedJson.rows ?? {})) {
    const hRow = headJson.rows?.[rowKey];
    const rowIsNew = hRow === undefined;
    const isScalarRow = typeof sRow?.value === 'number';
    const sEntries = normalizeRowValue(sRow?.value);
    const hEntries = normalizeRowValue(hRow?.value);
    for (const [key, n] of Object.entries(sEntries)) {
      if (typeof n !== 'number') continue;
      const h = hEntries[key];
      const isRaise = typeof h !== 'number' || n > h;
      if (!isRaise) continue;
      // A brand-new SCALAR row (the row didn't exist in HEAD at all) that
      // starts at exactly 0 is not a raise — there is nothing yet to
      // justify, since 0 is a tripwire's natural starting line. A new row
      // starting above 0 (grandfathering existing debt) still is, and so is
      // a new KEY inside an already-existing shrink-only map row (rowIsNew
      // is false there — only the entry is new, not the row).
      if (rowIsNew && isScalarRow && n === 0) continue;
      raised.push({ row: rowKey, key, from: typeof h === 'number' ? h : null, to: n });
    }
  }
  return raised;
}

// `— ` is a real em dash (U+2014), matching the trailer format printed by
// cmdAccept and documented in the file header — not a hyphen or `--`.
const TRAILER_LINE_RE = /^Ratchet-Accept:\s*(.+)$/;

/** Every `Ratchet-Accept: <row> <key> — <reason>` line in a commit message,
 *  as `{row, key, reason}` (reason may be ''  — an empty reason is a real,
 *  rejectable finding, not a parse failure). Lines that don't match the
 *  trailer shape at all are silently not trailers. */
export function parseTrailers(message) {
  const out = [];
  for (const rawLine of message.split('\n')) {
    const m = TRAILER_LINE_RE.exec(rawLine.trim());
    if (!m) continue;
    const rest = m[1];
    const dash = rest.indexOf('—');
    if (dash === -1) continue;
    const head = rest.slice(0, dash).trim();
    const reason = rest.slice(dash + 1).trim();
    const sp = head.indexOf(' ');
    if (sp === -1) continue; // needs both <row> and <key>
    out.push({ row: head.slice(0, sp), key: head.slice(sp + 1).trim(), reason });
  }
  return out;
}

/** `problems`: human-readable strings, one per raised key with no covering
 *  trailer — empty means the commit message covers every raise. */
export function verifyAccepts(headJson, stagedJson, commitMessage) {
  const raised = diffRaisedEntries(headJson, stagedJson);
  if (raised.length === 0) return [];
  const trailers = parseTrailers(commitMessage);
  const problems = [];
  for (const r of raised) {
    const hit = trailers.find((t) => t.row === r.row && t.key === r.key && !reasonIsWeak(t.reason, r.row, r.key));
    if (!hit) {
      const weakOne = trailers.find((t) => t.row === r.row && t.key === r.key);
      const why = weakOne
        ? ` (found one, but the reason is too weak — needs >= ${WEAK_REASON_MIN_LENGTH} characters and can't just restate the row/path)`
        : '';
      problems.push(`(${r.row}) '${r.key}' rose${r.from === null ? ' (new entry)' : ` from ${r.from} to ${r.to}`} with no 'Ratchet-Accept: ${r.row} ${r.key} — <reason>' trailer${why}`);
    }
  }
  return problems;
}

/** True while `git merge` is in the middle of creating its automatic merge
 *  commit (MERGE_HEAD exists in the git dir for exactly that commit's
 *  duration) — a peer's raise was already justified on their own branch, and
 *  an ordinary merge's auto-generated "Merge branch …" message is never the
 *  place to re-justify it; commit-msg fires on that commit exactly as it
 *  does on any other, so without this the hook blocks every ordinary merge
 *  that brings in someone else's already-accepted raise. Resolved via
 *  `git rev-parse --git-dir` rather than a hardcoded `<root>/.git` — a
 *  worktree's git dir lives elsewhere (`.git/worktrees/<name>`), and a bare
 *  `.git` guess would silently never find MERGE_HEAD there at all. */
function isMidMerge(root) {
  const out = git(root, ['rev-parse', '--git-dir']);
  if (out === null) return false;
  const gitDir = out.trim();
  const gitDirAbs = path.isAbsolute(gitDir) ? gitDir : path.join(root, gitDir);
  return isFile(path.join(gitDirAbs, 'MERGE_HEAD'));
}

/** Every `scripts/ratchet-baseline.*.json` this script finds beside itself —
 *  shared by `verify-accepts` (staged vs HEAD) and `verify-accepts --range`
 *  (each commit vs its parent). This repo arms only one such baseline
 *  (ratchet-baseline.open.json), but the scan itself carries no repo name. */
function baselineFileNames() {
  try {
    return fs.readdirSync(SCRIPT_DIR).filter((f) => /^ratchet-baseline\..+\.json$/.test(f));
  } catch {
    return [];
  }
}

function cmdVerifyAccepts(root, commitMsgFile) {
  if (!commitMsgFile) {
    console.error('usage: ratchet.mjs verify-accepts <commit-msg-file> [--root <path>]');
    process.exit(1);
  }
  if (isMidMerge(root)) return; // an ordinary merge commit — see isMidMerge

  // Resolved against `root` (not the process's own cwd) so this also works
  // spawned with an explicit --root, the way the test suite calls it; the
  // real commit-msg hook runs with root === process.cwd() anyway, since it
  // never passes --root.
  const msgPath = path.isAbsolute(commitMsgFile) ? commitMsgFile : path.join(root, commitMsgFile);
  let message;
  try {
    message = fs.readFileSync(msgPath, 'utf8');
  } catch (e) {
    console.error(`verify-accepts: cannot read commit message file '${commitMsgFile}': ${e.message}`);
    process.exit(1);
  }

  const baselineFiles = baselineFileNames();
  const problems = [];
  for (const name of baselineFiles) {
    const abs = path.join(SCRIPT_DIR, name);
    const relPath = rel(root, abs);
    // "If the baseline file is not staged, the hook does nothing" — `git show
    // :<path>` returns null when the path isn't in the index at all (can't
    // happen for a tracked baseline) or when `root` isn't a usable git
    // checkout (e.g. a throwaway test tree with no .git) — either way,
    // nothing to compare.
    const staged = git(root, ['show', `:${relPath}`]);
    if (staged === null) continue;
    const headText = git(root, ['show', `HEAD:${relPath}`]);
    let stagedJson;
    try {
      stagedJson = JSON.parse(staged);
    } catch {
      continue; // `check` (via loadBaseline) is what guards baseline JSON integrity
    }
    let headJson = { rows: {} };
    if (headText !== null) {
      try { headJson = JSON.parse(headText); } catch { headJson = { rows: {} }; }
    }
    if (headText === staged) continue;
    for (const p of verifyAccepts(headJson, stagedJson, message)) problems.push(`${name}: ${p}`);
  }

  if (problems.length) {
    console.error('commit-msg: baseline raise(s) with no matching Ratchet-Accept trailer:');
    for (const p of problems) console.error(`  ${p}`);
    console.error('If this is a SQUASH landing a branch that already justified these raises, the squash rewrote the message and dropped them — copy every Ratchet-Accept: line from the branch\'s own commits into this one.');
    console.error('Otherwise: run `node scripts/ratchet.mjs accept <row> "<reason>" [--path <path>]...` and paste the printed trailer(s) into this commit message.');
    process.exit(1);
  }
}

// verify-accepts --range <A>..<B> (copied verbatim from the private repo's
// ratchet.mjs, 2026-09-21 joint review, item 8): a retroactive audit over a
// COMMIT RANGE, for exactly the gap the live commit-msg hook cannot close on
// its own — the hook was only added 2026-09-21, `check` is green precisely
// BECAUSE a baseline raise succeeded, so nothing else in CI or review would
// ever flag a trailerless raise that either predates the hook or slipped
// past it (a `--no-verify` commit, a repo this hook was never installed in).
// For each commit in the range that changed a baseline file, this runs the
// exact same `verifyAccepts` pure function against THAT commit's message and
// THAT commit's parent — so a historical audit and the live hook can never
// disagree about what counts as a raise. Ships as a command only; wiring it
// into a CI workflow is a per-repo decision this script doesn't make.
function cmdVerifyAcceptsRange(root, rangeArg) {
  if (!rangeArg || !rangeArg.includes('..')) {
    console.error('usage: ratchet.mjs verify-accepts --range <A>..<B>');
    process.exit(1);
  }
  const commitsOut = git(root, ['rev-list', '--reverse', rangeArg]);
  if (commitsOut === null) {
    console.error(`verify-accepts --range: could not resolve range '${rangeArg}'`);
    process.exit(1);
  }
  const commits = commitsOut.split('\n').map((l) => l.trim()).filter(Boolean);
  const baselineFiles = baselineFileNames();
  const relBaselinePaths = baselineFiles.map((name) => rel(root, path.join(SCRIPT_DIR, name)));

  const problems = [];
  for (const commit of commits) {
    const parentsOut = git(root, ['rev-list', '--parents', '-1', commit]);
    const parents = (parentsOut ?? '').trim().split(/\s+/).slice(1);

    // `git diff-tree` with neither `-m` nor `-c` — deliberately not passed —
    // reports NO paths at all for a merge commit, by git's own documented
    // default (verified: true for a clean merge AND for one resolved through
    // a real conflict, with content matching neither parent). That is this
    // audit's merge exemption, the historical counterpart to isMidMerge's
    // live one: a merge commit is never individually re-litigated here, on
    // the same "already justified on its own branch" theory. It is also
    // this audit's known blind spot — a raise entering ONLY through a merge
    // resolution, never as its own commit anywhere in the range, is invisible
    // to it; `-c`/`-m` would see it but would also re-flag every ordinary
    // clean merge, which is the tradeoff `isMidMerge` makes the same way live.
    const changedOut = git(root, ['diff-tree', '--no-commit-id', '--name-only', '-r', commit]);
    const changed = new Set((changedOut ?? '').split('\n').map((l) => l.trim()).filter(Boolean));
    const touchedBaselines = relBaselinePaths.filter((p) => changed.has(p));
    if (touchedBaselines.length === 0) continue;

    const message = git(root, ['log', '-1', '--format=%B', commit]) ?? '';
    const parentRef = parents[0]; // undefined for a root commit (no parent at all)

    for (const relPath of touchedBaselines) {
      const stagedText = git(root, ['show', `${commit}:${relPath}`]);
      if (stagedText === null) continue; // deleted in this commit — nothing to verify
      let stagedJson;
      try { stagedJson = JSON.parse(stagedText); } catch { continue; }

      let headJson = { rows: {} };
      if (parentRef) {
        const headText = git(root, ['show', `${parentRef}:${relPath}`]);
        if (headText !== null) {
          try { headJson = JSON.parse(headText); } catch { headJson = { rows: {} }; }
        }
      }

      for (const p of verifyAccepts(headJson, stagedJson, message)) {
        problems.push(`${commit.slice(0, 9)} ${relPath}: ${p}`);
      }
    }
  }

  if (problems.length) {
    console.error(`verify-accepts --range ${rangeArg}: baseline raise(s) with no matching Ratchet-Accept trailer:`);
    for (const p of problems) console.error(`  ${p}`);
    process.exit(1);
  }
  console.log(`verify-accepts --range ${rangeArg}: OK — every baseline raise in range is covered by a trailer.`);
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

function main(argv) {
  const args = argv.slice(2);
  const positional = [];
  const only = [];
  let root = process.cwd();
  let range;
  let checkOnly = false;
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--root') root = path.resolve(args[++i]);
    else if (args[i] === '--path') {
      if (!args[i + 1]) {
        console.error('--path needs a file');
        process.exit(1);
      }
      only.push(args[++i]);
    } else if (args[i] === '--range') range = args[++i];
    else if (args[i] === '--check') checkOnly = true;
    else positional.push(args[i]);
  }
  const cmd = positional[0];
  if (cmd === 'check') cmdCheck(root);
  else if (cmd === 'tighten') cmdTighten(root, only);
  else if (cmd === 'accept') cmdAccept(root, positional[1], positional.slice(2).join(' '), only);
  else if (cmd === 'verify-accepts') {
    if (range) cmdVerifyAcceptsRange(root, range);
    else cmdVerifyAccepts(root, positional[1]);
  } else if (cmd === 'docs') cmdDocs(root, { check: checkOnly });
  else {
    console.error('usage: ratchet.mjs <check|tighten|accept|verify-accepts|docs> [args] [--path <file>]... [--range <A>..<B>] [--check] [--root <path>]');
    process.exit(1);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv);
}
