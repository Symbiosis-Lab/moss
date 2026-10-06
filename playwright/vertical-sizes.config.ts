/**
 * Playwright config for the vertical-rl `sizes=` fetch gate
 * (tests/render-gates/site/vertical-sizes.spec.ts).
 *
 * Stylesheet-injection gate: the spec inlines site.css and vertical.css into
 * `page.setContent`, routes the srcset candidates to generated SVGs, and reads
 * which one each engine picked. No webServer, no `moss build`, no MOSS_BIN.
 *
 * Run via: pnpm run test:render-gates vertical-sizes
 */
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'vertical-sizes',
  fullyParallel: true,
});
