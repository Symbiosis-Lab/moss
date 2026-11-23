/** Render gate (chromium and webkit): a theme that flattens `.nav-right` keeps its own masthead layout at every width. Needs target/debug/moss-cli; run with `npx playwright test -c playwright/nav-flattened.config.ts`. */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { NAV_FLATTENED_GATE, NAV_FLATTENED_BESIDE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('nav-flattened');
const serveDir = buildScratchSite(NAV_FLATTENED_GATE);
const besideServeDir = buildScratchSite(NAV_FLATTENED_BESIDE_GATE);

export default defineGateConfig({
  gate: 'nav-flattened',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'light',
  },
  webServer: [
    { serveDir, port: PORT },
    { serveDir: besideServeDir, port: gatePort('nav-flattened:beside') },
  ],
});
