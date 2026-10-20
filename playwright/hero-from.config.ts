/**
 * Render gate: a hero drawn from an event shows once, reads as moss's own
 * event line and call to action, and gives way to its fallback after the end.
 *
 * Whether a fallback is painted, and whether a label shares a line with a
 * date, only exist once an engine lays the page out.
 *
 *   npx playwright test -c playwright/hero-from.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { HERO_FROM_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('hero-from');
const serveDir = buildScratchSite(HERO_FROM_GATE);

export default defineGateConfig({
  gate: 'hero-from',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
