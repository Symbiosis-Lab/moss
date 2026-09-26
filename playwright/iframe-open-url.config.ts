import './localhost-no-proxy';
import { defineConfig, devices } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { gatePort } from './gate-ports';

const PORT = gatePort('iframe-open-url');
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

export default defineConfig({
  testDir: '../tests/render-gates/site',
  testMatch: /iframe-open-url\.spec\.ts$/,
  workers: 1,
  reporter: 'list',
  outputDir: '../target/test-tmp/playwright-iframe-open-url',
  use: { baseURL: `http://localhost:${PORT}` },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
  webServer: {
    command: `npx vite --config playwright/vite.iframe-open-url-harness.config.ts`,
    cwd: repoRoot,
    port: PORT,
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
