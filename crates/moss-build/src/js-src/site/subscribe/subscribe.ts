/**
 * moss-hosted subscribe form: its state machine, bound by forms.ts.
 *
 * Intercepts submit on a `form.moss-subscribe-form[data-moss-hosted="true"]`,
 * POSTs JSON to the form's action URL, and drives the form's `data-state`
 * attribute through `idle → loading → success | error`. All visual transitions
 * (button morph, input fade, status slide-in) are handled by CSS selectors
 * keyed on that attribute — this module is presentation-agnostic aside from
 * the DOM shape it expects.
 *
 * With no script the browser posts the form itself, and the service answers
 * in JSON: a one-line form is subscribed when no bot check is asked, while a
 * form with questions loses its answers and a bot check cannot be shown.
 */

import { COPY, langBucket, type Lang, type SubscribeCopy } from './i18n.js';
import { claim, inPreview, lockSlotWidth, setState } from './hosted-form.js';
import { solveCaptcha } from './captcha.js';
import { clearField, markField } from '../forms/fields.js';
import { answersOf, checkQuestions, showAddress, watchQuestions } from './questions.js';

const POST_MS = 30000;

type StatusKey = 'check-email' | 'error';

type SubscribeResponse = {
  subscribed?: boolean;
  confirmationSent?: boolean;
  error?: string;
  code?: string;
  need_captcha?: boolean;
  captcha_url?: string;
};

/**
 * Hydrate one form. Safe to call multiple times on the same element —
 * `claim` prevents double-binding.
 */
export function hydrateSubscribeForm(form: HTMLFormElement): void {
  const input = form.querySelector<HTMLInputElement>('input[type="email"]');
  const btn = form.querySelector<HTMLButtonElement>('button[type="submit"]');
  // Only mark the form hydrated once the required DOM is confirmed — otherwise
  // a malformed form (missing input/button) would be flagged as done without
  // any listener bound, and a later retry would early-return.
  if (!input || !btn) return;
  if (!claim(form)) return;


  const lang: Lang = langBucket(document.documentElement.lang);
  const copy: SubscribeCopy = COPY[lang];

  // Rust fills the copy in server-side; overwrite it so it follows the page's
  // lang where that differs (a zh-hans article on a mixed-language site),
  // except a field the author wrote: `:::subscribe {button="..."
  // placeholder="..."}` marks each one it sets, independently.
  // A form that asks questions labels its email field, so it needs no placeholder.
  const asks = form.closest('.moss-form') !== null;
  if (form.dataset.placeholderOverride !== 'true' && !asks) input.placeholder = copy.placeholder;
  const labelEl = btn.querySelector<HTMLElement>('.moss-btn__label');
  if (labelEl && form.dataset.buttonOverride !== 'true') labelEl.textContent = copy.label;
  const subscribedEl = form.querySelector<HTMLElement>('.moss-subscribe-subscribed');
  if (subscribedEl) subscribedEl.textContent = copy.subscribed;
  // A form with questions is checked by moss before it is sent.
  if (asks) watchQuestions(form);

  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    if (form.dataset.state === 'loading') return;

    const email = (input.value ?? '').trim();
    if (asks ? !checkQuestions(form) : !email) {
      if (!asks) input.focus();
      return;
    }

    // Lock the slot's width to the button's natural idle width so the input's
    // flex calculation never sees the button change size. Measured once per
    // form; all subsequent submits reuse the same slot width.
    lockSlotWidth(form, btn);

    setState(form, 'loading', btn);

    // Preview mode: run the real success animation locally with no network
    // POST (the form's action is the production subscribe endpoint, so a
    // localhost submit would create a real subscriber). ship.rs strips
    // data-moss-preview on publish, so this branch is dead on the live site.
    if (inPreview()) {
      window.setTimeout(() => {
        // Mirror the real success path (status copy + state), so preview
        // shows the same confirmation the live site does.
        showStatus(form, 'check-email', copy);
        if (asks) showAddress(form, email);
        setState(form, 'success', btn);
      }, 600);
      return;
    }

    // `return` brings the reader back here once the mailed link signs this browser in.
    const jsonBody = {
      email,
      scope: form.querySelector<HTMLInputElement>('input[name="scope"]')?.value,
      lang,
      return: location.pathname + location.search + location.hash,
      ...(asks ? { answers: answersOf(form) } : {}),
    };

    const post = async (): Promise<[Response, SubscribeResponse]> => {
      const res = await fetch(form.action, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
        body: JSON.stringify(jsonBody),
        signal: AbortSignal.timeout(POST_MS),
      });
      return [res, await res.json().catch(() => ({}))];
    };

    try {
      let [res, data] = await post();
      // The service asks for its bot check only when a request looks like
      // abuse; the button keeps spinning while it runs. Two checks a press at most.
      for (let checks = 0; res.status === 403 && data.need_captcha && data.captcha_url; checks++) {
        if (checks === 2) {
          showStatus(form, 'error', copy, copy.verifyFailed);
          setState(form, 'error', btn);
          return;
        }
        if (!(await solveCaptcha(data.captcha_url))) {
          setState(form, 'idle', btn);
          return;
        }
        [res, data] = await post();
      }
      // The service's own address check marks the field as the browser's would.
      const emailField = form.querySelector<HTMLElement>('[data-field="email"]');
      if (res.status === 400 && data.code === 'invalid_email' && asks && emailField) {
        clearField(emailField);
        markField(form, emailField, 'email');
        input.focus();
        setState(form, 'idle', btn);
        return;
      }
      // A 2xx that does not say the address is subscribed (an empty or
      // unreadable body, a proxy's page) is not a subscription.
      if (!res.ok || data.subscribed !== true || data.confirmationSent !== true) {
        showStatus(form, 'error', copy);
        setState(form, 'error', btn);
        return;
      }
      showStatus(form, 'check-email', copy);
      if (asks) showAddress(form, email);
      setState(form, 'success', btn);
    } catch {
      showStatus(form, 'error', copy);
      setState(form, 'error', btn);
    }
  });

  // Success stays up until the reader acts: tap the circle (clears the field)
  // or type again (keeps what they typed).
  btn.addEventListener('click', (e) => {
    if (form.dataset.state !== 'success' || asks) return;
    e.preventDefault(); // always prevent submit from re-firing on success
    input.value = '';
    setState(form, 'idle');
  });
  input.addEventListener('input', () => {
    if (form.dataset.state === 'success') setState(form, 'idle');
  });
}

/**
 * Swap which status span is visible and set its text from the copy table.
 * The Rust renderer seeds each span with server-rendered text, but we
 * overwrite it here so the lang used client-side wins.
 */
function showStatus(
  form: HTMLFormElement,
  key: StatusKey,
  copy: SubscribeCopy,
  text?: string,
): void {
  const container = form.querySelector<HTMLElement>('.moss-subscribe-status');
  if (!container) return;
  const spans = container.querySelectorAll<HTMLElement>('[data-copy]');
  spans.forEach((span) => {
    const k = span.dataset.copy;
    if (k === key) {
      span.hidden = false;
      span.textContent = text ?? copyFor(key, copy);
    } else {
      span.hidden = true;
    }
  });
  container.dataset.state = key;
  // A form that asks questions is replaced by its status on success, so the
  // status takes the focus the pressed button had; the one-line form keeps it.
  if (key === 'check-email' && form.closest('.moss-form')) container.focus();
}

function copyFor(key: StatusKey, copy: SubscribeCopy): string {
  return key === 'check-email' ? copy.checkEmail : copy.error;
}
