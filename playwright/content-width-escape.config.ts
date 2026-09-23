/**
 * Playwright config for the content-width escape gate
 * (tests/render-gates/site/content-width-escape.spec.ts).
 *
 * Builds a scratch site with MOSS_BIN, because two of its claims are about
 * what the renderer emits (a percent replaces the width token) as much as
 * about site.css and vertical.css. Both engines: the old escape's
 * paint-only offset landed differently in each.
 *
 * Run via: bash scripts/render-gates.sh content-width-escape
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { CONTENT_WIDTH_ESCAPE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';

const PORT = 8813;
const serveDir = buildScratchSite(CONTENT_WIDTH_ESCAPE_GATE);

export default defineGateConfig({
  gate: 'content-width-escape',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light', reducedMotion: 'reduce' },
  webServer: { serveDir, port: PORT },
});
