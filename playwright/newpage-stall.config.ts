/**
 * The page-open guard in tests/support/browser-test.ts: a stalled first page
 * relaunches the browser, a stalled later page is tried once more. Chromium
 * only, so a real WebKit stall cannot stand in for an injected one.
 *
 *   npx playwright test -c playwright/newpage-stall.config.ts
 */
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'newpage-stall',
  engines: ['chromium'],
  timeout: 90_000,
});
