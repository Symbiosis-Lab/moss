/**
 * Render gate: moss's round marks are round and one message line across, and
 * every text field shows one focus treatment on its own edge.
 *
 * Stylesheet injection and the real subscribe script against a mocked
 * service: no site is built.
 *
 *   npx playwright test -c playwright/round-marks.config.ts
 */
import './localhost-no-proxy';
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({ gate: 'round-marks' });
