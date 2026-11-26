/**
 * Render gate: the preview's progress line from a save to rest, in both
 * engines. The spec starts `moss edit --serve` itself, so there is no
 * webServer here.
 *
 *   npx playwright test -c playwright/progress-line.config.ts
 *
 * Prereq: `bash scripts/build-moss-cli.sh` (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'progress-line',
  timeout: 240_000,
});
