/**
 * Render gate for folded callouts: `::: {accordion}` opens them one at a time,
 * and every folded callout's answer animates open where the engine supports it.
 *
 * Runs tests/render-gates/site/folded-callouts.spec.ts in BOTH chromium and
 * webkit against a scratch site built by the moss CLI.
 *
 *   npx playwright test -c playwright/folded-callouts.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { FOLDED_CALLOUTS_GATE } from '../tests/e2e/helpers/folded-callouts-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('folded-callouts');

export default defineGateConfig({
  gate: 'folded-callouts',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: [{ serveDir: buildScratchSite(FOLDED_CALLOUTS_GATE), port: PORT }],
});
