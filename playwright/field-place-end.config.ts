/**
 * Render gate: a `page` field placed at `end` closes the article with a card
 * for the page it links to: after the body, its pictures painted, and never a
 * sideways page scroll on a phone.
 *
 *   npx playwright test -c playwright/field-place-end.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { FIELD_PLACE_END_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('field-place-end');
const serveDir = buildScratchSite(FIELD_PLACE_END_GATE);

export default defineGateConfig({
  gate: 'field-place-end',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
