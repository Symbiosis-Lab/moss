import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT = join(dirname(fileURLToPath(import.meta.url)), "..", "check-released-contract.mjs");
const FILE = "crates/moss-core/src/contract/released-contract.txt";
// Run from a git hook, an inherited GIT_DIR would point every call here at the real repository.
const env = Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.startsWith("GIT_")));

function repo() {
  const dir = mkdtempSync(join(tmpdir(), "released-contract-"));
  const git = (...args) => execFileSync("git", args, { cwd: dir, env, encoding: "utf8" }).trim();
  git("init", "-q");
  git("config", "user.email", "test@example.invalid");
  git("config", "user.name", "test");
  const commit = (text, msg, path = FILE) => {
    mkdirSync(join(dir, dirname(path)), { recursive: true });
    writeFileSync(join(dir, path), text);
    git("add", "-A");
    git("commit", "-q", "-m", msg);
    return git("rev-parse", "HEAD");
  };
  return { dir, git, commit };
}

function push(dir, sha, ref) {
  const r = spawnSync("node", [SCRIPT], { cwd: dir, env, input: `refs/heads/x ${sha} ${ref} ${"0".repeat(40)}\n`, encoding: "utf8" });
  return { ok: r.status === 0, err: r.stderr };
}

const RELEASED = "# header\nclass moss-a\nclass moss-b\n";

function released() {
  const r = repo();
  r.commit(RELEASED, "release");
  r.git("tag", "v0.1.0");
  return r;
}

test("an unchanged list pushes anywhere", () => {
  const { dir, git } = released();
  assert.ok(push(dir, git("rev-parse", "HEAD"), "refs/heads/develop").ok);
});

test("dropping a released line is refused, even on a release branch", () => {
  const { dir, commit } = released();
  const sha = commit("# header\nclass moss-a\n", "drop");
  const res = push(dir, sha, "refs/heads/release/v0.2.0");
  assert.ok(!res.ok);
  assert.match(res.err, /drops lines v0\.1\.0 released:\n {2}class moss-b/);
});

test("adding a line outside a release branch is refused", () => {
  const { dir, commit } = released();
  const sha = commit(RELEASED + "class moss-c\n", "add");
  const res = push(dir, sha, "refs/heads/develop");
  assert.ok(!res.ok);
  assert.match(res.err, /only a release cut writes it/);
});

test("a release branch may add lines", () => {
  const { dir, commit } = released();
  const sha = commit(RELEASED + "class moss-c\n", "cut");
  assert.ok(push(dir, sha, "refs/heads/release/v0.2.0").ok);
});

test("before any tag carries the list there is nothing to compare", () => {
  const { dir, commit, git } = repo();
  commit("unrelated\n", "first", "README");
  git("tag", "v0.1.0");
  const sha = commit(RELEASED, "introduce the list");
  assert.ok(push(dir, sha, "refs/heads/develop").ok);
});

test("the content lock is held the same way", () => {
  const LOCK = "crates/moss-build/tests/content.lock";
  const r = repo();
  r.commit("grid input abc\ngrid problems 1\n", "release", LOCK);
  r.git("tag", "v0.1.0");
  const sha = r.commit("grid input abc\ngrid problems 1\ngrid problems 0\n", "append", LOCK);
  const res = push(r.dir, sha, "refs/heads/develop");
  assert.ok(!res.ok);
  assert.match(res.err, /only a release cut writes it/);
  assert.ok(push(r.dir, sha, "refs/heads/release/v0.2.0").ok);
});
