/**
 * Render gate: grid tile styling reaches the grid's own cells only. A folder
 * listing embedded in a cell computes the same corners and overflow as the
 * same listing outside a grid, while a real tile keeps its rounded clip.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/grid-tile-scope.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { GRID_TILE_SCOPE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('grid-tile-scope');
const serveDir = buildScratchSite(GRID_TILE_SCOPE_GATE);

export default defineGateConfig({
  gate: 'grid-tile-scope',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
