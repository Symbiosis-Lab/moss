/**
 * Render gate for the monotonic reading scale.
 *
 * Runs tests/render-gates/site/reading-scale-order.spec.ts in BOTH chromium and
 * webkit. The sizes under test are `calc()` chains over custom properties that
 * a media query, a `lang` attribute and a reader-set class all feed — only an
 * engine resolves that, and webkit is additionally the engine moss's own
 * preview runs.
 *
 * The scratch site is scaffolded and built by `buildScratchSite` at
 * config-parse time (see tests/e2e/helpers/scratch-site.ts for why that is not
 * a `globalSetup`).
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/reading-scale-order.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { READING_SCALE_ORDER_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('reading-scale-order');
const serveDir = buildScratchSite(READING_SCALE_ORDER_GATE);

export default defineGateConfig({
  gate: 'reading-scale-order',
  outputDir: '../target/test-tmp/playwright-reading-scale-order-gate',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
