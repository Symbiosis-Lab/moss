/**
 * Render gate: a `style:stream` listing spaces its posts by the hairline's
 * padding alone and ends on the last post's date, so the More link under it
 * sits at its own margin, as it does under a card listing.
 *
 *   npx playwright test -c playwright/stream-listing.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { STREAM_LISTING_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('stream-listing');
const serveDir = buildScratchSite(STREAM_LISTING_GATE);

export default defineGateConfig({
  gate: 'stream-listing',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
