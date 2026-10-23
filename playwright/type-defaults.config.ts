/**
 * Render gate: six defaults a hand-built design pinned down. See the spec header.
 *
 *   bash scripts/render-gates.sh type-defaults
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { TYPE_DEFAULTS_GATE } from '../tests/e2e/helpers/type-defaults-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('type-defaults');
const serveDir = buildScratchSite(TYPE_DEFAULTS_GATE);

export default defineGateConfig({
  gate: 'type-defaults',
  engines: ['chromium', 'webkit'],
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
