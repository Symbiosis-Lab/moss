#!/usr/bin/env node
/**
 * Whether the package's UI (src/ui/) is ready to publish: a stranger can read every comment,
 * nothing names what is private, and everything it carries may be published
 * under its licence.
 *
 *   1. History and outside references, in comments and test titles: doc paths,
 *      ADR ids, tracker numbers, plan labels and dates, and any file path that
 *      names no file in this repository, which a reader here cannot open. A
 *      comment says what the code cannot; its history belongs in the commit
 *      message.
 *   2. Private names anywhere in any file of the package, from the rules file
 *      that MOSS_SCRUB_RULES names. The list itself is not published, so the
 *      command refuses to pass without it: no file, an unreadable one or an
 *      empty list is a failure, not a skipped check.
 *   3. Licence and provenance: a third-party licence header, code noted as
 *      taken from somewhere else, an asset file, and each npm package the code
 *      imports, whose licence must be one the package may depend on.
 *
 *   MOSS_SCRUB_RULES=<rules.json> node scripts/public-ready.mjs    # from packages/moss-ui: lists every hit
 */
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { builtinModules } from "node:module";
import { dirname, join, posix, relative } from "node:path";
import { fileURLToPath } from "node:url";

/** Rules read in comments and test titles. */
export const HISTORY_RULES = [
  // This repo's docs, not a URL's path: https://codemirror.net/docs/ref is public.
  { name: "doc-path", re: /(?<![\w./-])docs\/[\w./-]+/g },
  { name: "adr", re: /\bADR[- ]?\d+\b/g },
  // `#123` and `moss#12`, not a CSS colour such as `#333` or `#000000` in a stylesheet. An
  // upstream project's issue (`tauri#10194`) is public and stays.
  { name: "tracker", re: /(?<![\w&#/])#\d{2,5}\b(?![\w-])|\bmoss#\d+\b|\b(?:issue|PR) \d{2,5}\b/gi },
  // Labels such as "Task 4.2", "slice 3", "Law 1", "decision A" and "C3:"; not ordinary prose such as
  // "step 1", a heading level or a C0/C1 control character.
  {
    name: "plan-label",
    re: /\b(?:Task|Phase|Plan|Bundle|Slice|Cut|Wave|Gap|Milestone|Law|Decision|Blocker|BLOCKER)\s[0-9][\w.]*|\bThread-\d+\b|\bthis (?:task|investigation|slice|plan)\b|\b(?:slice|phase|task|bundle) \d[\w.-]*|\b[Dd]ecision [A-Z]\b|\bRC-\d+\b|\b(?![Hh][1-6]\b)(?!C[01] control)[A-Z]\d(?:\.\d+)?\b(?=[\s:—)\]-])/g,
  },
  { name: "date", re: /\b20\d\d-\d\d-\d\d\b/g },
  // A section of a document (`design §4`); a published standard's (`RFC 9110 §15.5`) is public.
  { name: "doc-section", re: /(?<!\bRFC ?\d{3,5},? )§\s*[\w.-]+/g },
  { name: "history", re: /\b(?:on|in) this branch\b/gi },
];

/** A file path or name in prose; a URL's path is not one, since nothing may precede it but space. */
const FILE_PATH = /(?<![\w./@:-])@?(?:[\w.-]+\/)*[\w-][\w.-]*\.(?:[cm]?[jt]sx?|css|json|rs|html|md|toml|ya?ml|sh|py)\b(?![\w/-])/g;
/** How this package names its own modules, mapped onto the files that hold them. */
const SELF_SPECIFIER = /^@symbiosis-lab\/moss-ui\/ui\//;

/**
 * Whether a path in a comment names a tracked file. A path may be relative to
 * the repository, the package or the file it sits in, so any tracked path that
 * ends with it counts; a path starting `./` or `../` must name the file it
 * resolves to from `from`, the repo-relative path of the file it sits in. A
 * bare file name counts only when one tracked file has it: `style.css` beside
 * four fixtures of that name could mean any of them, or a file in another
 * repository. The names of a site's own files are prose, not paths.
 */
const SITE_FILES = new Set(["index.md", "config.toml"]);
export function pathResolver(tracked) {
  const exact = new Set(tracked);
  const suffixes = new Map();
  for (const file of tracked) {
    const parts = file.split("/");
    for (let i = 0; i < parts.length; i++) {
      const suffix = parts.slice(i).join("/");
      suffixes.set(suffix, (suffixes.get(suffix) ?? 0) + 1);
    }
  }
  return (token, from = "") => {
    if (/^\.\.?\//.test(token)) return exact.has(posix.join(posix.dirname(from), token));
    if (SITE_FILES.has(token)) return true;
    const path = token.replace(SELF_SPECIFIER, "packages/moss-ui/src/ui/");
    const count = suffixes.get(path) ?? 0;
    return path.includes("/") ? count > 0 : count === 1;
  };
}

/** A licence header, or a note that code came from elsewhere. */
const LICENCE_HEADER = /SPDX-License-Identifier|Copyright \(c\)|\bLicensed under\b|\b(?:MIT|Apache|BSD|GPL|MPL) License\b/i;
const TAKEN_FROM = /\b(?:copied|ported|adapted|vendored|borrowed|lifted|taken)\s+(?:verbatim\s+)?from\s+\S*(?:https?:\/\/|github\.com|stackoverflow|gist\.)|\bbased on https?:\/\//i;
const ASSET = /\.(?:png|jpe?g|gif|webp|avif|svg|ico|woff2?|ttf|otf|mp4|webm|mp3|wav)$/i;

/** Licences an MIT package may depend on and ship. */
const PERMITTED_LICENCES = new Set(["MIT", "ISC", "BSD-2-Clause", "BSD-3-Clause", "Apache-2.0", "0BSD", "CC0-1.0", "Unlicense", "BlueOak-1.0.0", "MIT OR Apache-2.0", "(MIT OR Apache-2.0)"]);

/**
 * The comment text of each line, with its line number; code and string
 * contents are blanked. Line-based: a block comment's state carries over.
 */
export function commentLines(source, ext) {
  const out = [];
  const cssOnly = ext === ".css";
  let block = null; // the terminator of the open block comment
  let quote = null; // only a template literal stays open across lines
  source.split("\n").forEach((line, i) => {
    let text = "";
    if (quote !== "`") quote = null;
    for (let j = 0; j < line.length; j++) {
      if (block) {
        const end = line.indexOf(block, j);
        if (end === -1) { text += line.slice(j); j = line.length; break; }
        text += line.slice(j, end) + " ";
        j = end + block.length - 1;
        block = null;
        continue;
      }
      const c = line[j];
      if (quote) {
        if (c === "\\") j++;
        else if (c === quote) quote = null;
        continue;
      }
      if (!cssOnly && (c === "'" || c === '"' || c === "`")) { quote = c; continue; }
      if (line.startsWith("/*", j)) { block = "*/"; j++; continue; }
      if (!cssOnly && c === "/" && line[j + 1] !== "/" && /(?:^|[(,=:[!&|?{};+\-*%<>~^]|\breturn)\s*$/.test(line.slice(0, j))) {
        // A regex literal: its quotes and slashes are not code or comments.
        let inClass = false;
        for (j++; j < line.length; j++) {
          if (line[j] === "\\") j++;
          else if (line[j] === "[") inClass = true;
          else if (line[j] === "]") inClass = false;
          else if (line[j] === "/" && !inClass) break;
        }
        continue;
      }
      if (line.startsWith("<!--", j)) { block = "-->"; j += 3; continue; }
      if (!cssOnly && line.startsWith("//", j)) { text += line.slice(j + 2); break; }
    }
    if (text.trim()) out.push([i + 1, text]);
  });
  return out;
}

/** Test titles: the first string argument of describe/it/test. */
export function testTitles(source) {
  const out = [];
  // `test('…')`, `it.skip(\n  '…')` and `it.each([…])('… %s')`; the title may start on the next line.
  const title = /\b(?:describe|it|test)(?:\.\w+)*(?:\((?:[^()]|\([^()]*\))*\))?\(\s*(['"`])((?:\\.|(?!\1)[^\\])*)\1/g;
  for (const m of source.matchAll(title)) out.push([source.slice(0, m.index).split("\n").length, m[2]]);
  return out;
}

function packageName(spec) {
  const parts = spec.split("/");
  return spec.startsWith("@") ? parts.slice(0, 2).join("/") : parts[0];
}

/**
 * Every hit in one file, as { file, line, rule, match }. `known` answers
 * whether a path names a tracked file; without it paths go unchecked.
 * `repoPath` is the file's path from the repository root.
 */
export function scanFile(file, source, privateRules, known, repoPath = file) {
  const hits = [];
  const ext = file.slice(file.lastIndexOf("."));
  const isTest = /\/__tests__\/|\.test\.[cm]?[jt]s$|test-setup/.test(file);
  const readable = [...commentLines(source, ext), ...(isTest ? testTitles(source) : [])];
  for (const [line, text] of readable) {
    for (const rule of HISTORY_RULES) {
      for (const m of text.matchAll(rule.re)) {
        if (rule.name === "tracker" && ext === ".css" && /^#\d{3}(?:\d{3})?$/.test(m[0])) continue;
        hits.push({ file, line, rule: rule.name, match: m[0].trim() });
      }
    }
    if (known) {
      for (const m of text.matchAll(FILE_PATH)) if (!known(m[0], repoPath)) hits.push({ file, line, rule: "unknown-path", match: m[0] });
    }
    if (LICENCE_HEADER.test(text)) hits.push({ file, line, rule: "licence-header", match: text.trim().slice(0, 60) });
    if (TAKEN_FROM.test(text)) hits.push({ file, line, rule: "taken-from", match: text.trim().slice(0, 60) });
  }
  if (privateRules) {
    source.split("\n").forEach((text, i) => {
      for (const rule of privateRules) {
        if ((rule.allow ?? []).some((a) => file.startsWith(a))) continue;
        if (new RegExp(rule.pattern, rule.flags ?? "").test(text)) hits.push({ file, line: i + 1, rule: "private-name", match: rule.pattern });
      }
    });
  }
  return hits;
}

/** The npm packages the UI imports whose licence is not permitted, or unknown. */
export function packageLicences(root, files) {
  const used = new Map();
  for (const file of files) {
    const source = readFileSync(join(root, file), "utf8");
    for (const m of source.matchAll(/^\s*(?:import\s+(?:type\s+)?(?:[\w*{}\s,]+\s+from\s+)?|export\s+(?:type\s+)?(?:\*|\{[^}]*\})\s+from\s+)(["'])([^."'/][^"']*)\1|\bimport\(\s*(["'])([^."'/][^"']*)\3|\bvi\.mock\(\s*(["'])([^."'/][^"']*)\5/gm)) {
      const name = packageName(m[2] ?? m[4] ?? m[6]);
      if (name.startsWith("node:") || builtinModules.includes(name) || name.startsWith("@symbiosis-lab/")) continue;
      if (!used.has(name)) used.set(name, file);
    }
  }
  const hits = [];
  for (const [name, file] of [...used].sort()) {
    const manifest = join(root, "node_modules", name, "package.json");
    const licence = existsSync(manifest) ? JSON.parse(readFileSync(manifest, "utf8")).license : undefined;
    const id = typeof licence === "string" ? licence : licence?.type;
    if (!PERMITTED_LICENCES.has(id)) hits.push({ file, line: 0, rule: "package-licence", match: `${name}: ${id ?? "unknown"}` });
  }
  return hits;
}

/** The private-name rules at `path`; throws unless it names a file holding at least one rule. */
export function loadPrivateRules(path) {
  if (!path) throw new Error("MOSS_SCRUB_RULES names no rules file, so private names cannot be checked");
  let rules;
  try {
    rules = JSON.parse(readFileSync(path, "utf8"));
  } catch (err) {
    throw new Error(`MOSS_SCRUB_RULES: cannot read ${path}: ${err.message}`);
  }
  rules = Array.isArray(rules) ? rules.filter((r) => typeof r.pattern === "string") : [];
  if (rules.length === 0) throw new Error(`MOSS_SCRUB_RULES: ${path} holds no rules`);
  return rules;
}

/**
 * Every file under src/ui/, package-relative. The contract beside it is
 * generated from the engine's command table and inherits that table's docs.
 */
function filesUnder(root, dir) {
  const out = [];
  const walk = (d) => {
    for (const entry of readdirSync(d, { withFileTypes: true })) {
      if (entry.name === "node_modules") continue;
      const abs = join(d, entry.name);
      if (entry.isDirectory()) walk(abs);
      else out.push(relative(root, abs).split("\\").join("/"));
    }
  };
  walk(join(root, dir));
  return out.sort();
}

/** The repository's files, tracked or about to be, and the package's path within it. */
function gitView(root) {
  const git = (args, cwd = root) => execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
  const top = git(["rev-parse", "--show-toplevel"]);
  return {
    tracked: git(["ls-files", "--cached", "--others", "--exclude-standard"], top).split("\n").filter(Boolean),
    prefix: git(["rev-parse", "--show-prefix"]),
  };
}

/**
 * `tracked` lists the repository's files and `prefix` is the package's path
 * within it; both default to git's view of the repository holding `root`.
 * `privateRules`, when given, are checked in every file of the package; the
 * UI's files get every other check too.
 */
export function check(root, { tracked, prefix = "", privateRules = null } = {}) {
  if (!tracked) ({ tracked, prefix } = gitView(root));
  const files = filesUnder(root, "src/ui");
  const known = pathResolver(tracked);
  const hits = [];
  for (const file of files) {
    if (ASSET.test(file)) { hits.push({ file, line: 0, rule: "asset", match: "needs a stated source and licence" }); continue; }
    hits.push(...scanFile(file, readFileSync(join(root, file), "utf8"), null, known, prefix + file));
  }
  if (privateRules) {
    for (const file of filesUnder(root, ".")) {
      if (ASSET.test(file)) continue;
      hits.push(...scanFile(file, readFileSync(join(root, file), "utf8"), privateRules).filter((h) => h.rule === "private-name"));
    }
  }
  hits.push(...packageLicences(root, files.filter((f) => /\.[cm]?[jt]s$/.test(f))));
  return { files, hits };
}

// `node scripts/public-ready.mjs [package root]`; the root defaults to this package.
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const root = process.argv[2] ?? dirname(dirname(fileURLToPath(import.meta.url)));
  let privateRules;
  try {
    privateRules = loadPrivateRules(process.env.MOSS_SCRUB_RULES);
  } catch (err) {
    console.error(`public-ready: ${err.message}.`);
    process.exit(2);
  }
  const { files, hits } = check(root, { privateRules });
  if (hits.length > 0) {
    for (const h of hits) console.error(`${h.file}:${h.line}: ${h.rule}: ${h.match}`);
    console.error(`public-ready: ${hits.length} hit(s).`);
    process.exit(1);
  }
  console.log(`public-ready: ${files.length} files are ready to publish; ${privateRules.length} private-name rules checked.`);
}
