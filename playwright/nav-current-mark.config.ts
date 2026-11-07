/** Render gate (chromium and webkit): the current page's nav link shares its siblings' text line. Needs target/debug/moss-cli; run with `npx playwright test -c playwright/nav-current-mark.config.ts`. */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { NAV_CURRENT_MARK_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('nav-current-mark');
const serveDir = buildScratchSite(NAV_CURRENT_MARK_GATE);

export default defineGateConfig({
  gate: 'nav-current-mark',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'light',
  },
  webServer: { serveDir, port: PORT },
});
