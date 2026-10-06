/**
 * Render gate: a coverless `:::grid` cell beside covered siblings must read
 * as a card — the same `data-cover="quote"` upgrade the auto-generated
 * listing grid already gives a coverless card in a mixed row — instead of
 * showing the bare `.moss-card-no-cover` placeholder box.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/grid-card-no-cover.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { GRID_CARD_NO_COVER_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('grid-card-no-cover');
const serveDir = buildScratchSite(GRID_CARD_NO_COVER_GATE);

export default defineGateConfig({
  gate: 'grid-card-no-cover',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
