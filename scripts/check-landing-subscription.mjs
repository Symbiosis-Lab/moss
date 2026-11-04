import { loadPlaywright, resolveBaseURL } from './landing-harness.mjs';
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const { chromium, webkit } = await loadPlaywright();
try {
for (const engine of [chromium, webkit]) {
 const browser = await engine.launch();
 try {
  for (const locale of ['', 'zh-hans/', 'zh-hant/']) {
   const page = await browser.newPage({reducedMotion:'reduce'});
   const calls=[], checks=[]; let reply={subscribed:true,confirmationSent:true}, status=200, challenge=false, verdict='passed';
   const CAPTCHA='https://api.mosspub.com/captcha/';
   // the page's visit counter posts to the same host: answer it without counting it, and never let a check reach production
   await page.route('https://api.mosspub.com/**',async route=>{
    const url=new URL(route.request().url());
    if(/\/beacon$/.test(url.pathname))return route.fulfill({status:204});
    // the host's captcha page, reporting at once what the scenario sets: passed, failed, or closed by the reader
    if(url.pathname==='/captcha/'){checks.push(url.search);return route.fulfill({contentType:'text/html',body:`<!doctype html><script>parent.postMessage({moss_captcha:"${verdict}"},"*")</script>`});}
    calls.push({url:route.request().url(),body:JSON.parse(route.request().postData())});
    if(challenge){challenge=false;return route.fulfill({status:403,contentType:'application/json',body:JSON.stringify({need_captcha:true,captcha_url:CAPTCHA})});}
    await route.fulfill({status,contentType:'application/json',body:JSON.stringify(reply)});
   });
   await page.goto(new URL(locale,baseURL).href);
   // No link points at the form any more: the hero CTA leads to the downloads. Reveal the closing
   // scene the way the page does and wait for it to stop being inert, which is what the old click
   // was really waiting for.
   await page.evaluate(()=>window.mossLanding?.openSignup());
   await page.waitForFunction(()=>{const e=document.querySelector('#beta-email');return !!e&&!e.closest('[inert]');},null,{timeout:20000});
   await page.locator('#beta-email').focus();
   await page.waitForFunction(()=>document.activeElement?.id==='beta-email',null,{timeout:20000});
   // a catalog string that carries a line break has to be set as markup: set as text, the reader sees its tag
   const shown=await page.evaluate(()=>document.body.innerText), tag=shown.match(/.{0,40}<\/?[a-z][^>]*>.{0,40}/i);
   if(tag)throw Error(`Markup shown as text on ${locale||'en'}: ${tag[0]}`);
   const input=page.locator('#beta-email'), button=page.locator('#beta-form button'), message=page.locator('#beta-form .form-status');
   await input.fill('invalid');await button.click();if(calls.length)throw Error('Invalid email was submitted');
   for (const scenario of ['new','existing','malformed','server','checked','check-failed','check-closed']) {
    reply=scenario==='existing'?{subscribed:true,alreadySubscribed:true}:scenario==='malformed'?{}:{subscribed:true,confirmationSent:true};status=scenario==='server'?503:200;
    // the host asks for its bot check: the form passes it out of sight and posts again
    challenge=scenario.startsWith('check');
    verdict=scenario==='check-failed'?'failed':scenario==='check-closed'?'cancelled':'passed';
    const before=calls.length;
    await input.fill('verification@example.invalid');await button.click();
    if(scenario==='check-closed'){
     // closing the check leaves the form as it was: no message, the address kept, nothing posted again
     await page.waitForFunction(()=>!document.querySelector('#beta-form button').disabled&&!document.querySelector('#beta-form button').hasAttribute('aria-busy'));
     if(await message.evaluate(m=>m.dataset.state??'')||await message.textContent())throw Error('Closing the check showed a message');
     if(await input.inputValue()!=='verification@example.invalid')throw Error('Closing the check lost the address');
     if(calls.length-before!==1)throw Error('Closing the check posted again');
     continue;
    }
    const good=['new','existing','checked'].includes(scenario);
    await page.waitForFunction(g=>document.querySelector('#beta-form .form-status').dataset.state===(g?'success':'error'),good);
    if(!await message.textContent())throw Error('No status');
    if((await input.inputValue()==='')!==good)throw Error('Address retention');
    if(scenario==='check-failed'&&calls.length-before!==1)throw Error('A failed check posted again');
    if(scenario==='checked'){
     if(calls.length-before!==2)throw Error(`The bot check was not answered: ${calls.length-before} subscribe request(s)`);
     // quiet=1 asks the host for the page that shows Turnstile only when the reader must act
     if(!checks.length||!checks.every(s=>s.startsWith('?quiet=1')))throw Error(`The bot check was not asked for quietly: ${JSON.stringify(checks)}`);
    }
   }
   if(calls.some(c=>c.url!=='https://api.mosspub.com/api/sites/landing/subscribe'||c.body.scope!==locale.replace('/','')||c.body.email!=='verification@example.invalid'))throw Error('Wrong subscription contract');
   console.log(engine.name(),locale||'en','focus, validation, new/existing subscriber, malformed/server error, bot check passed, failed and closed: PASS (mocked; no subscriber created)');
   await page.close();
  }
 } finally {await browser.close();}
}
} finally { await close(); }
