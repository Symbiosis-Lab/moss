/**
 * Render gate for Task 2.4: dark tokens are beaten by layer order.
 *
 * Runs tests/render-gates/cascade/dark-layer-order.spec.ts in BOTH chromium and
 * webkit. The author's [data-theme="dark"] override and moss's tokens-layer dark
 * block have identical specificity, so only layer order can produce the author
 * win the spec asserts (background rgb(17, 17, 17)).
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
 *   npx playwright test -c playwright/dark-layer-order.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { DARK_LAYER_ORDER_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('dark-layer-order');
const serveDir = buildScratchSite(DARK_LAYER_ORDER_GATE);

export default defineGateConfig({
  gate: 'dark-layer-order',
  testDir: 'cascade',
  outputDir: '../target/test-tmp/playwright-dark-layer-order-gate',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'dark',
    // The spec ALSO sets localStorage["moss-theme"]="dark" (the explicit
    // toggle), which the pre-paint script prefers. Matching colorScheme here
    // just keeps the system preference from fighting the assertion.
  },
  webServer: { serveDir, port: PORT },
});
