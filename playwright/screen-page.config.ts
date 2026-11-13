/**
 * Playwright config for the `content_width: screen` page gate
 * (tests/render-gates/site/screen-page.spec.ts).
 *
 * Stylesheet-injection gate: the spec inlines site.css into `page.setContent`,
 * routes the srcset candidates to generated SVGs, and reads which one each
 * engine picked and where the nav sits. No webServer, no `moss build`.
 *
 * Run via: pnpm run test:render-gates screen-page
 */
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'screen-page',
  fullyParallel: true,
});
