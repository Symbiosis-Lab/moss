/**
 * Render gate for diagrams drawn from Mermaid text: tests/render-gates/site/diagrams.spec.ts in
 * chromium and webkit. Builds the DIAGRAMS_GATE scratch site from the corpus in
 * crates/moss-build/tests/fixtures/mermaid-corpus/. DIAGRAM_GATE_SERVE_DIR serves an already built
 * site instead, so the same checks can read a copy of a real one.
 *
 * Prereq: `cargo build -p moss-cli`.
 *
 *   npx playwright test -c playwright/diagrams.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { DIAGRAMS_GATE } from '../tests/e2e/helpers/diagram-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

export default defineGateConfig({
  gate: 'diagrams',
  webServer: { serveDir: process.env.DIAGRAM_GATE_SERVE_DIR ?? buildScratchSite(DIAGRAMS_GATE), port: gatePort('diagrams') },
});
