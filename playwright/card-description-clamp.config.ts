/**
 * Render gate: a card's auto-built description clamps to two lines with a
 * visible ellipsis, in both LTR and CJK text, without breaking equal-height
 * rows or single-line titles.
 *
 * Whether the legacy `-webkit-line-clamp` multi-line-truncation box actually
 * paints an ellipsis (vs. a bare block clip that just stops mid-sentence) is
 * a real-engine question, not something a Rust test reading emitted CSS can
 * answer.
 *
 *   npx playwright test -c playwright/card-description-clamp.config.ts
 *
 * No binary and no scratch site: the spec injects the branch's real site.css
 * straight into `page.setContent`.
 */
import './localhost-no-proxy';
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'card-description-clamp',
  use: { colorScheme: 'light' },
});
