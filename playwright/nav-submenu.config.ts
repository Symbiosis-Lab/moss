/**
 * Render gate for the header submenu: opens by keyboard and click, closes on
 * Escape and outside click, and the open panel stays inside the viewport on
 * both edges, with a CJK parent label, at desktop, 390px and 321px.
 *
 *   npx playwright test -c playwright/nav-submenu.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { NAV_SUBMENU_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('nav-submenu');
const serveDir = buildScratchSite(NAV_SUBMENU_GATE);

export default defineGateConfig({
  gate: 'nav-submenu',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
