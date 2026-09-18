import { mkdir, readFile, writeFile } from 'node:fs/promises';

const source = new URL('../site/index.html', import.meta.url);
const html = await readFile(source, 'utf8');
const runtime = await readFile(new URL('../site/landing-i18n.js', import.meta.url), 'utf8');
const catalogSource = runtime.match(/  const en = \{[\s\S]+?  const catalogs=/)?.[0]
  .replace('  const catalogs=', '  return { en, hans, hant };');
if (!catalogSource) throw new Error('Could not read landing locale catalogs');
const { en, hans, hant } = Function(catalogSource)();
const catalogs = { 'zh-hans': hans, 'zh-hant': hant };

function escaped(value) {
  return value.replaceAll('&', '&amp;').replaceAll('"', '&quot;').replaceAll("'", '&#39;');
}

function localize(sourceHtml, locale) {
  let result = sourceHtml.replace('<html lang="en">', `<html lang="${locale}">`);
  const target = catalogs[locale];
  const replacements = Object.keys(en)
    .filter(key => typeof en[key] === 'string' && en[key] !== target[key])
    .map(key => [en[key], target[key]])
    .sort((a, b) => b[0].length - a[0].length);
  const protectedParts = result.split(/(<(?:script|style)\b[\s\S]*?<\/(?:script|style)>)/gi);
  result = protectedParts.map(part => {
    if (/^<(?:script|style)\b/i.test(part)) return part;
    for (const [from, to] of replacements) part = part.replaceAll(from, to).replaceAll(escaped(from), escaped(to));
    for (const key of Object.keys(en.docs)) part = part.replaceAll(en.docs[key], target.docs[key]);
    return part;
  }).join('');
  const nav = locale === 'zh-hans'
    ? '<nav class="language-picker nav-lang-toggle" aria-label="语言"><a class="nav-lang-link" data-landing-locale="en" hreflang="en" href="/">English</a><span aria-hidden="true">/</span><span class="nav-lang-current">简体中文</span><span aria-hidden="true">/</span><a class="nav-lang-link" data-landing-locale="zh-hant" hreflang="zh-Hant" href="/zh-hant/">繁體中文</a></nav>'
    : '<nav class="language-picker nav-lang-toggle" aria-label="語言"><a class="nav-lang-link" data-landing-locale="en" hreflang="en" href="/">English</a><span aria-hidden="true">/</span><a class="nav-lang-link" data-landing-locale="zh-hans" hreflang="zh-Hans" href="/zh-hans/">简体中文</a><span aria-hidden="true">/</span><span class="nav-lang-current">繁體中文</span></nav>';
  result = result.replace(/<nav class="language-picker nav-lang-toggle"[\s\S]*?<\/nav>/, nav);
  const required = ['title', 'description', 'intro', 'h1', 'b1', 'h2', 'b2', 'h3', 'b3', 'h4', 'b4', 'betaCta', 'start', 'editor', 'theme', 'media', 'requestType', 'plugin', 'registry', 'closeH', 'closeB', 'download', 'macNote', 'soon', 'install', 'copy', 'betaH', 'betaB', 'email', 'request', 'privacy'];
  for (const key of required) {
    if (!result.includes(target[key]) && !result.includes(escaped(target[key]))) throw new Error(`${locale} static copy missing ${key}`);
  }
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

for (const locale of ['zh-hans', 'zh-hant']) {
  const directory = new URL(`../site/${locale}/`, import.meta.url);
  await mkdir(directory, { recursive: true });
  await emit(new URL('index.html', directory), localize(html, locale));
}

const icon = await readFile(new URL('../crates/moss-build/icons/icon.svg', import.meta.url), 'utf8');
const favicon = icon.replace('viewBox="-647 -373 3145 3145"', 'viewBox="-411 -244 2608 2608"');
await emit(new URL('../site/assets/brand/favicon.svg', import.meta.url), favicon);
