/**
 * Render gate: a rotating hero can be paused, and the control works where a
 * reader uses it (still focused, on a phone under the overlay panel).
 *
 *   npx playwright test -c playwright/hero-pause.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { HERO_PAUSE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('hero-pause');
const serveDir = buildScratchSite(HERO_PAUSE_GATE);

export default defineGateConfig({
  gate: 'hero-pause',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
