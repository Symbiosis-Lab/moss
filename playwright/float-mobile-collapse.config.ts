/**
 * Playwright config for the floated embed/figure/listing mobile-collapse
 * gate (tests/render-gates/site/float-mobile-collapse.spec.ts).
 *
 * Stylesheet-injection gate: the spec inlines the real site.css into
 * `page.setContent` and reads real boxes at desktop and phone widths. No
 * webServer, no `moss build`, no MOSS_BIN.
 *
 * Run via: pnpm run test:render-gates float-mobile-collapse
 */
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'float-mobile-collapse',
  fullyParallel: true,
});
