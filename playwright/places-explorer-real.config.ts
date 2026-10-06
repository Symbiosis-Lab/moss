/**
 * Render gate: a realistic places root — a synthetic root, long multi-link
 * markdown bylines, a work-less middle place and a zh-Hant page, the shapes
 * PLACES_EXPLORER_GATE's short English fixture never exercises. See
 * PLACES_EXPLORER_REAL_GATE's own doc (tests/e2e/helpers/gate-sites.ts) for
 * why these four live in one fixture rather than four.
 *
 *   npx playwright test -c playwright/places-explorer-real.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PLACES_EXPLORER_REAL_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('places-explorer-real');
const serveDir = buildScratchSite(PLACES_EXPLORER_REAL_GATE);

export default defineGateConfig({
  gate: 'places-explorer-real',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
