/**
 * Render gate: a theme's `--moss-card-min` decides a grid listing's columns in
 * a container under 320px too (tests/render-gates/site/card-min-token.spec.ts).
 *
 * Prereq: target/debug/moss-cli (bash scripts/build-moss-cli.sh) or MOSS_BIN.
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { CARD_MIN_TOKEN_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('card-min-token');
const serveDir = buildScratchSite(CARD_MIN_TOKEN_GATE);

export default defineGateConfig({
  gate: 'card-min-token',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
