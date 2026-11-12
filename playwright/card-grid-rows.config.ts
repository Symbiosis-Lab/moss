/**
 * Render gate: a theme that boxes grid cells and adds `align-self: stretch`
 * gets boxes of one height per row, and nothing inside a cell moves. Row heights and margin collapsing are
 * layout, which only a real engine computes.
 *
 *   npx playwright test -c playwright/card-grid-rows.config.ts
 *
 * No binary and no scratch site: the spec injects the branch's real site.css
 * straight into `page.setContent`.
 */
import './localhost-no-proxy';
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'card-grid-rows',
  use: { colorScheme: 'light' },
});
