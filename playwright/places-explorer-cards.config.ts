/**
 * Render gate: the works row — selection expands a card in place, and the
 * row follows the camera's own view.
 *
 *   npx playwright test -c playwright/places-explorer-cards.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PLACES_EXPLORER_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('places-explorer-cards');
const serveDir = buildScratchSite(PLACES_EXPLORER_GATE);

export default defineGateConfig({
  gate: 'places-explorer-cards',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
