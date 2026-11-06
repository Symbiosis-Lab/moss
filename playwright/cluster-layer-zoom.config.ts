/**
 * Render gate: clicking a cluster of the places explorer over a historical
 * layer zooms in to its members and never out of the layer's view.
 *
 *   npx playwright test -c playwright/cluster-layer-zoom.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { CLUSTER_LAYER_ZOOM_GATE } from '../tests/e2e/helpers/cluster-layer-zoom-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('cluster-layer-zoom');
const serveDir = buildScratchSite(CLUSTER_LAYER_ZOOM_GATE);

export default defineGateConfig({
  gate: 'cluster-layer-zoom',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
