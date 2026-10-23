/**
 * Render gate: a footer.md that ends in a `:::hero` closes the page with an
 * edge-to-edge picture that carries its words, in both themes at desktop and
 * phone width.
 *
 * Where the picture sits, how far it reaches and what is below it are layout
 * questions only an engine answers.
 *
 *   npx playwright test -c playwright/closing-picture.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { CLOSING_PICTURE_ARCHIVAL_CAPTION_GATE, CLOSING_PICTURE_ARCHIVAL_GATE, CLOSING_PICTURE_BUSY_FADE_GATE, CLOSING_PICTURE_BUSY_GATE, CLOSING_PICTURE_CAPPED_GATE, CLOSING_PICTURE_FADE_GATE, CLOSING_PICTURE_GATE, CLOSING_PICTURE_LANDSCAPE_GATE, CLOSING_PICTURE_PLAIN_GATE, CLOSING_PICTURE_SKY_GATE } from '../tests/e2e/helpers/closing-picture-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('closing-picture');
const serveDir = buildScratchSite(CLOSING_PICTURE_GATE);
const busyServeDir = buildScratchSite(CLOSING_PICTURE_BUSY_GATE);
const skyServeDir = buildScratchSite(CLOSING_PICTURE_SKY_GATE);
const fadeServeDir = buildScratchSite(CLOSING_PICTURE_FADE_GATE);
const busyFadeServeDir = buildScratchSite(CLOSING_PICTURE_BUSY_FADE_GATE);
const plainServeDir = buildScratchSite(CLOSING_PICTURE_PLAIN_GATE);
const cappedServeDir = buildScratchSite(CLOSING_PICTURE_CAPPED_GATE);
const landscapeServeDir = buildScratchSite(CLOSING_PICTURE_LANDSCAPE_GATE);
const archivalServeDir = buildScratchSite(CLOSING_PICTURE_ARCHIVAL_GATE);
const archivalCaptionServeDir = buildScratchSite(CLOSING_PICTURE_ARCHIVAL_CAPTION_GATE);

export default defineGateConfig({
  gate: 'closing-picture',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: [
    { serveDir, port: PORT },
    { serveDir: busyServeDir, port: gatePort('closing-picture:busy') },
    { serveDir: skyServeDir, port: gatePort('closing-picture:sky') },
    { serveDir: fadeServeDir, port: gatePort('closing-picture:fade') },
    { serveDir: busyFadeServeDir, port: gatePort('closing-picture:busy-fade') },
    { serveDir: plainServeDir, port: gatePort('closing-picture:plain') },
    { serveDir: cappedServeDir, port: gatePort('closing-picture:capped') },
    { serveDir: landscapeServeDir, port: gatePort('closing-picture:landscape') },
    { serveDir: archivalServeDir, port: gatePort('closing-picture:archival') },
    { serveDir: archivalCaptionServeDir, port: gatePort('closing-picture:archival-caption') },
  ],
});
