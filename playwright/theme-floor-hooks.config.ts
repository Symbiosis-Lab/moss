/**
 * Render gate: a theme that sets only tokens gets a coloured header band with
 * its own link colour (and still a transparent header over a `header=over`
 * hero), a hero-only home with no blank article between hero and footer, and
 * plain cards with readable text.
 *
 *   npx playwright test -c playwright/theme-floor-hooks.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { THEME_FLOOR_HOOKS_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('theme-floor-hooks');
const serveDir = buildScratchSite(THEME_FLOOR_HOOKS_GATE);

export default defineGateConfig({
  gate: 'theme-floor-hooks',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
