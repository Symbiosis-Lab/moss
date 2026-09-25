#!/usr/bin/env node
// Slow-network contract: the HTML remains readable before the deferred runtime
// arrives, and releasing that runtime does not move a reader already mid-page.
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

async function delayedBoot(browser, locale) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  await page.route('**/*', async (route) => {
    if (route.request().resourceType() === 'document') return route.continue();
    await gate;
    return route.continue();
  });
  await page.goto(new URL(locale, baseURL).href, { waitUntil: 'commit' });
  await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight * .4));
  if (fault) await installFault(page);
  const before = await page.evaluate((selectors) => ({ y: scrollY, rows: selectors.map((selector) => { const r = document.querySelector(selector).getBoundingClientRect(); return [selector, r.top]; }) }), selectors);
  if (fault) await page.evaluate(() => document.documentElement.classList.add('js'));
  release();
  await page.waitForFunction(() => document.documentElement.dataset.ready === '1', null, { timeout: 20000 });
  const after = await page.evaluate((selectors) => ({ y: scrollY, rows: selectors.map((selector) => { const r = document.querySelector(selector).getBoundingClientRect(); return [selector, r.top]; }) }), selectors);
  assert(Math.abs(after.y - before.y) <= 2, `${locale || 'en'} delayed boot: scroll moved ${before.y} -> ${after.y}`);
  for (let i = 0; i < before.rows.length; i++) assert(Math.abs(before.rows[i][1] - after.rows[i][1]) <= 2, `${locale || 'en'} delayed boot: ${before.rows[i][0]} reflowed ${before.rows[i][1]} -> ${after.rows[i][1]} (${JSON.stringify({ before, after })})`);
  await page.close();
}

for (const [name, type] of [['chromium', engines.chromium], ['webkit', engines.webkit]]) {
  const browser = await type.launch();
  try {
    for (const locale of locales) { await htmlOnly(browser, locale); await delayedBoot(browser, locale); }
    console.log(`${name}: slow-network checks passed for ${locales.length} locales`);
  } finally { await browser.close(); }
}
await close();
