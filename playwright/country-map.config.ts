/**
 * Render gate: a country map paints its levels along one ramp that reads as
 * order in both themes, fits a phone, and opens its panel from a tap or a
 * list row.
 *
 *   npx playwright test -c playwright/country-map.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { COUNTRY_MAP_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('country-map');
const serveDir = buildScratchSite(COUNTRY_MAP_GATE);

export default defineGateConfig({
  gate: 'country-map',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
