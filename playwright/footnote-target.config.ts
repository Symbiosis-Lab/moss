/**
 * Render gate: the footnote landing fix — `:target` highlight wash + `scroll-padding-top`
 * island clearance.
 *
 * Both claims need a real engine: whether the endnote `<li>` clears the
 * floating nav island is a live-layout question (`toBeInViewport` plus a
 * bounding-box comparison), and whether a `:target` selector actually paints
 * a non-transparent `background-color` — animated or, under reduced motion,
 * static — is a computed-style question jsdom cannot answer (no cascade, no
 * animation model).
 *
 *   npx playwright test -c playwright/footnote-target.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { FOOTNOTE_TARGET_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('footnote-target');
const serveDir = buildScratchSite(FOOTNOTE_TARGET_GATE);

export default defineGateConfig({
  gate: 'footnote-target',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
