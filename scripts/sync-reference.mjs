#!/usr/bin/env node
// sync-reference.mjs — regenerate the auto: regions of the docs reference pages
// from `moss describe --json`, so the published contract can never drift from
// the running binary. Source of truth = the moss binary, not this file.
//
//   node scripts/sync-reference.mjs --check   # exit 1 if any region is stale
//   node scripts/sync-reference.mjs --write   # rewrite stale regions in place
//
// The describe JSON comes from `$MOSS_BIN describe --json` (MOSS_BIN defaults to
// `moss` on PATH). The pre-push hook and the pull-request workflow point
// MOSS_BIN at the CLI built from the tree being checked.
//
// Region markers (one convention everywhere):
//   <!-- auto:start:NAME -->
//   ...generated table...
//   <!-- auto:end:NAME -->
// Content between the markers is owned by this generator; edit the source
// (tokens.json / components.rs / the describe command) instead.

import { readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SITE = join(dirname(fileURLToPath(import.meta.url)), "..", "site");
const code = (s) => "`" + s + "`";
const cell = (s) => String(s ?? "").replace(/\|/g, "\\|").replace(/\n+/g, " ").trim();
const table = (headers, rows) =>
  [
    "| " + headers.join(" | ") + " |",
    "|" + headers.map(() => "---").join("|") + "|",
    ...rows.map((r) => "| " + r.map(cell).join(" | ") + " |"),
  ].join("\n");

// describe → { region-name: { page, md } }. One region per page.
function regionsFor(d) {
  const out = {};

  // CSS tokens — one flat table across all categories
  // tokens is an object keyed by category; flatten to [Category, Variable, Default, Description]
  const tokenRows = [];
  for (const [cat, list] of Object.entries(d.tokens ?? {})) {
    for (const t of list) {
      tokenRows.push([cat, code(`--${t.name}`), code(t.value), t.description || ""]);
    }
  }
  out["css-tokens"] = {
    page: "docs/reference/css-tokens.md",
    md: table(["Category", "Variable", "Default", "Description"], tokenRows),
  };

  // Component classes — [Class, Kind, Description]
  out["components"] = {
    page: "docs/reference/components.md",
    md: table(
      ["Class", "Kind", "Description"],
      (d.components ?? []).map((c) => [code(`.${c.class}`), c.kind || "", c.description || ""]),
    ),
  };

  // CLI commands — [Command, Arguments, Description]
  out["cli"] = {
    page: "docs/reference/cli.md",
    md: table(
      ["Command", "Arguments", "Description"],
      (d.cli_commands ?? []).map((c) => [code(c.name), code(c.args), c.description || ""]),
    ),
  };

  // Plugin hooks — [Hook, Arity, Context, Description]
  out["hooks"] = {
    page: "docs/reference/hooks.md",
    md: table(
      ["Hook", "Arity", "Context", "Description"],
      (d.plugin_hooks ?? []).map((h) => [code(h.name), h.arity, code(h.context), h.description || ""]),
    ),
  };

  // Manifest fields — [Field, Type, Required, Description]
  out["manifest"] = {
    page: "docs/reference/manifest.md",
    md: table(
      ["Field", "Type", "Required", "Description"],
      (d.manifest_fields ?? []).map((f) => [code(f.name), code(f.type), f.required ? "yes" : "no", f.description || ""]),
    ),
  };

  // Template slots — [Slot, Position, Authorable]
  out["slots"] = {
    page: "docs/reference/slots.md",
    md: table(
      ["Slot", "Position", "Authorable"],
      (d.slots ?? []).map((s) => [code(s.name), s.position || "", s.authorable ? "yes" : "no"]),
    ),
  };

  return out;
}

function replaceRegion(text, name, body) {
  const re = new RegExp(`(<!-- auto:start:${name} -->)[\\s\\S]*?(<!-- auto:end:${name} -->)`);
  if (!re.test(text)) return { text, found: false };
  return { text: text.replace(re, `$1\n${body}\n$2`), found: true };
}

function main() {
  const mode = process.argv.includes("--write") ? "write" : "check";
  const bin = process.env.MOSS_BIN || "moss";
  const raw = execFileSync(bin, ["describe", "--json"], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  const d = JSON.parse(raw);
  console.error(`describe schema v${d.describe_schema_version} from ${bin} (${d.moss_binary_version ?? "?"})`);

  const regions = regionsFor(d);
  // group regions by page
  const byPage = {};
  for (const [name, r] of Object.entries(regions)) (byPage[r.page] ??= []).push([name, r.md]);

  let stale = 0, missing = 0;
  for (const [page, list] of Object.entries(byPage)) {
    const path = join(SITE, page);
    let text;
    try { text = readFileSync(path, "utf8"); } catch { console.error(`  ! page not found: ${page}`); missing += list.length; continue; }
    let next = text;
    for (const [name, body] of list) {
      const res = replaceRegion(next, name, body);
      if (!res.found) { console.error(`  - ${page}: no marker auto:${name} (needs wiring)`); missing++; continue; }
      if (res.text !== next) { stale++; if (mode === "check") console.error(`  ✗ stale: ${page} [${name}]`); }
      next = res.text;
    }
    if (mode === "write" && next !== text) { writeFileSync(path, next); console.error(`  ✓ wrote ${page}`); }
  }

  console.error(`\n${mode}: ${stale} stale region(s), ${missing} unwired/missing region(s)`);
  if (mode === "check" && stale > 0) {
    console.error("Reference docs are out of date. Run: node scripts/sync-reference.mjs --write");
    process.exit(1);
  }
}

main();
