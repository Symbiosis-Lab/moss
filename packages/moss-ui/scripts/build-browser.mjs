import { createHash } from 'node:crypto';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';

const root = new URL('../', import.meta.url);
const out = new URL('dist-browser/', root);
const script = await readFile(new URL('src/browser/placeholder.js', root));
const hash = createHash('sha256').update(script).digest('hex').slice(0, 12);
await rm(out, { recursive: true, force: true });
await mkdir(new URL('assets/', out), { recursive: true });
await writeFile(new URL(`assets/app-${hash}.js`, out), script);
await writeFile(new URL('index.html', out), `<!doctype html>
<html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>moss editor</title><body><main><h1>moss editor</h1><p id="status">Connecting to moss…</p></main>
<script type="module" src="/assets/app-${hash}.js"></script></body></html>
`);
