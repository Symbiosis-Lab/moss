/** Render gate (chromium and webkit): a theme that holds the masthead to one fixed-height row (`flex-wrap: nowrap`) keeps its links inside the header at every width, with no sideways scroll. Needs target/debug/moss-cli; run with `npx playwright test -c playwright/nav-nowrap.config.ts`. */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { NAV_NOWRAP_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('nav-nowrap');
const serveDir = buildScratchSite(NAV_NOWRAP_GATE);

export default defineGateConfig({
  gate: 'nav-nowrap',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'light',
  },
  webServer: { serveDir, port: PORT },
});
