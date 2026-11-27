/** Render gate (chromium and webkit): a page with a stylesheet of its own keeps the flex masthead that stylesheet was written against, never the plain name's float. Needs target/debug/moss-cli; run with `npx playwright test -c playwright/nav-themed.config.ts`. */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { NAV_THEMED_COMPACT_GATE, NAV_THEMED_MENU_GATE, NAV_THEMED_SPLIT_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('nav-themed');
const serveDir = buildScratchSite(NAV_THEMED_MENU_GATE);
const splitServeDir = buildScratchSite(NAV_THEMED_SPLIT_GATE);
const compactServeDir = buildScratchSite(NAV_THEMED_COMPACT_GATE);

export default defineGateConfig({
  gate: 'nav-themed',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'light',
  },
  webServer: [
    { serveDir, port: PORT },
    { serveDir: splitServeDir, port: gatePort('nav-themed:split') },
    { serveDir: compactServeDir, port: gatePort('nav-themed:compact') },
  ],
});
