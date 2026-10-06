import assert from 'node:assert/strict';
import { resolveBaseURL, loadPlaywright } from './landing-harness.mjs';
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const pw = await loadPlaywright();
try {
for (const engine of ['chromium', 'webkit']) {
 const browser = await pw[engine].launch();
 try {
 for (const route of ['/', '/zh-hans/', '/zh-hant/']) {
  const page = await browser.newPage({ reducedMotion:'reduce' });
  const requests=[]; page.on('request', r=>requests.push(r.url()));
  await page.goto(new URL(route,baseURL).href,{waitUntil:'domcontentloaded'});
  assert.equal(await page.locator('.nav-search-btn').count(),0);
  await page.keyboard.press('/'); await page.keyboard.press('Control+k');
  assert.equal(await page.locator('#moss-search').count(),0);
  assert(!requests.some(u=>u.includes('/_moss/pagefind/') || /search[.]/.test(u)));
  if (route === '/') for (const selector of ['meta[name="description"]','meta[property="og:description"]','meta[name="twitter:description"]']) assert.equal(await page.locator(selector).getAttribute('content'),'The simplest way to publish and own your website.');
  console.log(`${engine} ${route}: tagline and absence of search passed`);
  await page.close();
 }
 for (const [route,query,label] of [['/docs/','Markdown','Search'],['/zh-hans/开始使用/','网站','搜索'],['/zh-hant/開始使用/','網站','搜尋']]) {
  const page = await browser.newPage({ viewport:{width:1440,height:900},reducedMotion:'reduce' });
  const errors=[]; page.on('pageerror', e=>errors.push(e.message));
  const requests=[]; page.on('request', r=>requests.push(r.url()));
 await page.goto(new URL(route,baseURL).href,{waitUntil:'domcontentloaded'});
  assert(!requests.some(u=>u.includes('/_moss/pagefind/')),'Index must remain lazy');
  const trigger=page.locator('.nav-search-btn');
  await trigger.click();
  const input=page.locator('.moss-search__input');
  await input.waitFor({state:'visible'});
  assert.equal(await input.getAttribute('aria-label'),label);
  assert.equal(await page.locator('.moss-search__panel').evaluate(el=>getComputedStyle(el).backgroundColor),'rgb(250, 248, 245)');
  await input.fill(query);
  const results=page.locator('.moss-search__link');
  await results.first().waitFor({state:'visible',timeout:20000});
  const href=await results.first().getAttribute('href');
  const response=await page.request.get(new URL(href,baseURL).href); assert.equal(response.status(),200);
 await input.fill('"uniquely nonexistent platypus"');
  await page.locator('.moss-search__status').waitFor({state:'visible',timeout:10000});
  await input.press('Escape');
  assert.equal(await trigger.evaluate(el=>document.activeElement===el),true);
  await page.keyboard.press('/'); await input.waitFor({state:'visible'}); await input.press('Escape');
  await page.keyboard.press('Control+k'); await input.waitFor({state:'visible'}); await input.press('Escape');
  assert.deepEqual(errors,[]);
  console.log(`${engine} ${route}: results, links, empty state, Escape/focus, /, Ctrl+K passed`);
  await page.close();
 }
 const page=await browser.newPage({viewport:{width:390,height:844},isMobile:true,hasTouch:true,reducedMotion:'reduce'});
 await page.goto(new URL('/docs/',baseURL).href,{waitUntil:'domcontentloaded'}); await page.locator('.nav-search-btn').tap();
 await page.locator('.moss-search__input').fill('Markdown'); await page.locator('.moss-search__link').first().waitFor({state:'visible'});
 const bounds=await page.locator('.moss-search__panel').boundingBox(); assert(bounds.x>=0 && bounds.x+bounds.width<=390);
 console.log(`${engine} mobile: search and panel fit passed`); await page.close();
 } finally { await browser.close(); }
}
} finally {await close();}
