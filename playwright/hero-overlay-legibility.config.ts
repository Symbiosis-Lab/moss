/**
 * Render gate: an overlaid hero's text sits on a panel legible against the
 * photo beneath it, and the hero never clips the text to fit a fixed frame.
 *
 * Both halves need a real engine: the panel's contrast depends on the
 * `background` a browser actually composites (alpha blending against
 * whatever `object-fit: cover` shows through), and the clipping question is
 * "does this element's rendered box contain that one", which only a layout
 * engine answers. Built through a real `moss-cli build` (not a probe page,
 * unlike hero-tone.spec.ts) because the panel's colour is computed from the
 * busy fixture image's own scanned pixels — the thing under test.
 *
 *   npx playwright test -c playwright/hero-overlay-legibility.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli
 * (or MOSS_BIN), and `ffmpeg` on PATH to generate the busy fixture image.
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { HERO_OVERLAY_LEGIBILITY_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('hero-overlay-legibility');
const serveDir = buildScratchSite(HERO_OVERLAY_LEGIBILITY_GATE);

export default defineGateConfig({
  gate: 'hero-overlay-legibility',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
