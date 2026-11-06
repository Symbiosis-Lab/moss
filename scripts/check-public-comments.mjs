#!/usr/bin/env node
// Fails when a comment in public Rust/TypeScript/JS source carries private
// process vocabulary: plan labels, internal ticket ids, ADR ids, references to
// private documents. See "This repository is public" in .claude/CLAUDE.md.
//
// Best-effort lexer: line and block comments are extracted while skipping
// string literals (Rust "..", r#".."#, char literals; TS/JS '..', "..", `..`).
// Regex literals are not understood, so a regex containing `//` could confuse it.
//
// Usage: node scripts/check-public-comments.mjs [rootDir]

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, extname, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const PATTERNS = [
  ["ticket id", /\bLOG-[0-9A-F]{4}-/],
  ["plan stage label", /\b(?:Phase|Stage)[ -]\d+[a-z]?\b/],
  ["plan milestone label", /\bM6[ab]\b/],
  ["PR label", /\bPR\d+[a-z]?\b/],
  ["private document reference", /NORTH-STAR|\bshape-spec\b/],
  ["ADR id", /\bADR-\d+/],
  ["private ratchet row", /\b[Rr]ow \([a-z]\)/],
  ["report reference", /\breport \d+ §/],
  // A plan task label; `T10:30` style times and `T00:00:00Z` are not labels.
  ["plan task label", /\bT\d{1,2}[a-z]?\b(?!:\d)/],
];

// Legitimate matches: [path suffix, exact phrase, reason].
export const ALLOW = [
  ["moss-build/src/seta/client.rs", '"LOG-2A6D-T1820-04-26"', "documents the shape of the id the hosting service assigns"],
  ["toast/toast-types.ts", "LOG-ABCD-0001", "example of the id shown to the user, as the service returns it"],
  ["seta/domains/wire.rs", "T1 (Tor)", "a country code, not a plan label"],
];

const SKIP_DIRS = new Set(["node_modules", "target", "dist", "fixtures", "__snapshots__", "snapshots", "generated", ".git"]);
const EXTS = new Set([".rs", ".ts", ".js", ".mjs"]);

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    if (SKIP_DIRS.has(name)) continue;
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) yield* walk(p);
    else if (EXTS.has(extname(name)) && !/\.(d|generated|min)\.[a-z]+$/.test(name)) yield p;
  }
}

function sourceFiles(root) {
  const out = [];
  const crates = join(root, "crates");
  try { out.push(...walk(crates)); } catch {}
  try {
    for (const pkg of readdirSync(join(root, "packages"))) {
      try { out.push(...walk(join(root, "packages", pkg, "src"))); } catch {}
    }
  } catch {}
  return out;
}

/** Yields { line, text } for each comment (block comments yield per line). */
export function* comments(src, rust) {
  let i = 0, line = 1;
  const n = src.length;
  while (i < n) {
    const c = src[i], d = src[i + 1];
    if (c === "\n") { line++; i++; continue; }
    if (c === "/" && d === "/") {
      let j = src.indexOf("\n", i);
      if (j < 0) j = n;
      yield { line, text: src.slice(i, j) };
      i = j;
      continue;
    }
    if (c === "/" && d === "*") {
      let j = src.indexOf("*/", i + 2);
      j = j < 0 ? n : j + 2;
      let l = line;
      for (const t of src.slice(i, j).split("\n")) yield { line: l++, text: t };
      line = l - 1;
      i = j;
      continue;
    }
    if (rust && c === "r" && /[#"]/.test(d ?? "") && !/\w/.test(src[i - 1] ?? "")) {
      let k = i + 1, hashes = 0;
      while (src[k] === "#") { hashes++; k++; }
      if (src[k] === '"') {
        const end = src.indexOf('"' + "#".repeat(hashes), k + 1);
        const j = end < 0 ? n : end + 1 + hashes;
        line += (src.slice(i, j).match(/\n/g) ?? []).length;
        i = j;
        continue;
      }
    }
    if (rust && c === "'") {
      // Char literal ('x', '\n', '"') versus lifetime ('a).
      const m = /^'(?:\\.[^']*|[^\\'])'/.exec(src.slice(i, i + 12));
      i += m ? m[0].length : 1;
      continue;
    }
    if (c === '"' || (!rust && (c === "'" || c === "`"))) {
      let k = i + 1;
      while (k < n && src[k] !== c) {
        if (src[k] === "\\") k++;
        if (src[k] === "\n") line++;
        k++;
      }
      i = k + 1;
      continue;
    }
    i++;
  }
}

export function check(root) {
  const hits = [];
  for (const file of sourceFiles(root)) {
    const rel = relative(root, file).split(sep).join("/");
    const rust = file.endsWith(".rs");
    for (const { line, text } of comments(readFileSync(file, "utf8"), rust)) {
      for (const [label, re] of PATTERNS) {
        const m = re.exec(text);
        if (!m) continue;
        if (ALLOW.some(([p, phrase]) => rel.endsWith(p) && text.includes(phrase))) continue;
        hits.push(`${rel}:${line}: ${label}: "${m[0]}"`);
      }
    }
  }
  return hits;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const root = process.argv[2] ?? join(fileURLToPath(import.meta.url), "..", "..");
  const hits = check(root);
  if (hits.length) {
    console.error(hits.join("\n"));
    console.error(`\n${hits.length} private process label(s) in comments. Say the technical reason instead (see .claude/CLAUDE.md, "This repository is public").`);
    process.exit(1);
  }
}
