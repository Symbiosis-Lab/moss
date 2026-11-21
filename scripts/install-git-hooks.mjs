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

// The merge drivers .gitattributes names. git runs a driver from the checkout's top, so the path is relative.
for (const [name, script] of [['moss-json-keys', 'merge-json.mjs keys'], ['moss-ratchet', 'merge-json.mjs ratchet'], ['moss-changelog', 'merge-changelog.mjs']]) {
  try {
    execFileSync('git', ['config', `merge.${name}.driver`, `node scripts/${script} %O %A %B`], { stdio: 'ignore' });
  } catch (e) {
    console.error(`warning: could not register the ${name} merge driver (${e.message})`);
  }
}
