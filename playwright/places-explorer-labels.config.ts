/**
 * Render gate: the places explorer's label layer — major cities, mountain
 * ranges, peaks and rivers, placed greedily by priority, never overlapping
 * another label, a marker, the controls or the card row, within the area
 * budget, and respecting the own-place rule for a city label beside a
 * marker.
 *
 *   npx playwright test -c playwright/places-explorer-labels.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PLACES_EXPLORER_LABELS_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('places-explorer-labels');
const serveDir = buildScratchSite(PLACES_EXPLORER_LABELS_GATE);

export default defineGateConfig({
  gate: 'places-explorer-labels',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
