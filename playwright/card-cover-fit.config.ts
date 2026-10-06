/**
 * Render gate: `--moss-card-cover-fit` reaches every card layout.
 *
 * The token is a specificity question — whether a later, more specific rule
 * re-declares `object-fit` over the one that reads it — and only a real
 * cascade answers that.
 *
 *   npx playwright test -c playwright/card-cover-fit.config.ts
 *
 * No binary and no scratch site: the spec injects the branch's real site.css
 * straight into `page.setContent`.
 */
import './localhost-no-proxy';
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'card-cover-fit',
  use: { colorScheme: 'light' },
});
