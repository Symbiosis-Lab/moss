#!/usr/bin/env node
// Points local git at .githooks/ (pre-push, commit-msg) for anyone who runs
// `pnpm install` inside a checkout of this repo. npm/pnpm also run the
// package.json `prepare` script when this package is installed as a
// dependency from a registry tarball — there's no .git there, so this exits
// quietly rather than failing that install.
import { execFileSync } from 'node:child_process';

try {
  execFileSync('git', ['rev-parse', '--git-dir'], { stdio: 'ignore' });
} catch {
  process.exit(0); // not a git checkout — nothing to do
}

try {
  execFileSync('git', ['config', 'core.hooksPath', '.githooks'], { stdio: 'ignore' });
} catch (e) {
  console.error(`warning: could not set core.hooksPath (${e.message})`);
}
