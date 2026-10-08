import './localhost-no-proxy';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('bridge-capture');
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

// The fixture loads the shipped bridge bundle by its repo path, so the static
// server is rooted at the repo.
export default defineGateConfig({
  gate: 'bridge-capture',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'light',
  },
  webServer: { serveDir: repoRoot, port: PORT },
});
