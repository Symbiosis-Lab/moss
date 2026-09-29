/**
 * Render gate: the place-map palette (relief/seafloor ramps, water, coast,
 * marker...) is defined in site.css, not just referenced by the SVG
 * emitter's `var(--moss-place-*, #fallback)` call sites.
 *
 * A Rust snapshot test proves every token the emitter can produce has a
 * matching declaration in site.css; it reads the stylesheet as text, so it
 * cannot prove the CASCADE actually resolves one to the approved colour
 * rather than silently falling through to the Rust-side fallback hex. That
 * needs a real browser.
 *
 *   npx playwright test -c playwright/place-map-tokens.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PLACE_MAP_TOKENS_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('place-map-tokens');
const serveDir = buildScratchSite(PLACE_MAP_TOKENS_GATE);

export default defineGateConfig({
  gate: 'place-map-tokens',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
