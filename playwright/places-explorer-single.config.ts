/**
 * Render gate: a places root with exactly one work — its initial frame and
 * its resting card state.
 *
 *   npx playwright test -c playwright/places-explorer-single.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PLACES_EXPLORER_SINGLE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('places-explorer-single');
const serveDir = buildScratchSite(PLACES_EXPLORER_SINGLE_GATE);

export default defineGateConfig({
  gate: 'places-explorer-single',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
