import './localhost-no-proxy';
import { defineGateConfig } from './define-gate-config';

/**
 * Render gate for `![[clip.mp4|loop]]` ambient video feature.
 * Self-contained: injects the compiled theme.js + minimal ambient CSS via
 * page.setContent + addStyleTag. No dev/vite webServer needed.
 *
 *   npx playwright test -c playwright/ambient-video.config.ts
 *
 * Catches JS/CSS behaviour that jsdom cannot prove:
 *   - wrapper injection, toggle presence, aria-label state
 *   - prefers-reduced-motion: reduce guard removes autoplay + shows toggle
 *
 * Chromium only: the gate is about wrapper/toggle/attribute behaviour, not
 * video decode, so one engine answers it. Naming that engine as a project
 * (rather than spreading the device into a bare top-level `use`, which this
 * config used to do) is what lets `--project` skip it correctly on a runner
 * that only has WebKit installed — the previous, project-less shape ran
 * unfiltered instead and tried to launch a Chromium that was not there.
 */
export default defineGateConfig({
  gate: 'ambient-video',
  engines: ['chromium'],
  fullyParallel: true,
});
