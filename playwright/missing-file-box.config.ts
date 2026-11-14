/**
 * Render gate: a picture or media file the page names that is not in the
 * folder is drawn as a box naming it, in both engines and both colour schemes.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/missing-file-box.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { MISSING_FILE_BOX_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('missing-file-box');
const serveDir = buildScratchSite(MISSING_FILE_BOX_GATE);

export default defineGateConfig({
  gate: 'missing-file-box',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
