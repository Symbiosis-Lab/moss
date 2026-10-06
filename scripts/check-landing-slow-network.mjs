#!/usr/bin/env node
// Slow-network contract: with nothing but the HTML document, the page is
// readable end to end and the signup is honest about needing the runtime. (The
// runtime is inline in the page, so there is no deferred runtime to release.)
import { loadPlaywright, resolveBaseURL } from './landing-harness.mjs';

const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const engines = await loadPlaywright();
const assert = (ok, message) => { if (!ok) throw new Error(message); };
const locales = ['', 'zh-hans/', 'zh-hant/'];
const selectors = ['.scene h2', '.lede', '#beta', '#footer a'];
const fault = process.env.SLOW_NETWORK_FAULT === '1';
const installFault = (page) => page.addStyleTag({ content: ':is(html:not(.js), html[data-static]) #col { display:none } :is(html:not(.js), html[data-static]) #intro { min-height:0; padding-block:100px 32px } :is(html:not(.js), html[data-static]) #intro h1 { position:static } :is(html:not(.js), html[data-static]) .page { display:block; margin-top:0 } :is(html:not(.js), html[data-static]) .scene { min-height:0; padding:32px 0 } :is(html:not(.js), html[data-static]) #five { position:relative; min-height:100vh; opacity:1; pointer-events:auto; background:#101110 } :is(html:not(.js), html[data-static]) #five-in { width:min(900px, calc(100% - 40px)) }' });

async function visibleCopy(page, label) {
  return page.evaluate((selectors) => selectors.map((selector) => {
    const el = document.querySelector(selector), rect = el?.getBoundingClientRect(), cs = el && getComputedStyle(el);
    return { selector, visible: !!el && cs.visibility !== 'hidden' && cs.display !== 'none', top: rect?.top ?? 0, width: rect?.width ?? 0, height: rect?.height ?? 0, parent: el?.parentElement && [getComputedStyle(el.parentElement).display, getComputedStyle(el.parentElement).visibility] };
  }), selectors).then((rows) => {
    for (const row of rows) assert(row.visible && row.width > 0 && row.height > 0, `${label}: ${row.selector} is not readable: ${JSON.stringify(row)}`);
    return rows;
  });
}

async function htmlOnly(browser, locale) {
  const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  await page.route('**/*', (route) => route.request().resourceType() === 'document' ? route.continue() : route.abort());
  await page.goto(new URL(locale, baseURL).href, { waitUntil: 'commit' });
  await page.waitForSelector('#c1 h2');
  await page.waitForFunction(() => document.querySelector('#c1 h2').getBoundingClientRect().width > 0, null, { timeout: 5000 });
  if (fault) await installFault(page);
  await visibleCopy(page, `${locale || 'en'} html-only`);
  const state = await page.evaluate(() => {
    const footer = document.querySelector('#footer').getBoundingClientRect();
    const close = document.querySelector('#five-headline').getBoundingClientRect();
    const note = document.querySelector('.static-signup-note');
    return { scrollable: document.documentElement.scrollHeight > innerHeight, footer: footer.height > 0, close: close.height > 0, note: !!note && getComputedStyle(note).display !== 'none', form: getComputedStyle(document.querySelector('#beta-form')).visibility };
  });
  assert(state.scrollable && state.footer && state.close, `${locale || 'en'} html-only: page is not end-to-end scrollable: ${JSON.stringify(state)}`);
  assert(state.note && state.form === 'hidden', `${locale || 'en'} html-only: signup fallback is not honest: ${JSON.stringify(state)}`);
  await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
  assert(await page.evaluate(() => document.querySelector('#footer').getBoundingClientRect().top < innerHeight), `${locale || 'en'} html-only: footer cannot be reached`);
  await page.close();
}

// With scripts off entirely, a desktop reads the page as plain document flow:
// the close stands after the copy rather than pulled up over it, the way the
// live page pulls it up for the carry to scrub across.
async function noScript(browser, locale) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto(new URL(locale, baseURL).href);
  const overlap = await page.evaluate(() => Math.round(Math.max(...[...document.querySelectorAll('.scene')].map((s) => s.getBoundingClientRect().bottom)) - document.querySelector('#five').getBoundingClientRect().top));
  assert(overlap <= 1, `${locale || 'en'} with scripts off at 1440px: the close rides ${overlap}px up over the copy`);
  await context.close();
}

for (const [name, type] of [['chromium', engines.chromium], ['webkit', engines.webkit]]) {
  const browser = await type.launch();
  try {
    for (const locale of locales) { await htmlOnly(browser, locale); await noScript(browser, locale); }
    console.log(`${name}: slow-network checks passed for ${locales.length} locales, and with scripts off on a desktop`);
  } finally { await browser.close(); }
}
await close();
