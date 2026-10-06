/**
 * Render gate: a route-enabled page's dashed line, numbered badges and
 * leader draw correctly in a real browser — DOM order (the line sits under
 * the marker layer, badges over it), badge legibility (sampled-pixel
 * contrast >= 4.5:1 for both the filled and the hollow badge, light and
 * dark), and the offset badge's leader line, at a desktop and a phone width.
 *
 * The Rust snapshot suite (snapshot_places_route_site) pins the exact SVG
 * bytes this same fixture shape produces; it cannot say what a browser does
 * with them once site.css's cascade and `prefers-color-scheme` apply.
 *
 *   npx playwright test -c playwright/place-map-route.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { PLACE_MAP_ROUTE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('place-map-route');
const serveDir = buildScratchSite(PLACE_MAP_ROUTE_GATE);

export default defineGateConfig({
  gate: 'place-map-route',
  // The badge sits on a locator shown at roughly half the 720-unit viewBox
  // (~350 CSS px), so its ~18-unit diameter draws at well under 10 CSS px —
  // a higher device scale gives the pixel-sampling contrast check more
  // samples to work with, on both engines alike.
  use: { baseURL: `http://localhost:${PORT}/`, deviceScaleFactor: 2 },
  webServer: { serveDir, port: PORT },
});
