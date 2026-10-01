/**
 * Render gate: the coincident-cluster ring — a cluster proven unable to
 * ever separate by zooming blooms into one real button per member, dims
 * every other marker, scopes the card row to its members, and closes on
 * Escape or an outside click.
 *
 *   npx playwright test -c playwright/places-explorer-ring.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PLACES_EXPLORER_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('places-explorer-ring');
const serveDir = buildScratchSite(PLACES_EXPLORER_GATE);

export default defineGateConfig({
  gate: 'places-explorer-ring',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
