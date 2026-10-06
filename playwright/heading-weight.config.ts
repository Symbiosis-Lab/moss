/**
 * Render gate for the --moss-font-heading-weight token.
 *
 * Runs tests/render-gates/cascade/heading-weight.spec.ts in BOTH chromium and
 * webkit, asserting that headings compute to the fixture's 440 — a value no
 * hardcoded default uses, so a match proves the token drives the property.
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
 *   npx playwright test -c playwright/heading-weight.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { HEADING_WEIGHT_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('heading-weight');
const serveDir = buildScratchSite(HEADING_WEIGHT_GATE);

export default defineGateConfig({
  gate: 'heading-weight',
  testDir: 'cascade',
  outputDir: '../target/test-tmp/playwright-heading-weight-gate',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
