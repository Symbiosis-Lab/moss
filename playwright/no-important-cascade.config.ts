/**
 * Render gate for Task 2.2: user CSS wins without !important.
 *
 * Runs tests/render-gates/cascade/no-important-cascade.spec.ts in BOTH chromium
 * and webkit against the gate-test.html probe the fixture writes into the built
 * output. The spec asserts that at 390px the user's two-track grid rule and the
 * user's .read-more colour both win — with no !important anywhere.
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
 *   npx playwright test -c playwright/no-important-cascade.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { NO_IMPORTANT_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';

const PORT = 8792;
const serveDir = buildScratchSite(NO_IMPORTANT_GATE);

export default defineGateConfig({
  gate: 'no-important-cascade',
  testDir: 'cascade',
  outputDir: '../target/test-tmp/playwright-no-important-gate',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'light', // pin light so dark-mode :root vars do not interfere
  },
  webServer: { serveDir, port: PORT },
});
