#!/usr/bin/env node
// The released-contract list and the content lock record what releases shipped, so only a release cut may
// change them, and then only by adding lines. Run by the pre-push hook with git's pre-push lines on stdin
// (`<local ref> <sha> <remote ref> <sha>`), or by hand with a commit: `node scripts/check-released-contract.mjs HEAD`.
//
// On a push to a `release/v*` branch each file may grow; everywhere else it
// must match the newest release tag the commit contains. Before any tag
// carries the file, there is nothing to compare against and the check passes.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const FILES = ["crates/moss-core/src/contract/released-contract.txt", "crates/moss-build/tests/content.lock"];

const git = (...args) => {
  try {
    return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
  } catch {
    return null;
  }
};

const lines = (text) => new Set(text.split("\n").map((l) => l.trim()).filter((l) => l && !l.startsWith("#")));

function check(sha, mayGrow) {
  const tag = git("describe", "--tags", "--abbrev=0", "--match", "v[0-9]*", sha)?.trim();
  if (!tag) return [];
  const problems = [];
  for (const file of FILES) {
    const released = git("show", `${tag}:${file}`);
    if (released === null) continue;
    const now = git("show", `${sha}:${file}`) ?? "";
    const was = lines(released);
    const is = lines(now);
    const dropped = [...was].filter((l) => !is.has(l));
    const added = [...is].filter((l) => !was.has(l));
    if (dropped.length) problems.push(`${file} drops lines ${tag} released:\n  ${dropped.join("\n  ")}`);
    if (added.length && !mayGrow) {
      problems.push(`${file} adds lines outside a release branch (only a release cut writes it):\n  ${added.slice(0, 20).join("\n  ")}`);
    }
  }
  return problems;
}

const ZERO = /^0+$/;
const pushes = process.argv[2]
  ? [{ sha: process.argv[2], mayGrow: false }]
  : readFileSync(0, "utf8")
      .split("\n")
      .map((l) => l.trim().split(/\s+/))
      .filter((f) => f.length === 4 && !ZERO.test(f[1]))
      .map(([, sha, remoteRef]) => ({ sha, mayGrow: /^refs\/heads\/release\/v/.test(remoteRef) }));

const problems = pushes.flatMap(({ sha, mayGrow }) => check(sha, mayGrow));
if (problems.length) {
  console.error(`check-released-contract: ${problems.join("\n")}`);
  process.exit(1);
}
