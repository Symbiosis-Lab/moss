/**
 * Render gate for `:::form`: layout at phone and desktop width, a visible focus
 * ring, the invalid state and the honeypot, in light and dark.
 *
 *   npx playwright test -c playwright/request-form.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { REQUEST_FORM } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

export default defineGateConfig({
  gate: 'request-form',
  webServer: [{ serveDir: buildScratchSite(REQUEST_FORM), port: gatePort('request-form') }],
});
