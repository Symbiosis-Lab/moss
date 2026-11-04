#!/usr/bin/env node
// check-dist-freshness.mjs — refuses a commit that stages a change under
// one of PACKAGES' own src/ (or its build config) without also rebuilding
// the committed output that ships from it: a package's dist/, or the bundles
// moss-build embeds.
//
// The only other gate that catches a stale dist is pr.yml's "Package
// artifacts are up to date" step, which runs solely on a pull request to
// main — a develop landing bypasses it entirely. That gap is what let a
// pair of commits changing packages/moss-syntax/src land on develop with
// dist/ still shipping the old shortcode catalog.
//
// Scoped per package: a commit touching neither trigger list below costs
// one `git diff --cached --name-only` call and no build.
//
// Wired from .githooks/pre-commit, same shape as commit-msg/pre-push:
//   node scripts/check-dist-freshness.mjs

import { spawnSync } from 'node:child_process';

const PACKAGES = [
  {
    label: 'moss-syntax',
    filterName: '@symbiosis-lab/moss-syntax',
    distPath: 'packages/moss-syntax/dist',
    triggers: [
      'packages/moss-syntax/src/',
      'packages/moss-syntax/tsdown.config.ts',
      'packages/moss-syntax/tsconfig.json',
      'packages/moss-syntax/package.json',
    ],
  },
  {
    label: 'moss-api',
    filterName: '@symbiosis-lab/moss-api',
    distPath: 'packages/moss-api/dist',
    triggers: [
      'packages/moss-api/src/',
      'packages/moss-api/tsdown.config.ts',
      'packages/moss-api/tsconfig.json',
      'packages/moss-api/package.json',
    ],
  },
  {
    label: 'moss-watercolor',
    filterName: '@symbiosis-lab/moss-watercolor',
    distPath: 'packages/moss-watercolor/dist',
    triggers: [
      'packages/moss-watercolor/src/',
      'packages/moss-watercolor/tsdown.config.ts',
      'packages/moss-watercolor/tsconfig.json',
      'packages/moss-watercolor/package.json',
    ],
  },
  {
    label: 'moss-blueprint-grid',
    filterName: '@symbiosis-lab/moss-blueprint-grid',
    distPath: 'packages/moss-blueprint-grid/dist',
    triggers: [
      'packages/moss-blueprint-grid/src/',
      'packages/moss-blueprint-grid/tsdown.config.ts',
      'packages/moss-blueprint-grid/tsconfig.json',
      'packages/moss-blueprint-grid/package.json',
    ],
  },
  // The bundles moss-build embeds. After the packages above, because the bridge
  // bundles a package's dist/: a package change rebuilds its dist, then this.
  {
    label: 'backend scripts',
    build: ['node', ['scripts/build-backend-scripts.mjs']],
    distPath: ['crates/moss-build/src/assets/js', 'crates/moss-build/src/ops/serve/js'],
    triggers: [
      'crates/moss-build/src/js-src/',
      'scripts/build-backend-scripts.mjs',
      'packages/moss-blueprint-grid/',
    ],
  },
];

function run(cmd, args) {
  return spawnSync(cmd, args, { encoding: 'utf8' });
}

function stagedFiles() {
  const r = run('git', ['diff', '--cached', '--name-only']);
  if (r.status !== 0) return [];
  return r.stdout.split('\n').filter(Boolean);
}

function touches(file, triggers) {
  return triggers.some((t) => file === t || file.startsWith(t));
}

function main() {
  const staged = stagedFiles();
  const relevant = PACKAGES.filter((pkg) => staged.some((f) => touches(f, pkg.triggers)));
  if (relevant.length === 0) process.exit(0);

  // Never block a commit over a missing interpreter — same posture as
  // pre-push toward a missing `node`.
  const pnpmCheck = run('pnpm', ['--version']);
  if (pnpmCheck.status !== 0 || pnpmCheck.error) {
    console.error('check-dist-freshness: pnpm not found on PATH — skipping the dist check.');
    process.exit(0);
  }

  let sawFailure = false;
  for (const pkg of relevant) {
    const [cmd, args] = pkg.build ?? ['pnpm', ['--filter', pkg.filterName, 'run', 'build']];
    const build = run(cmd, args);
    if (build.status !== 0) {
      console.error(`check-dist-freshness: ${pkg.label} build failed — fix the build before committing.`);
      sawFailure = true;
      continue;
    }
    const paths = [].concat(pkg.distPath);
    const diff = run('git', ['diff', '--exit-code', '--', ...paths]);
    if (diff.status !== 0) {
      console.error(
        `${paths.join(' and ')} is stale — run \`${[cmd, ...args].join(' ')}\`, then \`git add ${paths.join(' ')}\` and commit again.`
      );
      sawFailure = true;
    }
  }

  process.exit(sawFailure ? 1 : 0);
}

main();
