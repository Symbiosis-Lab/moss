/**
 * Render gate for `chrome: none`: where the controls stand on a page drawn
 * without the site's frame, in chromium and webkit, on a site the moss CLI
 * built.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 *
 *   npx playwright test -c playwright/page-chrome-none.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PAGE_CHROME_NONE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('page-chrome-none');

export default defineGateConfig({
  gate: 'page-chrome-none',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir: buildScratchSite(PAGE_CHROME_NONE_GATE), port: PORT },
});
