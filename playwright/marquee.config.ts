/**
 * Render gate: a picture strip that slides sideways in a loop
 * (`motion:marquee`, `:::gallery marquee`).
 *
 *   npx playwright test -c playwright/marquee.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { MARQUEE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('marquee');
const serveDir = buildScratchSite(MARQUEE_GATE);

export default defineGateConfig({
  gate: 'marquee',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
