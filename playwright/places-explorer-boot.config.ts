/**
 * Render gate: the places explorer's progressive enhancement — the static
 * figure is replaced by the interactive layer once both fetches resolve,
 * stays static with no JavaScript, and stays static when a fetch fails.
 *
 *   npx playwright test -c playwright/places-explorer-boot.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PLACES_EXPLORER_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('places-explorer-boot');
const serveDir = buildScratchSite(PLACES_EXPLORER_GATE);

export default defineGateConfig({
  gate: 'places-explorer-boot',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
