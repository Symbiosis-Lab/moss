/**
 * Render gate: the article locator's top edge tracks the top of the article
 * body's first block, whether that block is a heading or a paragraph, and
 * the block flows beside the float rather than dropping under it.
 *
 *   npx playwright test -c playwright/place-map-align.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PLACE_MAP_ALIGN_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('place-map-align');
const serveDir = buildScratchSite(PLACE_MAP_ALIGN_GATE);

export default defineGateConfig({
  gate: 'place-map-align',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
