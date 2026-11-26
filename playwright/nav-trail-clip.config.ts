/** Render gate (chromium and webkit): with JS off, a long site name over a deep trail gives way before the parent crumb is clipped under its floor. Needs target/debug/moss-cli; run with `npx playwright test -c playwright/nav-trail-clip.config.ts`. */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { NAV_TRAIL_CLIP_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('nav-trail-clip');
const serveDir = buildScratchSite(NAV_TRAIL_CLIP_GATE);

export default defineGateConfig({
  gate: 'nav-trail-clip',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'light',
  },
  webServer: [{ serveDir, port: PORT }],
});
