/**
 * Render gate: the places explorer under vertical typesetting — the map
 * stays a horizontal widget, on its own page and inside its embed.
 *
 *   npx playwright test -c playwright/places-explorer-vertical.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PLACES_EXPLORER_VERTICAL_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('places-explorer-vertical');
const serveDir = buildScratchSite(PLACES_EXPLORER_VERTICAL_GATE);

export default defineGateConfig({
  gate: 'places-explorer-vertical',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
