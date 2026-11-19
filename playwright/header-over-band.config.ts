/**
 * Render gate: the site header laid over a full-width band that opens the page
 * (`::: {screen header=over}`) sits on the band's pictures and stays legible,
 * and lets the pointer through to the band's links.
 *
 *   npx playwright test -c playwright/header-over-band.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { HEADER_OVER_BAND_GATE } from '../tests/e2e/helpers/header-over-band-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('header-over-band');

export default defineGateConfig({
  gate: 'header-over-band',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir: buildScratchSite(HEADER_OVER_BAND_GATE), port: PORT },
});
