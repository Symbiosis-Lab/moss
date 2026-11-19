/**
 * Render gate: a link card's picture slot closes up only when its page names
 * no picture; a card not fetched yet, or whose picture did not download,
 * keeps its box, on the built page and in the preview.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/link-card-cover-slot.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { LINK_CARD_COVER_SLOT_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('link-card-cover-slot');
const serveDir = buildScratchSite(LINK_CARD_COVER_SLOT_GATE);

export default defineGateConfig({
  gate: 'link-card-cover-slot',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
