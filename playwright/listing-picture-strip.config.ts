/**
 * Render gate: a `style:summary` listing with `pictures:5` draws each row's
 * pictures as one strip: five equal columns in one row on a wide screen, a
 * sideways scroll row on a phone that never widens the page.
 *
 *   npx playwright test -c playwright/listing-picture-strip.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { LISTING_PICTURE_STRIP_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('listing-picture-strip');
const serveDir = buildScratchSite(LISTING_PICTURE_STRIP_GATE);

export default defineGateConfig({
  gate: 'listing-picture-strip',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
