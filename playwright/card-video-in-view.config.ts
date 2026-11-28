import './localhost-no-proxy';
import { defineGateConfig } from './define-gate-config';

/**
 * Render gate: video card covers play muted while in view, stay still under
 * reduced motion, and no more than three play at once. Self-contained: the
 * compiled theme script runs against a handwritten page served by a route.
 *
 *   npx playwright test -c playwright/card-video-in-view.config.ts
 */
export default defineGateConfig({
  gate: 'card-video-in-view',
});
