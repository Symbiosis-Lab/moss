/**
 * Render gate: a footer.md that ends in a `:::hero` closes the page with an
 * edge-to-edge picture that carries its words, in both themes at desktop and
 * phone width.
 *
 * Where the picture sits, how far it reaches and what is below it are layout
 * questions only an engine answers.
 *
 *   npx playwright test -c playwright/closing-picture.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { CLOSING_PICTURE_GATE } from '../tests/e2e/helpers/closing-picture-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('closing-picture');
const serveDir = buildScratchSite(CLOSING_PICTURE_GATE);

export default defineGateConfig({
  gate: 'closing-picture',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
