/**
 * Render gate: bold text and headings in a family the theme hosts as Regular
 * and Bold only.
 *
 * Runs tests/render-gates/cascade/hosted-bold.spec.ts in BOTH chromium and
 * webkit. Which face a weight lands on is the engine's font matching, which
 * no text-level test can see.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/hosted-bold.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { HOSTED_BOLD_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('hosted-bold');
const serveDir = buildScratchSite(HOSTED_BOLD_GATE);

export default defineGateConfig({
  gate: 'hosted-bold',
  testDir: 'cascade',
  outputDir: '../target/test-tmp/playwright-hosted-bold-gate',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
