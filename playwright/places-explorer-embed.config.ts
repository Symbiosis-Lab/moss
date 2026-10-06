/**
 * Render gate: the places-explorer embed — a `style:map` card and an
 * article's own locator, lazily hydrated to a live map behind their static
 * poster, collapsed to cooperative gestures with an expand control and an
 * open-in-new-tab control reusing the existing immersive/fullscreen
 * mechanism.
 *
 *   npx playwright test -c playwright/places-explorer-embed.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PLACES_EXPLORER_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('places-explorer-embed');
const serveDir = buildScratchSite(PLACES_EXPLORER_GATE);

export default defineGateConfig({
  gate: 'places-explorer-embed',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
