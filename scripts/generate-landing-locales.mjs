import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';

const source = new URL('../site/index.html', import.meta.url);
const html = await readFile(source, 'utf8');
// landing-i18n.js exports its catalog as a plain CommonJS module when
// `document` doesn't exist (see the file itself), so this is a real
// require(), not a regex slice of the source evaluated with Function().
const { en, hans, hant } = createRequire(import.meta.url)('../site/landing-i18n.js');
const catalogs = { 'zh-hans': hans, 'zh-hant': hant };

function escaped(value) {
  return value.replaceAll('&', '&amp;').replaceAll('"', '&quot;').replaceAll("'", '&#39;');
}

function localize(sourceHtml, locale) {
  let result = sourceHtml.replace('<html lang="en">', `<html lang="${locale}">`);
  const target = catalogs[locale];
  const replacements = Object.keys(en)
    .filter(key => key !== 'product' && typeof en[key] === 'string' && en[key] !== target[key])
    .map(key => [en[key], target[key]])
    .sort((a, b) => b[0].length - a[0].length);
  const protectedParts = result.split(/(<(?:script|style)\b[\s\S]*?<\/(?:script|style)>)/gi);
  result = protectedParts.map(part => {
    if (/^<(?:script|style)\b/i.test(part)) return part;
    for (const [from, to] of replacements) part = part.replaceAll(from, to).replaceAll(escaped(from), escaped(to));
    for (const key of Object.keys(en.docs)) part = part.replaceAll(en.docs[key], target.docs[key]);
    return part;
  }).join('');
  result = result.replace(/(<span class="moss-wordmark">)[^<]+/, `$1${target.product}`);
  const languages = [ ['en', 'EN', 'English', '/'], ['zh-hant', '繁', '繁體中文', '/zh-hant/'], ['zh-hans', '简', '简体中文', '/zh-hans/'] ];
  const nav = `<nav class="language-picker nav-lang-toggle" aria-label="${locale === 'zh-hans' ? '语言' : '語言'}">` + languages.map(([code, label, name, href]) => code === locale
    ? `<span class="nav-lang-current" lang="${code}" aria-label="${name}">${label}</span>`
    : `<a class="nav-lang-link" data-landing-locale="${code}" hreflang="${code}" lang="${code}" aria-label="${name}" href="${href}">${label}</a>`).join('<span aria-hidden="true">/</span>') + '</nav>';
  result = result.replace(/<nav class="language-picker nav-lang-toggle"[\s\S]*?<\/nav>/, nav);
  const discovery = `<!-- landing-discovery:start -->
<link rel="canonical" href="https://mosspub.com/${locale}/">
<link rel="alternate" hreflang="en" href="https://mosspub.com/">
<link rel="alternate" hreflang="zh-Hant" href="https://mosspub.com/zh-hant/">
<link rel="alternate" hreflang="zh-Hans" href="https://mosspub.com/zh-hans/">
<link rel="alternate" hreflang="x-default" href="https://mosspub.com/">
<!-- landing-discovery:end -->`;
  result = result.replace(/<!-- landing-discovery:start -->[\s\S]*?<!-- landing-discovery:end -->/, discovery);
  const required = ['title', 'description', 'intro', 'h1', 'b1', 'h2', 'b2', 'h3', 'b3', 'h4', 'b4', 'betaCta', 'start', 'editor', 'theme', 'media', 'requestType', 'plugin', 'registry', 'closeH', 'closeB', 'download', 'macNote', 'soon', 'install', 'copy', 'betaH', 'betaB', 'email', 'request', 'privacy'];
  for (const key of required) {
    if (!result.includes(target[key]) && !result.includes(escaped(target[key]))) throw new Error(`${locale} static copy missing ${key}`);
  }
  // Translations may change text and localized doc URLs, never app selectors,
  // install commands, or external destinations.
  const selectors = value => [...value.matchAll(/\b(?:class|id)="[^"]*"/g)].map(match => match[0]).sort();
  if (JSON.stringify(selectors(result)) !== JSON.stringify(selectors(sourceHtml))) throw new Error(`${locale} altered HTML selectors`);
  // The footer privacy link is deliberately excluded from this invariant: it is same-origin
  // (/privacy/, /zh-hans/privacy/, /zh-hant/privacy/ — see docs.privacy above) precisely so it
  // resolves to the localized page on any host, so it must NOT stay byte-identical across
  // locales. Its correctness is instead asserted by check-site-preview.mjs, which fetches the
  // link from each landing route and checks the target page's <html lang> against the locale.
  const externals = value => [...value.replace(/<!-- landing-discovery:start -->[\s\S]*?<!-- landing-discovery:end -->/, '').matchAll(/(?:href|src)="https?:[^" ]+"/g)].map(match => match[0]).sort();
  if (JSON.stringify(externals(result)) !== JSON.stringify(externals(sourceHtml))) throw new Error(`${locale} altered external URLs`);
  const commands = value => [...value.matchAll(/<code>[^<]+<\/code>/g)].map(match => match[0]);
  if (JSON.stringify(commands(result)) !== JSON.stringify(commands(sourceHtml))) throw new Error(`${locale} altered install commands`);
  return result;
}

async function emit(url, contents) {
  if (process.argv.includes('--check')) {
    const current = await readFile(url, 'utf8').catch(() => '');
    if (current !== contents) throw new Error(`${url.pathname} is stale; run node scripts/generate-landing-locales.mjs`);
    return;
  }
  await writeFile(url, contents);
}
// Same contract as emit(), for a binary target (favicon.ico): --check
// compares bytes instead of decoding as UTF-8, which would corrupt a PNG's
// own bytes into an unequal string on the read alone.
async function emitBinary(url, buffer) {
  if (process.argv.includes('--check')) {
    const current = await readFile(url).catch(() => null);
    if (!current || !current.equals(buffer)) throw new Error(`${url.pathname} is stale; run node scripts/generate-landing-locales.mjs`);
    return;
  }
  await writeFile(url, buffer);
}

for (const locale of ['zh-hans', 'zh-hant']) {
  const directory = new URL(`../site/${locale}/`, import.meta.url);
  await mkdir(directory, { recursive: true });
  await emit(new URL('index.html', directory), localize(html, locale));
}

const icon = await readFile(new URL('../crates/moss-build/icons/icon.svg', import.meta.url), 'utf8');
// moss's own crate ships icon.svg at its full, roomy viewBox (see that
// file's header: never rescaled there, since mark.css's proportion table
// keys off it for every other consumer of the mark). crates/moss-build's
// own favicon rasterizer tightens a COPY of that box for its bundled
// default favicon -- see tighten_default_favicon_viewbox's own comment for
// how -411 -244 2608 2608 was measured (an even ~12%-of-side margin,
// deliberately less aggressive than the crop below). This script does the
// same kind of crop for the landing site's OWN favicon.svg, which moss's
// Rust-side tightening never touches (it only fires for its own bundled
// default, never for a vault-supplied assets/favicon.svg). Measured
// 2026-09-20 by rendering icon.svg at 4000x4000 and scanning alpha for the
// ink's own pixel bounding box: x=[8.9,1777.1] y=[7.7,2112.3] within the
// shipped box, content 1768x2105 -- a tight SQUARE window built from that
// (side = the taller dimension, so the mark touches the box on its own
// longer axis with zero margin there, and only the unavoidable letterboxing
// a portrait mark needs to become square on the other).
const favicon = icon.replace('viewBox="-647 -373 3145 3145"', 'viewBox="-182 -15 2150 2150"');
await emit(new URL('../site/assets/brand/favicon.svg', import.meta.url), favicon);
// moss's own build (crates/moss-build/src/build/render/blocking.rs,
// resolve_favicon) looks for a user favicon at exactly assets/favicon.{svg,
// png,ico} -- never assets/brand/ -- so every moss-built docs page (Get
// Started, etc.) falls back to its own bundled default mark without this
// copy. Kept identical to the one above rather than symlinked: git-tracked
// symlinks are one more thing a deploy path has to preserve faithfully, and
// this file changes only when the mark itself does.
await emit(new URL('../site/assets/favicon.svg', import.meta.url), favicon);

// A real /favicon.ico (site owner finding 6, 2026-09-21): browsers request
// this path directly, by convention, regardless of what a page's own <link
// rel="icon"> says, and this site never had one -- 404 in both eras. The
// docs pages already get PNG fallbacks (apple-touch-icon 180, 32, 16) for
// free: crates/moss-build's resolve_favicon rasterizes them at build time
// from this same assets/favicon.svg (never assets/brand/), so nothing here
// needs to duplicate that. An .ico is not among what it produces, and it
// runs after this script (moss-cli build site, not generate-landing-
// locales.mjs), so it cannot supply one either -- generated here instead,
// straight from the tightened SVG above, with rsvg-convert (the same tool
// used to spot-check this crop while it was being measured). #faf8f5
// matches crates/moss-build/src/build/site_meta/favicon.rs's own paper()
// background, so this and the build-time PNGs read as one consistent icon
// rather than two different crops or grounds.
function rasterizePNG(svg, size) {
  return execFileSync('rsvg-convert', ['-w', String(size), '-h', String(size), '-b', '#faf8f5'], { input: svg, maxBuffer: 1024 * 1024 });
}
// A minimal ICO container: a 6-byte header, one 16-byte directory entry per
// image, then the images themselves verbatim -- modern ICO readers (every
// browser; Windows since Vista) accept PNG-encoded entries directly, so
// this packs the same PNG bytes rsvg-convert already produced rather than
// re-encoding through BMP.
function packICO(images) {
  const count = images.length;
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); header.writeUInt16LE(1, 2); header.writeUInt16LE(count, 4);
  let offset = 6 + count * 16;
  const dir = Buffer.alloc(count * 16);
  images.forEach(({ size, data }, i) => {
    const e = i * 16;
    dir.writeUInt8(size >= 256 ? 0 : size, e); dir.writeUInt8(size >= 256 ? 0 : size, e + 1);
    dir.writeUInt8(0, e + 2); dir.writeUInt8(0, e + 3);
    dir.writeUInt16LE(1, e + 4); dir.writeUInt16LE(32, e + 6);
    dir.writeUInt32LE(data.length, e + 8); dir.writeUInt32LE(offset, e + 12);
    offset += data.length;
  });
  return Buffer.concat([header, dir, ...images.map((im) => im.data)]);
}
try {
  const ico = packICO([16, 32].map((size) => ({ size, data: rasterizePNG(favicon, size) })));
  await emitBinary(new URL('../site/favicon.ico', import.meta.url), ico);
} catch (e) {
  throw new Error(`favicon.ico generation needs rsvg-convert on PATH (brew install librsvg / apt install librsvg2-bin): ${e.message}`);
}
