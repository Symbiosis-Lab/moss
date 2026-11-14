/**
 * Render gate: a picture's focus point in the browser editor. The spec starts
 * `moss edit --serve` itself, so there is no webServer here.
 *
 *   npx playwright test -c playwright/focus-point.config.ts
 *
 * Prereq: `bash scripts/build-moss-cli.sh` (or MOSS_BIN): it builds the
 * browser bundle into the binary, without which the editor is not served.
 */
import './localhost-no-proxy';
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'focus-point',
  timeout: 120_000,
});
