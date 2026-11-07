/**
 * Render gate: the dots of a static place map are links to their place pages
 * in a real browser. Tab reaches a dot, it is named for its place, a focus
 * ring shows, its hit target is at least 24 by 24 CSS px, Enter opens the
 * place page, and the place page's own dot is not a link.
 *
 *   npx playwright test -c playwright/place-dot-links.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PLACE_DOT_LINKS_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('place-dot-links');
const serveDir = buildScratchSite(PLACE_DOT_LINKS_GATE);

export default defineGateConfig({
  gate: 'place-dot-links',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
