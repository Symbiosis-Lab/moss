/**
 * moss-hosted subscribe form — inline state machine.
 *
 * Intercepts submit on any `form.moss-subscribe-form[data-moss-hosted="true"]`,
 * POSTs JSON to the form's action URL, and drives the form's `data-state`
 * attribute through `idle → loading → success | error`. All visual transitions
 * (button morph, input fade, status slide-in) are handled by CSS selectors
 * keyed on that attribute — this module is presentation-agnostic aside from
 * the DOM shape it expects.
 *
 * No JS fallback: if the script fails to load, the browser's native form POST
 * still works. The user lands on the raw-JSON page — ugly but not broken.
 */

import { COPY, langBucket, type Lang, type SubscribeCopy } from './i18n.js';
import { autoHydrate, claim, inPreview, lockSlotWidth, setState } from './hosted-form.js';

type StatusKey = 'check-email' | 'already-subscribed' | 'error';

type SubscribeResponse = {
  subscribed?: boolean;
  confirmationSent?: boolean;
  alreadySubscribed?: boolean;
  error?: string;
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

  // Rust already filled the copy in server-side, but we overwrite
  // anyway so the form matches the page's detected lang when the Rust-emitted
  // lang doesn't match (e.g., a zh-hans article on a mixed-lang site).
  //
  // Exception: a `:::subscribe` shortcode may carry an author-supplied button
  // override (e.g., `button: Request access`). The renderer marks those with
  // `data-button-override="true"`; skip BOTH the placeholder and label
  // overwrite for them so the author's copy survives. (All moss-hosted forms
  // are `data-position="inline"` now — footer vs inline is a CSS concern — so
  // the override marker, not the position, is what gates the overwrite.)
  const hasButtonOverride = form.dataset.buttonOverride === 'true';
  if (!hasButtonOverride) {
    input.placeholder = copy.placeholder;
    const labelEl = btn.querySelector<HTMLElement>('.moss-btn__label');
    if (labelEl) labelEl.textContent = copy.label;
  }

  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    if (form.dataset.state === 'loading') return;

    const email = (input.value ?? '').trim();
    if (!email) {
      input.focus();
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
        setState(form, 'success', btn);
      }, 600);
      return;
    }

    const scopeInput = form.querySelector<HTMLInputElement>('input[name="scope"]');
    const jsonBody: { email: string; scope?: string } = { email };
    if (scopeInput) {
      jsonBody.scope = scopeInput.value;
    }

    try {
      const res = await fetch(form.action, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
        body: JSON.stringify(jsonBody),
      });
      const data: SubscribeResponse = await res.json().catch(() => ({}));
      if (!res.ok) {
        showStatus(form, 'error', copy);
        setState(form, 'error', btn);
        return;
      }
      showStatus(form, data.alreadySubscribed ? 'already-subscribed' : 'check-email', copy);
      setState(form, 'success', btn);
    } catch {
      showStatus(form, 'error', copy);
      setState(form, 'error', btn);
    }
  });

  // Success stays up until the reader acts: tap the circle (clears the field)
  // or type again (keeps what they typed).
  btn.addEventListener('click', (e) => {
    if (form.dataset.state !== 'success') return;
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
): void {
  const container = form.querySelector<HTMLElement>('.moss-subscribe-status');
  if (!container) return;
  const spans = container.querySelectorAll<HTMLElement>('[data-copy]');
  spans.forEach((span) => {
    const k = span.dataset.copy;
    if (k === key) {
      span.hidden = false;
      span.textContent = copyFor(key, copy);
    } else {
      span.hidden = true;
    }
  });
  container.dataset.state = key;
  // on success, focus the status region so screen-readers announce it.
  // Guard tabIndex — the element needs a non-negative tabIndex (e.g. tabindex="-1")
  // to be programmably focusable; a plain div without one silently ignores focus().
  if (key === 'check-email' || key === 'already-subscribed') {
    if (container.tabIndex >= 0) container.focus();
  }
}

function copyFor(key: StatusKey, copy: SubscribeCopy): string {
  switch (key) {
    case 'check-email':
      return copy.checkEmail;
    case 'already-subscribed':
      return copy.alreadySubscribed;
    case 'error':
      return copy.error;
  }
}

autoHydrate<HTMLFormElement>('form.moss-subscribe-form[data-moss-hosted="true"]', hydrateSubscribeForm);
