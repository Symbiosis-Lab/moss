/**
 * Render gate for the contents ruler and the island's section name.
 *
 * Runs tests/render-gates/site/contents-ruler.spec.ts in BOTH chromium and
 * webkit against a scratch site built by the moss CLI. Where the ruler sits,
 * whether its labels clear the text, whether a label is clipped, which control
 * is on screen and what a phone's bar does are all questions about painted
 * layout; jsdom answers none of them.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/contents-ruler.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { CONTENTS_RULER_GATE } from '../tests/e2e/helpers/contents-ruler-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('contents-ruler');
const serveDir = buildScratchSite(CONTENTS_RULER_GATE);

export default defineGateConfig({
  gate: 'contents-ruler',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
