/**
 * Render gate: a `children_style: lead` listing paints its first card's
 * picture (never 0 px wide, as WebKit did for a grid item with an aspect
 * ratio), hangs the lead's words from the picture's top edge on a wide screen,
 * and never overflows sideways.
 *
 *   npx playwright test -c playwright/lead-listing.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { LEAD_LISTING_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('lead-listing');
const serveDir = buildScratchSite(LEAD_LISTING_GATE);

export default defineGateConfig({
  gate: 'lead-listing',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
