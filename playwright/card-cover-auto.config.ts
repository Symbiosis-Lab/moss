/**
 * Render gate: `--moss-card-cover-ratio: auto` gives each card cover its own
 * image's shape, and a fixed ratio still crops every cover to one box.
 *
 *   npx playwright test -c playwright/card-cover-auto.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { CARD_COVER_AUTO_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('card-cover-auto');
const serveDir = buildScratchSite(CARD_COVER_AUTO_GATE);

export default defineGateConfig({
  gate: 'card-cover-auto',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
