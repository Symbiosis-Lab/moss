/**
 * Render gate: `:::grid {collage}` lays its tiles out inside one box without
 * overlap, shows credits on hover and in the picture viewer, and stacks
 * compactly on a phone.
 *
 *   npx playwright test -c playwright/collage.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { COLLAGE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('collage');
const serveDir = buildScratchSite(COLLAGE_GATE);

export default defineGateConfig({
  gate: 'collage',
  engines: ['chromium', 'webkit'],
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
