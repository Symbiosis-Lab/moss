/**
 * Render gate: `.moss-card-cover`'s default aspect-ratio is landscape (4/3),
 * not the phone-only portrait value (3/4) that leaked into the unscoped base
 * rule at a9db42542d6 (2026-09-15). See the spec header for the full story.
 *
 *   npx playwright test -c playwright/card-cover-ratio.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { GRID_MOBILE_COLLAPSE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';

const PORT = 8798;
const serveDir = buildScratchSite(GRID_MOBILE_COLLAPSE_GATE);

export default defineGateConfig({
  gate: 'card-cover-ratio',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
