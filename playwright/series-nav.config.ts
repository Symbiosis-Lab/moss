/**
 * Render gate: the series nav's direction mark sits in its label's line, with
 * a word's gap, on the first, middle and last page of a series
 * (tests/render-gates/site/series-nav.spec.ts).
 *
 * Prereq: target/debug/moss-cli (bash scripts/build-moss-cli.sh) or MOSS_BIN.
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { SERIES_NAV_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('series-nav');
const serveDir = buildScratchSite(SERIES_NAV_GATE);

export default defineGateConfig({
  gate: 'series-nav',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
