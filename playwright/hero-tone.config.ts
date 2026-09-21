/**
 * Render gate: a pale hero drops the scrim and flips its text dark, and the
 * three hero layouts stay told apart.
 *
 * Engine questions throughout. `content: none` vs `content: ""` on a
 * `::before` is only observable once a browser has resolved the cascade, and
 * three of the four rules under test win their tie on source order rather than
 * specificity — jsdom implements neither.
 *
 *   npx playwright test -c playwright/hero-tone.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { HERO_TONE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';

const PORT = 8757;
const serveDir = buildScratchSite(HERO_TONE_GATE);

export default defineGateConfig({
  gate: 'hero-tone',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
