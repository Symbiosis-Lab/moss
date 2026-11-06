/**
 * Render gate: a map drawn over a historical layer that does not fill its
 * frame takes the layer's shape on a place page and in a locator, while the
 * places root still fills the window.
 *
 *   npx playwright test -c playwright/place-map-cut.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PLACE_MAP_CUT_GATE } from '../tests/e2e/helpers/place-map-cut-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('place-map-cut');
const serveDir = buildScratchSite(PLACE_MAP_CUT_GATE);

export default defineGateConfig({
  gate: 'place-map-cut',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
