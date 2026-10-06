import { defineConfig } from 'vite';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { gatePort } from './gate-ports';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

export default defineConfig({
  root: repoRoot,
  server: { port: gatePort('iframe-open-url'), strictPort: true },
});
