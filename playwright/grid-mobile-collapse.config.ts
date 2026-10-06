/**
 * Render gate: `:::grid` collapses to one column on mobile, ratio grids
 * included, and a grid cell presents the same image box whichever syntax the
 * author used.
 *
 * Only a real engine can answer either question. A ratio grid used to carry an
 * inline `style="grid-template-columns:…"`, which no stylesheet rule can
 * override — the markup was "correct" by every text assertion and the page
 * still showed two columns on a phone.
 *
 *   npx playwright test -c playwright/grid-mobile-collapse.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { GRID_MOBILE_COLLAPSE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('grid-mobile-collapse');
const serveDir = buildScratchSite(GRID_MOBILE_COLLAPSE_GATE);

export default defineGateConfig({
  gate: 'grid-mobile-collapse',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
