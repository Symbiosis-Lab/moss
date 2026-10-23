/**
 * Playwright config for the facts-margin gate
 * (tests/render-gates/site/facts-margin.spec.ts).
 *
 * Two scratch sites built with MOSS_BIN: one whose theme widens the content
 * column, one left at the default.
 *
 * Run via: bash scripts/render-gates.sh facts-margin
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { FACTS_MARGIN_DEFAULT, FACTS_MARGIN_WIDENED } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const widenedServeDir = buildScratchSite(FACTS_MARGIN_WIDENED);
const defaultServeDir = buildScratchSite(FACTS_MARGIN_DEFAULT);

export default defineGateConfig({
  gate: 'facts-margin',
  use: { colorScheme: 'light' },
  webServer: [
    { serveDir: widenedServeDir, port: gatePort('facts-margin:widened') },
    { serveDir: defaultServeDir, port: gatePort('facts-margin:default') },
  ],
});
