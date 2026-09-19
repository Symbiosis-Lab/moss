import { pathToFileURL } from 'node:url';
const modulePath = process.env.PLAYWRIGHT_MODULE || 'playwright';
const { chromium, webkit } = await import(modulePath.startsWith('/') ? pathToFileURL(modulePath).href : modulePath);
const base = process.argv[2];
if (!base) throw Error('Pass site URL');
for (const engine of [chromium, webkit]) {
 const browser = await engine.launch();
 try {
  for (const locale of ['', 'zh-hans/', 'zh-hant/']) {
   const page = await browser.newPage({reducedMotion:'reduce'});
   const calls=[]; let reply={subscribed:true,confirmationSent:true}, status=200;
   await page.route('https://api.mosspub.com/**',async route=>{calls.push({url:route.request().url(),body:JSON.parse(route.request().postData())});await route.fulfill({status,contentType:'application/json',body:JSON.stringify(reply)});});
   await page.goto(new URL(locale,base).href);
   await page.locator('a[href="#beta"]').first().click();
   await page.waitForFunction(()=>document.activeElement?.id==='beta-email',null,{timeout:20000});
   const input=page.locator('#beta-email'), button=page.locator('#beta-form button'), message=page.locator('#beta-form .form-status');
   await input.fill('invalid');await button.click();if(calls.length)throw Error('Invalid email was submitted');
   for (const scenario of ['new','existing','malformed','server']) {
    reply=scenario==='existing'?{subscribed:true,alreadySubscribed:true}:scenario==='malformed'?{}:{subscribed:true,confirmationSent:true};status=scenario==='server'?503:200;
    await input.fill('verification@example.invalid');await button.click();
    const good=['new','existing'].includes(scenario);
    await page.waitForFunction(g=>document.querySelector('#beta-form .form-status').dataset.state===(g?'success':'error'),good);
    if(!await message.textContent())throw Error('No status');
    if((await input.inputValue()==='')!==good)throw Error('Address retention');
   }
   if(calls.some(c=>c.url!=='https://api.mosspub.com/api/sites/landing/subscribe'||c.body.scope!==''||c.body.email!=='verification@example.invalid'))throw Error('Wrong subscription contract');
   console.log(engine.name(),locale||'en','focus, validation, new/existing subscriber, malformed/server error: PASS (mocked; no subscriber created)');
   await page.close();
  }
 } finally {await browser.close();}
}
