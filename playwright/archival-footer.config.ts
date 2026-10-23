/**
 * Render gate: an archival site's footer keeps the article's column, its
 * edition block fills that column, and the moss mark closes the edition line
 * in both its inks, at desktop and phone width.
 *
 *   npx playwright test -c playwright/archival-footer.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { ARCHIVAL_FOOTER_GATE } from '../tests/e2e/helpers/archival-footer-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('archival-footer');
const serveDir = buildScratchSite(ARCHIVAL_FOOTER_GATE);

export default defineGateConfig({
  gate: 'archival-footer',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
