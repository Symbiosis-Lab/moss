/**
 * Render gate: a hero caption is readable below the image, and a captioned
 * hero shows its whole subject.
 *
 * Both are engine questions. The caption sits next to a section with
 * `overflow: hidden` and a height cap, so "is it visible and below the photo"
 * cannot be answered from emitted text; and the crop is `object-fit`, which
 * only exists once something lays the image out.
 *
 *   npx playwright test -c playwright/hero-caption.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { HERO_CAPTION_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';

const PORT = 8751;
const serveDir = buildScratchSite(HERO_CAPTION_GATE);

export default defineGateConfig({
  gate: 'hero-caption',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
