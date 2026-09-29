/**
 * Render gate: the preview server's own injections (`PREVIEW_CHEAP_REFLOW_STYLE`
 * and everything else `inject_preview_assets` adds — see iframe_bridge.rs) must
 * never change a page's LAYOUT relative to the static build. See the spec
 * header for the full story and the class of bug this guards.
 *
 * Two servers, both built from the SAME source fixture
 * (tests/e2e/helpers/gate-sites.ts → PREVIEW_PARITY_GATE):
 *
 *   STATIC   `buildScratchSite`'s usual throwaway `python3 -m http.server`
 *            over the built output directory — no preview injections at all.
 *   PREVIEW  the real `moss-cli build <srcDir> --serve`, on its own port via
 *            `MOSS_PREVIEW_PORT_BASE` (never 8080 — that may be a developer's
 *            own running app).
 *
 * Both ports come from gate-ports.ts (`preview-parity-gate:static` /
 * `:preview`) — the spec asks that same helper for the same keys rather than
 * hardcode a port.
 *
 *   npx playwright test -c playwright/preview-parity-gate.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite, scratchSiteDir } from '../tests/e2e/helpers/scratch-site';
import { PREVIEW_PARITY_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const serveDir = buildScratchSite(PREVIEW_PARITY_GATE);
const srcDir = scratchSiteDir(PREVIEW_PARITY_GATE.name);

export default defineGateConfig({
  gate: 'preview-parity-gate',
  use: { colorScheme: 'light' },
  // A real preview build (22 images, a hero, a wrapping grid, a CSV embed)
  // costs more than the other gates' single-page probes — generous but not
  // unbounded.
  timeout: 45_000,
  webServer: [
    { serveDir, port: gatePort('preview-parity-gate:static') },
    { srcDir, port: gatePort('preview-parity-gate:preview') },
  ],
});
