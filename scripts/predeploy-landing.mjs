#!/usr/bin/env node
// Run this before every `moss-cli deploy` of the landing page, staging or
// production; a deploy that has not passed it is not a deploy of the landing.
//   1. What is deployed is already merged: HEAD is on the default branch and
//      site/ has no uncommitted change. A deploy from a branch publishes work
//      nobody else has seen and that the next deploy from develop silently
//      reverts.
//   2. The download button works for a person (check-landing-download.mjs),
//      not merely an href that looks right.
//   3. The sign-up's copies of moss's subscribe script and state stylesheet
//      match their source (build-backend-scripts.mjs --check), and every state
//      of the form looks right (check-landing-subscription.mjs).
//   4. The mark at the top of every page shows against its ground in the
//      light and the dark theme, whatever the OS setting (check-landing-logo.mjs).
// Pass the staged URL as the first argument to run the click-through against
// the deployed copy instead of the local source: predeploy-landing.mjs [url].
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { loadPlaywright } from './landing-harness.mjs';

const repo = fileURLToPath(new URL('..', import.meta.url));
const git = (...args) => execFileSync('git', ['-C', repo, ...args], { encoding: 'utf8' }).trim();
const fail = (message) => { console.error(`predeploy-landing: ${message}`); process.exit(1); };

await loadPlaywright().catch(error => fail(error.message));

const head = git('rev-parse', 'HEAD');
// Merged means on the published develop: a checkout's own develop can lag it, and a worktree at a
// commit already on origin is as merged as it gets. Without the network, the local branch decides.
spawnSync('git', ['-C', repo, 'fetch', '--quiet', 'origin', 'develop']);
const onDevelop = (ref) => spawnSync('git', ['-C', repo, 'merge-base', '--is-ancestor', head, ref]).status === 0;
if (!onDevelop('origin/develop') && !onDevelop('develop')) fail(`HEAD ${head.slice(0, 9)} is not on develop. Merge it to develop first, then deploy from there.`);
const dirty = git('status', '--porcelain', '--', 'site', ':!site/.moss');
if (dirty) fail(`site/ has uncommitted changes, so what would deploy is not what develop holds:\n${dirty}`);

const run = (script, ...args) => spawnSync(process.execPath, [fileURLToPath(new URL(script, import.meta.url)), ...args], { stdio: 'inherit', cwd: repo }).status === 0;
if (!run('build-backend-scripts.mjs', '--check')) fail('site/home/subscribe.js or subscribe-form.css is stale; run node scripts/build-backend-scripts.mjs and commit.');
if (!run('check-landing-download.mjs', ...process.argv.slice(2))) fail('the download check failed; do not deploy.');
if (!run('check-landing-subscription.mjs', ...process.argv.slice(2))) fail('the sign-up check failed; do not deploy.');
if (!run('check-landing-logo.mjs', ...process.argv.slice(2))) fail('the mark at the top of a page does not stand out from its ground in some theme; do not deploy.');
console.log('predeploy-landing: ok — merged to develop, copies of moss current, download button, sign-up states and the mark in both themes verified.');
