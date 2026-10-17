#!/usr/bin/env node
// Run this before every `moss-cli deploy` of the landing page, staging or
// production; a deploy that has not passed it is not a deploy of the landing.
//   1. What is deployed is already merged: HEAD is on the default branch and
//      site/ has no uncommitted change. A deploy from a branch publishes work
//      nobody else has seen and that the next deploy from develop silently
//      reverts.
//   2. The download button works for a person (check-landing-download.mjs),
//      not merely an href that looks right.
// Pass the staged URL as the first argument to run the click-through against
// the deployed copy instead of the local source: predeploy-landing.mjs [url].
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repo = fileURLToPath(new URL('..', import.meta.url));
const git = (...args) => execFileSync('git', ['-C', repo, ...args], { encoding: 'utf8' }).trim();
const fail = (message) => { console.error(`predeploy-landing: ${message}`); process.exit(1); };

const head = git('rev-parse', 'HEAD');
// Merged means on the published develop: a checkout's own develop can lag it, and a worktree at a
// commit already on origin is as merged as it gets. Without the network, the local branch decides.
spawnSync('git', ['-C', repo, 'fetch', '--quiet', 'origin', 'develop']);
const onDevelop = (ref) => spawnSync('git', ['-C', repo, 'merge-base', '--is-ancestor', head, ref]).status === 0;
if (!onDevelop('origin/develop') && !onDevelop('develop')) fail(`HEAD ${head.slice(0, 9)} is not on develop. Merge it to develop first, then deploy from there.`);
const dirty = git('status', '--porcelain', '--', 'site', ':!site/.moss');
if (dirty) fail(`site/ has uncommitted changes, so what would deploy is not what develop holds:\n${dirty}`);

const check = spawnSync(process.execPath, [fileURLToPath(new URL('check-landing-download.mjs', import.meta.url)), ...process.argv.slice(2)], { stdio: 'inherit' });
if (check.status !== 0) fail('the download check failed; do not deploy.');
console.log('predeploy-landing: ok — merged to develop, download button verified.');
