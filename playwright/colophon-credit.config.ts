/**
 * Render gate for `[site].credit`: the line before moss's mark wraps and never
 * widens the page, and the mark sits on the text's baseline.
 *
 *   npx playwright test -c playwright/colophon-credit.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { COLOPHON_CREDIT_LONG, COLOPHON_CREDIT_SHORT } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

export default defineGateConfig({
  gate: 'colophon-credit',
  use: { colorScheme: 'light' },
  webServer: [
    { serveDir: buildScratchSite(COLOPHON_CREDIT_LONG), port: gatePort('colophon-credit:long') },
    { serveDir: buildScratchSite(COLOPHON_CREDIT_SHORT), port: gatePort('colophon-credit:short') },
  ],
});
