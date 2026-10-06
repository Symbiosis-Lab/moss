/**
 * Render gate for the native feature CSS @layer cascade contract.
 *
 * Runs tests/render-gates/cascade/comments-cascade.spec.ts in BOTH chromium and
 * webkit. The spec asserts:
 *   1. .comment-author computed color == rgb(0, 128, 0)
 *      (the user's @layer themes rule beats comments.css in @layer plugins)
 *   2. the .moss-comments element still renders after the layer wrapping
 *
 * These assertions are only provable by a real browser render — jsdom is blind
 * to @layer cascade precedence.
 *
 * The scratch site is scaffolded and built by `buildScratchSite` below, at
 * config-parse time. That is deliberate and not a `globalSetup`: playwright
 * starts `webServer` BEFORE `globalSetup`, so a site built in `globalSetup`
 * does not exist yet when `webServer.cwd` is needed. See
 * tests/e2e/helpers/scratch-site.ts.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/comments-cascade.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { COMMENTS_CASCADE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('comments-cascade');
const serveDir = buildScratchSite(COMMENTS_CASCADE_GATE);

export default defineGateConfig({
  gate: 'comments-cascade',
  testDir: 'cascade',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
