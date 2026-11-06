/**
 * Render gate: in a hero's words a link inside a sentence is a text link and
 * a paragraph of nothing but links is a row of buttons.
 *
 *   npx playwright test -c playwright/hero-links.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { HERO_LINKS_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('hero-links');
const serveDir = buildScratchSite(HERO_LINKS_GATE);

export default defineGateConfig({
  gate: 'hero-links',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
