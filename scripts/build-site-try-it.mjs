#!/usr/bin/env node
// Builds what the landing's try-it block loads and is not checked in:
//   site/home/try-it/wasm/          moss-bindings compiled to WebAssembly
//   site/home/try-it/sample-*.html  the first page it shows, rendered by that wasm
// Both are ignored by git and published by `moss build site`, which does not
// read .gitignore. predeploy-landing.mjs runs this and refuses a deploy it fails.
// A built copy is not checked in because it would add megabytes to the public
// history every time moss-core changes.
//
// Usage: node scripts/build-site-try-it.mjs   (needs wasm-pack and the wasm32-unknown-unknown target)
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync, rmSync } from 'node:fs';
import { homedir } from 'node:os';
import { delimiter, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { tryItWords } from './landing-copy.mjs';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const HERE = join(ROOT, 'site/home/try-it');
const OUT = join(HERE, 'wasm');
const PAGES = { en: 'site/index.html', 'zh-hans': 'site/zh-hans/index.html', 'zh-hant': 'site/zh-hant/index.html' };

const env = { ...process.env, PATH: [join(homedir(), '.cargo/bin'), process.env.PATH].join(delimiter) };
const built = spawnSync('wasm-pack', [
  'build', 'crates/moss-bindings', '--release', '--target', 'web',
  '--out-dir', OUT, '--no-pack', '--', '--features', 'wasm',
], { cwd: ROOT, env, stdio: 'inherit' });
if (built.error) console.error(built.error.message);
if (built.status !== 0) process.exit(built.status ?? 1);
// wasm-pack's own `*` ignore file would be published with the folder.
rmSync(join(OUT, '.gitignore'), { force: true });

const wasm = await import(pathToFileURL(join(OUT, 'moss_bindings.js')).href);
wasm.initSync({ module: readFileSync(join(OUT, 'moss_bindings_bg.wasm')) });
for (const [locale, page] of Object.entries(PAGES)) {
  const folder = tryItWords(readFileSync(join(ROOT, page), 'utf8'))?.folder;
  if (!folder) throw new Error(`${page} has no try-it folder name`);
  const text = readFileSync(join(HERE, 'samples', `${locale}.txt`), 'utf8');
  const { bundle } = JSON.parse(wasm.siteBundle(JSON.stringify({ files: ['index.md'], texts: { 'index.md': text }, detected_lang: locale })));
  const site = new wasm.MossSite(JSON.stringify(bundle));
  site.setTexts(JSON.stringify({ 'index.md': text }), folder);
  // The block loads this page; noindex keeps it out of the landing's sitemap.
  const html = JSON.parse(site.preview('index.md', text, '/')).html.replace('<head>', '<head><meta name="robots" content="noindex">');
  writeFileSync(join(HERE, `sample-${locale}.html`), html);
  site.free();
}
console.log('built site/home/try-it/wasm and its sample pages');
