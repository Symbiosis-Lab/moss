/**
 * Render gate: a grid card's image fills the card's inline size, in both
 * writing modes. See the spec header for the full story.
 *
 *   npx playwright test -c playwright/grid-card-image-inline-size.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { GRID_CARD_IMAGE_INLINE_SIZE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('grid-card-image-inline-size');
const serveDir = buildScratchSite(GRID_CARD_IMAGE_INLINE_SIZE_GATE);

export default defineGateConfig({
  gate: 'grid-card-image-inline-size',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
