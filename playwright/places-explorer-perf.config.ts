/**
 * Render gate: the places explorer stays responsive — a scripted pan never
 * drops a frame for longer than a generous bound, at rest and at a deep
 * zoom with regional tiles in view, and a real zoom-control click resolves
 * quickly — in both Chromium and WebKit. Fails clearly on the old inline,
 * filtered-SVG world/tile layers; passes with margin once they are decoded
 * raster images on a permanently promoted layer (see map.ts's own module
 * doc for the numbers this guards).
 *
 *   npx playwright test -c playwright/places-explorer-perf.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PLACES_EXPLORER_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('places-explorer-perf');
const serveDir = buildScratchSite(PLACES_EXPLORER_GATE);

export default defineGateConfig({
  gate: 'places-explorer-perf',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
