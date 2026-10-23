/**
 * Render gate for the contents ruler that opens like a wheel around the
 * pointer on a page with `contents: pictures`.
 *
 * Runs tests/render-gates/site/contents-lens.spec.ts in BOTH chromium and
 * webkit against a scratch site built by the moss CLI: which strip is under
 * the pointer, where the opened rows are drawn and how big their pictures are
 * are all painted layout.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/contents-lens.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { CONTENTS_LENS_GATE } from '../tests/e2e/helpers/contents-lens-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('contents-lens');

export default defineGateConfig({
  gate: 'contents-lens',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir: buildScratchSite(CONTENTS_LENS_GATE), port: PORT },
});
