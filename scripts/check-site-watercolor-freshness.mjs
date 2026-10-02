#!/usr/bin/env node
// Refuses to pass when site/vendor/moss-watercolor/moss-watercolor.js --
// the landing's own checked-in copy of packages/moss-watercolor, loaded by
// site/index.html as a classic script -- no longer matches a fresh build of
// that package. The landing has no bundler of its own, so this file is a
// build artifact like packages/moss-syntax's or packages/moss-api's dist/,
// and needs the same drift check those get from check-dist-freshness.mjs;
// it lives apart from that script because its trigger is the *package's*
// source, not a change under the vendored file's own directory, and its
// rebuild command is scripts/build-site-watercolor.mjs, not a package's own
// `pnpm run build`.
import { spawnSync } from 'node:child_process';

const TRIGGERS = ['packages/moss-watercolor/src/', 'scripts/build-site-watercolor.mjs', 'site/vendor/moss-watercolor/'];

function run(cmd, args) {
  return spawnSync(cmd, args, { encoding: 'utf8' });
}

function stagedFiles() {
  const r = run('git', ['diff', '--cached', '--name-only']);
  if (r.status !== 0) return [];
  return r.stdout.split('\n').filter(Boolean);
}

function main() {
  const staged = stagedFiles();
  if (!staged.some((f) => TRIGGERS.some((t) => f === t || f.startsWith(t)))) process.exit(0);

  const build = run('node', ['scripts/build-site-watercolor.mjs']);
  if (build.status !== 0) {
    console.error('check-site-watercolor-freshness: build failed:\n' + (build.stderr || build.stdout));
    process.exit(1);
  }
  const diff = run('git', ['diff', '--exit-code', '--', 'site/vendor/moss-watercolor']);
  if (diff.status !== 0) {
    console.error(
      'site/vendor/moss-watercolor/moss-watercolor.js is stale — run `node scripts/build-site-watercolor.mjs`, then `git add site/vendor/moss-watercolor` and commit again.'
    );
    process.exit(1);
  }
  process.exit(0);
}

main();
