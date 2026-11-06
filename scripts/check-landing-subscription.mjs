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
import { mkdir, readFile } from 'node:fs/promises';
import { loadPlaywright, resolveBaseURL } from './landing-harness.mjs';

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
  const mock = { calls: [], checks: [], answer: { status: 200, body: { subscribed: true, confirmationSent: true } }, challenge: false, verdict: 'passed', held: null, holdCaptcha: false };
  await page.route('https://api.mosspub.com/**', async (route) => {
    const url = new URL(route.request().url());
    if (/\/beacon$/.test(url.pathname)) return route.fulfill({ status: 204 });
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
    await page.locator('#beta-form .moss-btn').click({ force: true });
    await waitState(page, 'idle');
  }
}

try {
  for (const engine of [chromium, webkit]) {
    const browser = await engine.launch();
    try {
      for (const locale of ['', 'zh-hans/', 'zh-hant/']) {
        const { page, mock } = await openForm(browser, locale);
        const idle = await look(page);
        if (idle.state !== 'idle' || idle.label !== 1 || idle.spinner !== 0) fail(`${locale || 'en'}: not idle at rest: ${JSON.stringify(idle)}`);
        // the landing's own resting look: a white button, controls 44px tall, a 13px status line
        if (idle.buttonBg !== 'rgb(255, 255, 255)' || Math.round(idle.buttonHeight) !== 44 || Math.round(idle.inputHeight) !== 44 || idle.statusFont !== '13px') fail(`${locale || 'en'}: the landing's resting look is lost: ${JSON.stringify(idle)}`);
        const copy = COPY[locale.replace('/', '') || 'en'];
        const says = (state, key) => state.message[0] === `${key}:${copy[key]}` || fail(`${locale || 'en'}: expected "${copy[key]}", saw ${JSON.stringify(state.message)}`);

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
        if (success.slotWidth !== loading.slotWidth) fail('The button slot moved on success');
        // tapping the circle returns to idle with an empty field (the circle keeps
        // moss's aria-disabled from the send, so the tap is forced)
        await page.locator('#beta-form .moss-btn').click({ force: true });
        await waitState(page, 'idle');
        if ((await look(page)).email !== '') fail('Tapping the circle kept the address');

        await submit(page, mock, { status: 200, body: { subscribed: true, alreadySubscribed: true } });
        await waitState(page, 'success');
        says(await look(page), 'already-subscribed');
        await reset(page);

        // errors: a refusal, and a request that never answers in a way the page can read
        for (const answer of [{ status: 503, body: {} }, 'offline']) {
          await submit(page, mock, answer);
          await waitState(page, 'error');
          const error = await look(page);
          if (error.label !== 1 || error.spinner !== 0 || error.status !== 1) fail(`Error state wrong for ${JSON.stringify(answer)}: ${JSON.stringify(error)}`);
          says(error, 'error');
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

      // motion on: the spinner turns, and the circle draws its check
      const { page, mock } = await openForm(browser, '', { reducedMotion: 'no-preference' });
      let release;
      mock.held = new Promise((r) => { release = r; });
      await submit(page, mock, { status: 200, body: { subscribed: true, confirmationSent: true } });
      await waitState(page, 'loading');
      if ((await look(page)).running < 1) fail(`${engine.name()}: the spinner is not turning with motion on`);
      release(); mock.held = null;
      await waitState(page, 'success');
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
