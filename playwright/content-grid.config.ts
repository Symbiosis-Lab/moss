/**
 * Render gate: a full-width band's text and a closing picture's words start
 * where the page's text column starts, at desktop and phone width, with the
 * default content width and with `content_width: full`.
 *
 *   npx playwright test -c playwright/content-grid.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { CENTER_ALIGN_GATE, DEFAULT_WIDTH_GATE, END_ALIGN_GATE, FULL_WIDTH_GATE } from '../tests/e2e/helpers/content-grid-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

export default defineGateConfig({
  gate: 'content-grid',
  use: { baseURL: `http://localhost:${gatePort('content-grid:default')}/` },
  webServer: [
    { serveDir: buildScratchSite(DEFAULT_WIDTH_GATE), port: gatePort('content-grid:default') },
    { serveDir: buildScratchSite(FULL_WIDTH_GATE), port: gatePort('content-grid:full') },
    { serveDir: buildScratchSite(END_ALIGN_GATE), port: gatePort('content-grid:end') },
    { serveDir: buildScratchSite(CENTER_ALIGN_GATE), port: gatePort('content-grid:center') },
  ],
});
