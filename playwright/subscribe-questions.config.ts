/**
 * Render gate for a `:::subscribe` that asks questions: every state of the
 * form at phone and desktop width, in light and dark.
 *
 *   npx playwright test -c playwright/subscribe-questions.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { SUBSCRIBE_QUESTIONS_GATE } from '../tests/e2e/helpers/subscribe-questions-site';
import { defineGateConfig } from './define-gate-config';

buildScratchSite(SUBSCRIBE_QUESTIONS_GATE);

export default defineGateConfig({ gate: 'subscribe-questions' });
