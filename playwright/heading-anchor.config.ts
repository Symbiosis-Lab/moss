/**
 * Render gate: a heading's `#` is drawn by CSS, so copying a heading copies
 * the heading — and a grid cell's heading carries no `#` at all.
 *
 * An engine question in the strictest sense: the assertion reads
 * `getSelection().toString()`, which IS the browser's own text serializer.
 * jsdom has no `::after` content and no selection model to serialize. Both
 * engines run because the bug being pinned was a disagreement between them —
 * Blink excluded a `user-select: none` text node from the clipboard, WebKit
 * included it — so a single-engine gate would have been green throughout.
 *
 *   npx playwright test -c playwright/heading-anchor.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { HEADING_ANCHOR_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';

const PORT = 8759;
const serveDir = buildScratchSite(HEADING_ANCHOR_GATE);

export default defineGateConfig({
  gate: 'heading-anchor',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
