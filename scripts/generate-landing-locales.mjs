import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';

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

for (const locale of ['zh-hans', 'zh-hant']) {
  const directory = new URL(`../site/${locale}/`, import.meta.url);
  await mkdir(directory, { recursive: true });
  await emit(new URL('index.html', directory), localize(html, locale));
}

const icon = await readFile(new URL('../crates/moss-build/icons/icon.svg', import.meta.url), 'utf8');
const favicon = icon.replace('viewBox="-647 -373 3145 3145"', 'viewBox="-411 -244 2608 2608"');
await emit(new URL('../site/assets/brand/favicon.svg', import.meta.url), favicon);
