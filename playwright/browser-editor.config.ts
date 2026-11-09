/**
 * Render gate: the browser editor `moss edit --serve` serves boots in both
 * engines, saves a keystroke to disk, and opens a PDF from its embed card in
 * the file viewer although the source route serves it sandboxed. The spec
 * starts the server itself, so there is no webServer here.
 *
 *   npx playwright test -c playwright/browser-editor.config.ts
 *
 * Prereq: `bash scripts/build-moss-cli.sh` (or MOSS_BIN): it builds the
 * browser bundle into the binary, without which the editor is not served.
 */
import './localhost-no-proxy';
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'browser-editor',
  // Chromium's full build: the headless shell has no PDF viewer, and the
  // spec asserts the viewer loads.
  fullChromium: true,
  timeout: 120_000,
});
