// The landing's sign-up is moss's own subscribe form. This walks every state a
// reader can reach and checks what each looks like, not only what it says:
// idle, loading (spinner shown, label hidden, button width held), success and
// already subscribed (the button becomes a circle with a check, the message
// takes the field's row), error (server refusal and a failed connection, which
// is how a refusal without CORS headers reaches the page), and the host's bot
// check (passed, failed and closed). Every request is mocked; no subscriber is
// created. In Chromium and WebKit, with reduced motion, plus one pass with
// motion on, where the spinner must actually turn.
//   node scripts/check-landing-subscription.mjs [url-or-build-dir] [--shots <dir>]
import { execFileSync } from 'node:child_process';
import { mkdir, readFile } from 'node:fs/promises';
import { loadPlaywright, resolveBaseURL } from './landing-harness.mjs';
import { readFocus, focusProblem, readFieldButton, fieldButtonProblem, readMarks, markProblem, hitMisses } from '../tests/support/form-marks.mjs';

const args = process.argv.slice(2);
const shotsAt = args.includes('--shots') ? args[args.indexOf('--shots') + 1] : null;
const target = args.find((a, i) => !a.startsWith('--') && args[i - 1] !== '--shots');
const { baseURL, close } = await resolveBaseURL(target);
const { chromium, webkit } = await loadPlaywright();
if (shotsAt) await mkdir(shotsAt, { recursive: true });

const ENDPOINT = 'https://api.mosspub.com/api/sites/landing/subscribe';
const CAPTCHA = 'https://api.mosspub.com/captcha/';
const EMAIL = 'verification@example.invalid';
const fail = (message) => { throw new Error(message); };
// The landing as it was before its form became moss's (the last commit with closing.js's own submit).
const OLD_HTML = execFileSync('git', ['show', '3211bf2c8:site/index.html'], { cwd: new URL('..', import.meta.url), encoding: 'utf8' });

// The messages come from moss's own copy table; read it rather than restate it.
const copySource = await readFile(new URL('../crates/moss-build/src/js-src/site/subscribe/i18n.ts', import.meta.url), 'utf8');
const COPY = Object.fromEntries(['en', 'zh-hans', 'zh-hant'].map((lang) => {
  const block = copySource.slice(copySource.indexOf(lang === 'en' ? '  en: {' : `  '${lang}': {`)).split('\n  },')[0];
  const field = (name) => block.match(new RegExp(`${name}: (['"])(.*?)\\1,`))?.[2];
  return [lang, { 'check-email': field('checkEmail'), 'already-subscribed': field('alreadySubscribed'), error: field('error') }];
}));

/** One page with the network mocked; `next` sets how the next subscribe request is answered. */
async function openForm(browser, locale, { reducedMotion = 'reduce', width = 1280, colorScheme = 'dark' } = {}) {
  const page = await browser.newPage({ reducedMotion, colorScheme, viewport: { width, height: width < 600 ? 844 : 900 } });
  const mock = { calls: [], checks: [], tracked: [], answer: { status: 200, body: { subscribed: true, confirmationSent: true } }, challenge: false, verdict: 'passed', held: null, holdCaptcha: false };
  await page.route('https://api.mosspub.com/**', async (route) => {
    const url = new URL(route.request().url());
    if (/\/beacon$/.test(url.pathname)) {
      const event = JSON.parse(route.request().postData() || '{}').event;
      if (event) mock.tracked.push(event);
      return route.fulfill({ status: 204 });
    }
    if (url.pathname === '/captcha/') {
      mock.checks.push(url.search);
      const report = `<script>parent.postMessage({moss_captcha:"${mock.holdCaptcha ? 'interactive' : mock.verdict}"},"*")</script>`;
      return route.fulfill({ contentType: 'text/html; charset=utf-8', body: `<!doctype html><body style="margin:0;font:14px system-ui">Verifying…${report}</body>` });
    }
    mock.calls.push({ url: route.request().url(), body: JSON.parse(route.request().postData() || '{}') });
    if (mock.challenge) {
      mock.challenge = false;
      return route.fulfill({ status: 403, contentType: 'application/json', body: JSON.stringify({ need_captcha: true, captcha_url: CAPTCHA }) });
    }
    if (mock.held) await mock.held;
    if (mock.answer === 'offline') return route.abort('failed');
    return route.fulfill({ status: mock.answer.status, contentType: 'application/json', body: JSON.stringify(mock.answer.body) });
  });
  await page.goto(new URL(locale, baseURL).href);
  await page.evaluate(() => window.mossLanding?.openSignup());
  await page.waitForFunction(() => { const e = document.querySelector('#beta-email'); return !!e && !e.closest('[inert]'); }, null, { timeout: 20000 });
  return { page, mock };
}

/** What the form shows now, read from the computed style the reader sees. */
const look = (page) => page.evaluate(() => {
  const form = document.querySelector('#beta-form');
  const q = (s) => form.querySelector(s);
  const style = (el, pseudo) => getComputedStyle(el, pseudo);
  const button = q('.moss-btn');
  const shown = [...form.querySelectorAll('.moss-subscribe-status [data-copy]')].filter((s) => !s.hidden);
  return {
    state: form.dataset.state,
    label: Number(style(q('.moss-btn__label')).opacity),
    spinner: Number(style(q('.moss-btn__spinner')).opacity),
    spin: style(q('.moss-btn__spinner'), '::after').animationName,
    check: Number(style(q('.moss-btn__check')).opacity),
    radius: style(button).borderTopLeftRadius,
    buttonWidth: button.getBoundingClientRect().width,
    slotWidth: q('.moss-btn-slot').getBoundingClientRect().width,
    input: Number(style(q('.moss-input')).opacity),
    buttonBg: style(button).backgroundColor,
    buttonHeight: button.getBoundingClientRect().height,
    inputHeight: q('.moss-input').getBoundingClientRect().height,
    statusFont: style(q('.moss-subscribe-status')).fontSize,
    statusInRow: (() => { const a = q('.moss-subscribe-status').getBoundingClientRect(), b = q('.moss-btn-slot').getBoundingClientRect(); return a.top < b.bottom && a.bottom > b.top; })(),
    status: Number(style(q('.moss-subscribe-status')).opacity),
    statusColor: style(q('.moss-subscribe-status')).color,
    message: shown.map((s) => s.dataset.copy + ':' + s.textContent.trim()),
    ariaDisabled: button.getAttribute('aria-disabled'),
    email: q('.moss-input').value,
    running: document.getAnimations().filter((a) => a.playState === 'running' && a.animationName === 'moss-btn-spin').length,
  };
});

/** The mark beside the message is round and one text line across; points around the circle's centre still hit the button. */
async function roundMark(page, where, kind, reach) {
  const problem = markProblem(kind, await page.evaluate(readMarks, '#beta-form'));
  if (problem) fail(`${where}: ${problem}`);
  if (reach) {
    const misses = await page.evaluate(hitMisses, { form: '#beta-form', reach });
    if (misses) fail(`${where}: ${misses} of 8 points ${reach}px from the circle's centre miss the button`);
  }
}

/** Focus is one treatment on the field's own edge, and the field and button are one height with their text on one line. */
async function oneFocus(page, where) {
  const problem = focusProblem(await page.evaluate(readFocus, { field: '#beta-email', ground: 'rgb(16, 17, 16)' })) || fieldButtonProblem(await page.evaluate(readFieldButton, '#beta-form'));
  if (problem) fail(`${where}: ${problem}`);
}

/** Sends the form and returns once the request has left, so a state read after it is this send's. */
async function submit(page, mock, answer) {
  mock.answer = answer;
  const sent = mock.calls.length;
  await page.locator('#beta-email').fill(EMAIL);
  await page.locator('#beta-form .moss-btn').click();
  for (const start = Date.now(); mock.calls.length === sent; await page.waitForTimeout(20)) {
    if (Date.now() - start > 10000) fail('The form sent nothing');
  }
}

/** Waits for the state, then for its transitions to settle, so a read sees what the reader sees. */
async function waitState(page, state) {
  await page.waitForFunction((s) => document.querySelector('#beta-form').dataset.state === s, state, { timeout: 10000 });
  await page.waitForFunction(() => document.querySelector('#beta-form').getAnimations({ subtree: true }).every((a) => a.animationName === 'moss-btn-spin' || a.playState !== 'running'), null, { timeout: 5000 });
}

async function shot(page, name) {
  if (!shotsAt) return;
  // The status row hangs below the form, outside #beta's box: take it in.
  await page.locator('#beta').scrollIntoViewIfNeeded();
  const box = await page.locator('#beta').boundingBox();
  const height = Math.min(box.height + 64, page.viewportSize().height - Math.max(0, box.y));
  await page.screenshot({ path: `${shotsAt}/${name}.png`, animations: 'allow', clip: { x: box.x, y: Math.max(0, box.y), width: box.width, height } });
}

/** After a success, back to idle the way a reader does it: tap the circle. */
async function reset(page) {
  if (await page.evaluate(() => document.querySelector('#beta-form').dataset.state) === 'success') {
    await page.locator('#beta-form .moss-btn').click();
    await waitState(page, 'idle');
  }
}

try {
  for (const engine of [chromium, webkit]) {
    const browser = await engine.launch();
    try {
      // A page cached from before the sign-up became moss's form, run against
      // today's scripts: its form must stay hidden, never a form that posts the
      // address in a URL.
      {
        const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
        await page.route('https://api.mosspub.com/**', (route) => route.fulfill({ status: 204 }));
        await page.route(new URL('/', baseURL).href, (route) => route.fulfill({ contentType: 'text/html; charset=utf-8', body: OLD_HTML }));
        await page.goto(new URL('/', baseURL).href);
        const old = await page.evaluate(() => getComputedStyle(document.querySelector('#beta-form')).visibility);
        if (old !== 'hidden') fail(`${engine.name()}: a cached old page shows a form nothing submits (visibility ${old})`);
        console.log(engine.name(), 'cached old page: form stays hidden: PASS');
        await page.close();
      }
      for (const locale of ['', 'zh-hans/', 'zh-hant/']) {
        const { page, mock } = await openForm(browser, locale);
        const idle = await look(page);
        if (idle.state !== 'idle' || idle.label !== 1 || idle.spinner !== 0) fail(`${locale || 'en'}: not idle at rest: ${JSON.stringify(idle)}`);
        // the landing's own resting look: a white button, controls 44px tall, a 13px status line
        if (idle.buttonBg !== 'rgb(255, 255, 255)' || Math.round(idle.buttonHeight) !== 44 || Math.round(idle.inputHeight) !== 44 || idle.statusFont !== '13px') fail(`${locale || 'en'}: the landing's resting look is lost: ${JSON.stringify(idle)}`);
        const copy = COPY[locale.replace('/', '') || 'en'];
        const says = (state, key) => state.message[0] === `${key}:${copy[key]}` || fail(`${locale || 'en'}: expected "${copy[key]}", saw ${JSON.stringify(state.message)}`);

        await oneFocus(page, `${locale || 'en'} idle`);
        await page.locator('#beta-email').fill('invalid');
        await page.locator('#beta-form .moss-btn').click();
        if (mock.calls.length) fail('An invalid address was posted');

        // loading: hold the request open and look at the button mid-send
        let release;
        mock.held = new Promise((r) => { release = r; });
        await submit(page, mock, { status: 200, body: { subscribed: true, confirmationSent: true } });
        await waitState(page, 'loading');
        const loading = await look(page);
        if (loading.spinner !== 1 || loading.label !== 0) fail(`Loading shows no spinner: ${JSON.stringify(loading)}`);
        if (loading.spin !== 'moss-btn-spin') fail(`The spinner has no spin: ${loading.spin}`);
        if (Math.abs(loading.buttonWidth - idle.buttonWidth) > 0.5) fail(`The button changed width while loading: ${idle.buttonWidth} -> ${loading.buttonWidth}`);
        if (loading.ariaDisabled !== 'true' || loading.input >= 1) fail(`Loading does not hold the form: ${JSON.stringify(loading)}`);
        await shot(page, `${engine.name()}-${locale.replace('/', '') || 'en'}-loading`);
        release(); mock.held = null;

        // success: the circle with its check, the message in the field's row
        await waitState(page, 'success');
        const success = await look(page);
        if (success.check !== 1 || success.radius !== '50%' || success.label !== 0) fail(`Success is not the check circle: ${JSON.stringify(success)}`);
        if (success.status !== 1 || !success.statusInRow) fail(`The success message is not in the button's row: ${JSON.stringify(success)}`);
        says(success, 'check-email');
        await roundMark(page, `${locale || 'en'} success`, 'circle');
        if (success.slotWidth !== loading.slotWidth) fail('The button slot moved on success');
        // tapping the circle returns to idle with an empty field, and is not counted as a sign-up
        await page.waitForTimeout(300); // the send's own beacon settles first
        const tracked = mock.tracked.length;
        await page.locator('#beta-form .moss-btn').click();
        await waitState(page, 'idle');
        if ((await look(page)).email !== '') fail('Tapping the circle kept the address');
        await page.waitForTimeout(300);
        if (mock.tracked.length !== tracked) fail(`Tapping the circle was counted as a sign-up: ${JSON.stringify(mock.tracked.slice(tracked))}`);
        if (!mock.tracked.includes('subscribe-submit')) fail('A sign-up was not counted');

        await submit(page, mock, { status: 200, body: { subscribed: true, alreadySubscribed: true } });
        await waitState(page, 'success');
        const already = await look(page);
        says(already, 'already-subscribed');
        await roundMark(page, `${locale || 'en'} already subscribed`, 'circle');
        await reset(page);

        // errors: a refusal, and a request that never answers in a way the page can read
        // a 2xx that does not say the address is subscribed is an error too
        for (const answer of [{ status: 503, body: {} }, 'offline', { status: 200, body: {} }]) {
          await submit(page, mock, answer);
          await waitState(page, 'error');
          const error = await look(page);
          if (error.label !== 1 || error.spinner !== 0 || error.status !== 1) fail(`Error state wrong for ${JSON.stringify(answer)}: ${JSON.stringify(error)}`);
          says(error, 'error');
          await roundMark(page, `${locale || 'en'} error`, 'icon');
          if (error.statusColor === idle.statusColor) fail('The error message is not in the error colour');
          if (error.email !== EMAIL) fail('An error lost the address');
          await reset(page);
        }

        // the host's bot check: passed posts again, failed is an error, closed leaves the form as it was
        const after = { passed: 'success', failed: 'error', cancelled: 'idle' };
        for (const verdict of Object.keys(after)) {
          mock.challenge = true; mock.verdict = verdict;
          const before = mock.calls.length;
          await submit(page, mock, { status: 200, body: { subscribed: true, confirmationSent: true } });
          await waitState(page, after[verdict]);
          const posts = mock.calls.length - before;
          if (posts !== (verdict === 'passed' ? 2 : 1)) fail(`Bot check ${verdict}: ${posts} subscribe request(s)`);
          if (verdict !== 'passed' && (await look(page)).email !== EMAIL) fail(`Bot check ${verdict} lost the address`);
          await reset(page);
        }
        if (!mock.checks.length || !mock.checks.every((s) => s.startsWith('?quiet=1'))) fail(`The bot check was not asked for quietly: ${JSON.stringify(mock.checks)}`);

        const scope = locale.replace('/', '');
        if (mock.calls.some((c) => c.url !== ENDPOINT || c.body.scope !== scope || c.body.email !== EMAIL || !c.body.lang)) fail(`Wrong subscription contract: ${JSON.stringify(mock.calls.slice(-1))}`);
        console.log(engine.name(), locale || 'en', 'idle, loading (spinner, width held), success, already subscribed, server and connection errors, bot check passed, failed and closed: PASS (mocked; no subscriber created)');
        await page.close();
      }

      // the same marks and focus at phone and desktop width, in both colour schemes
      for (const width of [390, 1280]) for (const colorScheme of ['light', 'dark']) {
        const at = `${width}px ${colorScheme}`;
        const { page, mock } = await openForm(browser, '', { width, colorScheme });
        await oneFocus(page, `${at} idle`);
        await submit(page, mock, { status: 200, body: { subscribed: true, confirmationSent: true } });
        await waitState(page, 'success');
        await roundMark(page, `${at} success`, 'circle', width < 768 ? 21 : 11);
        await reset(page);
        await submit(page, mock, { status: 503, body: {} });
        await waitState(page, 'error');
        await roundMark(page, `${at} error`, 'icon');
        await page.close();
      }
      console.log(engine.name(), 'success circle and error icon round and one line across, one focus treatment, field and button one height, at 390 and 1280 in light and dark: PASS');

      // motion on: the spinner turns, and the circle draws its check
      const { page, mock } = await openForm(browser, '', { reducedMotion: 'no-preference' });
      let release;
      mock.held = new Promise((r) => { release = r; });
      await submit(page, mock, { status: 200, body: { subscribed: true, confirmationSent: true } });
      await waitState(page, 'loading');
      if ((await look(page)).running < 1) fail(`${engine.name()}: the spinner is not turning with motion on`);
      // mid-morph the pill is still shrinking around a label wider than it (transparent after loading, but not display:none): the pill must still clip.
      // Sampled inside the page, 50ms after the state flips, so the timing is the page's own.
      await page.evaluate(() => {
        const form = document.querySelector('#beta-form');
        new MutationObserver((_, observer) => {
          if (form.dataset.state !== 'success') return;
          observer.disconnect();
          setTimeout(() => {
            const button = form.querySelector('.moss-btn'), label = button.querySelector('.moss-btn__label');
            const b = button.getBoundingClientRect(), l = label.getBoundingClientRect();
            window.__morph = { spills: l.left < b.left - 0.5 || l.right > b.right + 0.5, overflow: getComputedStyle(button).overflowX };
          }, 50);
        }).observe(form, { attributes: true, attributeFilter: ['data-state'] });
      });
      release(); mock.held = null;
      await page.waitForFunction(() => window.__morph, null, { timeout: 10000 });
      const morph = await page.evaluate(() => window.__morph);
      if (!morph.spills) fail(`${engine.name()}: the label is no longer wider than the shrinking pill, so this sample proves nothing: ${JSON.stringify(morph)}`);
      if (morph.overflow !== 'hidden') fail(`${engine.name()}: the label spills out of the shrinking pill (overflow ${morph.overflow})`);
      await waitState(page, 'success');
      if ((await page.evaluate(() => getComputedStyle(document.querySelector('#beta-form .moss-btn')).overflowX)) !== 'visible') fail(`${engine.name()}: the circle's hit area is still clipped after the morph`);
      await page.waitForFunction(() => getComputedStyle(document.querySelector('#beta-form .moss-btn__check')).opacity === '1', null, { timeout: 5000 });
      console.log(engine.name(), 'motion on: spinner turning, check drawn: PASS');
      await page.close();

      if (shotsAt) {
        for (const width of [390, 1280]) for (const colorScheme of ['light', 'dark']) {
          const { page, mock } = await openForm(browser, '', { width, colorScheme });
          const name = (s) => `${engine.name()}-${width}-${colorScheme}-${s}`;
          await shot(page, name('idle'));
          let release; mock.held = new Promise((r) => { release = r; });
          await submit(page, mock, { status: 200, body: { subscribed: true, confirmationSent: true } });
          await waitState(page, 'loading'); await shot(page, name('loading'));
          release(); mock.held = null; await waitState(page, 'success'); await shot(page, name('success'));
          await reset(page);
          await submit(page, mock, { status: 200, body: { subscribed: true, alreadySubscribed: true } });
          await waitState(page, 'success'); await shot(page, name('already-subscribed'));
          await reset(page);
          await submit(page, mock, { status: 503, body: {} });
          await waitState(page, 'error'); await shot(page, name('error'));
          await reset(page);
          mock.challenge = true; mock.holdCaptcha = true;
          await submit(page, mock, { status: 200, body: {} });
          await page.waitForSelector('iframe[src^="https://api.mosspub.com/captcha/"]', { state: 'attached', timeout: 10000 });
          await page.waitForTimeout(1200);
          await page.screenshot({ path: `${shotsAt}/${name('captcha')}.png` });
          await page.close();
        }
      }
    } finally { await browser.close(); }
  }
} finally { await close(); }
