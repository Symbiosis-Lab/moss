/** Render gate (chromium and webkit): a short link set and a long site name leave the toggles on the links' row. Needs target/debug/moss-cli; run with `npx playwright test -c playwright/nav-one-row.config.ts`. */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { NAV_ONE_ROW_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('nav-one-row');
const serveDir = buildScratchSite(NAV_ONE_ROW_GATE);

export default defineGateConfig({
  gate: 'nav-one-row',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'light',
  },
  webServer: { serveDir, port: PORT },
});
