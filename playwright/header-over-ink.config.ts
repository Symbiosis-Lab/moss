/**
 * Render gate: the site header laid over a hero picture (`header=over`) reads
 * at WCAG AA whatever the picture. Built through a real `moss-cli build`
 * because the ink is chosen from the picture's own pixels.
 *
 *   npx playwright test -c playwright/header-over-ink.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { HEADER_OVER_INK_GATE } from '../tests/e2e/helpers/header-over-ink-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('header-over-ink');

export default defineGateConfig({
  gate: 'header-over-ink',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir: buildScratchSite(HEADER_OVER_INK_GATE), port: PORT },
});
