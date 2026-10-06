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

import { APPLY_COPY, COPY, langBucket, type ApplyCopy, type Lang, type SubscribeCopy } from './i18n.js';

type FormState = 'idle' | 'loading' | 'success' | 'error';

type SubscribeResponse = {
  subscribed?: boolean;
  confirmationSent?: boolean;
  alreadySubscribed?: boolean;
  error?: string;
};

/** How long success stays up before auto-reverting to idle. */
const AUTO_REVERT_MS = 4000;

/** Per-form auto-revert timers, keyed by form element. */
const revertTimers = new WeakMap<HTMLFormElement, number>();

/**
 * Forms whose submit listener is already bound. A WeakSet keyed on the
 * element — not a `data-moss-hydrated` attribute — because idiomorph strips
 * any attribute the runtime added that isn't present in the freshly rendered
 * HTML: a dataset flag would vanish on the very node it was guarding, and a
 * post-morph `autoHydrate()` sweep would bind a second submit listener onto
 * the same (idiomorph-preserved) form element, double-POSTing on submit.
 */
const hydratedForms = new WeakSet<HTMLFormElement>();

/**
 * Hydrate one form. Safe to call multiple times on the same element —
 * the `hydratedForms` guard prevents double-binding.
 */
export function hydrateSubscribeForm(form: HTMLFormElement): void {
  if (hydratedForms.has(form)) return;

  const input = form.querySelector<HTMLInputElement>('input[type="email"]');
  const btn = form.querySelector<HTMLButtonElement>('button[type="submit"]');
  // Only mark the form hydrated once the required DOM is confirmed — otherwise
  // a malformed form (missing input/button) would be flagged as done without
  // any listener bound, and a later retry would early-return.
  if (!input || !btn) return;
  hydratedForms.add(form);

  const lang: Lang = langBucket(document.documentElement.lang);
  const copy: SubscribeCopy = COPY[lang];
  const applyCopy: ApplyCopy = APPLY_COPY[lang];

  // Is this an apply form (terminal success, FormData body)?
  const isApply = form.dataset.position === 'apply';
  // Does this form opt out of auto-revert? (apply forms set data-revert="false")
  const revertEnabled = form.dataset.revert !== 'false';

  // Apply copy — Rust already filled these in server-side, but we overwrite
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
  if (!isApply && !hasButtonOverride) {
    input.placeholder = copy.placeholder;
  }
  if (!hasButtonOverride && !isApply) {
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
    if (document.body.hasAttribute('data-moss-preview')) {
      window.setTimeout(() => {
        // Mirror the real success path (status copy + state), so preview
        // shows the same confirmation the live site does.
        showStatus(form, isApply ? 'received' : 'check-email', copy, applyCopy, isApply);
        setState(form, 'success', btn);
        if (revertEnabled) {
          scheduleAutoRevert(form, input);
        } else {
          freezeTerminal(form, btn, applyCopy.labelSuccess);
        }
      }, 600);
      return;
    }

    // Seam 1: Build the POST body.
    // - Subscribe forms (data-position != "apply"): keep the original JSON
    //   {email, scope} payload so the seta /subscribe endpoint (which expects
    //   JSON) and existing tests are byte-identical.
    // - Apply forms (data-position = "apply"): use URLSearchParams built from
    //   FormData so all fields (matters, scope, website honeypot) ride along
    //   naturally, and the no-JS native form POST uses the same encoding.
    let fetchBody: BodyInit;
    let contentType: string;
    if (isApply) {
      const formData = new FormData(form);
      const bodyParams = new URLSearchParams();
      for (const [k, v] of formData.entries()) {
        bodyParams.append(k, String(v));
      }
      fetchBody = bodyParams;
      contentType = 'application/x-www-form-urlencoded';
    } else {
      const scopeInput = form.querySelector<HTMLInputElement>('input[name="scope"]');
      const jsonBody: { email: string; scope?: string } = { email };
      if (scopeInput) {
        jsonBody.scope = scopeInput.value;
      }
      fetchBody = JSON.stringify(jsonBody);
      contentType = 'application/json';
    }

    try {
      const res = await fetch(form.action, {
        method: 'POST',
        headers: { 'Content-Type': contentType, Accept: 'application/json' },
        body: fetchBody,
      });
      const data: SubscribeResponse = await res.json().catch(() => ({}));
      if (!res.ok) {
        showStatus(form, 'error', copy, applyCopy, isApply);
        setState(form, 'error', btn);
        return;
      }
      if (isApply) {
        showStatus(form, 'received', copy, applyCopy, isApply);
        setState(form, 'success', btn);
        // Seam 2: terminal success — disable inputs + swap label, no auto-revert
        freezeTerminal(form, btn, applyCopy.labelSuccess);
      } else {
        const statusKey = data.alreadySubscribed ? 'already-subscribed' : 'check-email';
        showStatus(form, statusKey, copy, applyCopy, isApply);
        setState(form, 'success', btn);
        // Seam 2: auto-revert only when revert is enabled (subscribe forms)
        if (revertEnabled) {
          scheduleAutoRevert(form, input);
        }
      }
    } catch {
      showStatus(form, 'error', copy, applyCopy, isApply);
      setState(form, 'error', btn);
    }
  });

  // Tap the success circle to revert immediately — only for revertable forms.
  btn.addEventListener('click', (e) => {
    if (form.dataset.state !== 'success') return;
    e.preventDefault(); // always prevent submit from re-firing on success
    if (!revertEnabled) return; // apply forms: success is terminal — just prevent default
    revertToIdle(form, input);
  });
}

function setState(form: HTMLFormElement, state: FormState, btn?: HTMLButtonElement): void {
  form.dataset.state = state;
  // Seam 3 a11y: aria-disabled on the button during loading/success so
  // screen-readers know it's unavailable without visual CSS alone.
  // Resolve the button from the form when not passed — ensures revertToIdle
  // (which omits `btn`) never leaves aria-disabled stale on the live element.
  const resolvedBtn = btn ?? form.querySelector<HTMLButtonElement>('button[type="submit"]');
  if (resolvedBtn) {
    if (state === 'loading' || state === 'success') {
      resolvedBtn.setAttribute('aria-disabled', 'true');
    } else {
      resolvedBtn.removeAttribute('aria-disabled');
    }
  }
}

/**
 * Swap which status span is visible and set its text from the copy table.
 * The Rust renderer seeds each span with server-rendered text, but we
 * overwrite it here so the lang used client-side wins.
 */
function showStatus(
  form: HTMLFormElement,
  key: 'check-email' | 'already-subscribed' | 'error' | 'received',
  copy: SubscribeCopy,
  applyCopy: ApplyCopy,
  isApply: boolean,
): void {
  const container = form.querySelector<HTMLElement>('.moss-subscribe-status');
  if (!container) return;
  const spans = container.querySelectorAll<HTMLElement>('[data-copy]');
  spans.forEach((span) => {
    const k = span.dataset.copy;
    if (k === key) {
      span.hidden = false;
      span.textContent = copyFor(key, copy, applyCopy, isApply);
    } else {
      span.hidden = true;
    }
  });
  container.dataset.state = key;
  // Seam 3 a11y: on success, focus the status region so screen-readers announce it.
  // Guard tabIndex — the element needs a non-negative tabIndex (e.g. tabindex="-1")
  // to be programmably focusable; a plain div without one silently ignores focus().
  if (key === 'check-email' || key === 'already-subscribed' || key === 'received') {
    if (container.tabIndex >= 0) container.focus();
  }
}

function copyFor(
  key: 'check-email' | 'already-subscribed' | 'error' | 'received',
  copy: SubscribeCopy,
  applyCopy: ApplyCopy,
  isApply: boolean,
): string {
  if (isApply) {
    switch (key) {
      case 'received':
        return applyCopy.received;
      case 'error':
        return applyCopy.error;
      default:
        return '';
    }
  }
  switch (key) {
    case 'check-email':
      return copy.checkEmail;
    case 'already-subscribed':
      return copy.alreadySubscribed;
    case 'error':
      return copy.error;
    default:
      return '';
  }
}

/**
 * Freeze the apply form after terminal success: disable all visible inputs,
 * swap the button label to the data-label-success value.
 * No auto-revert is scheduled; the form remains in success state.
 */
function freezeTerminal(
  form: HTMLFormElement,
  btn: HTMLButtonElement,
  labelSuccess: string,
): void {
  // Disable all visible inputs except type=hidden (the scope input).
  // The website honeypot (type=text) IS included in this sweep — it's harmlessly
  // disabled alongside the real fields; only type=hidden inputs are excluded.
  const inputs = form.querySelectorAll<HTMLInputElement>('input:not([type="hidden"])');
  inputs.forEach((inp) => {
    inp.disabled = true;
  });
  // Swap the label to the success text
  const labelEl = btn.querySelector<HTMLElement>('.moss-btn__label');
  if (labelEl) {
    const successText = labelEl.dataset.labelSuccess ?? labelSuccess;
    labelEl.textContent = successText;
  }
}

/**
 * Freeze the button slot's width to the button's natural idle width, once.
 * The slot element (`.moss-btn-slot`) stays this width through every
 * state change, so the input never sees the button collapse into a circle and
 * cannot shift horizontally. The circle centers inside the slot via CSS, which
 * puts it on the same centerline as the original button.
 */
function lockSlotWidth(form: HTMLFormElement, btn: HTMLButtonElement): void {
  const slot = btn.parentElement;
  if (!slot || !slot.classList.contains('moss-btn-slot')) return;
  // The lock protects the ROW layout (input beside button): without it the
  // input's flex calculation sees the button collapse into a circle. In the
  // stacked column layout (mobile in-page card) the slot is its own stretched
  // row — a pixel lock there pins the circle to whatever width the form had
  // at submit time, off-center after any pane resize. Leave it fluid.
  if (getComputedStyle(form).flexDirection === 'column') {
    slot.style.width = '';
    return;
  }
  if (slot.style.width) return;
  // Zero offsetWidth means layout isn't ready (detached, display:none, jsdom).
  // Degrade gracefully: slot keeps its CSS flex:0 0 auto + min-height so the
  // morph still renders — it just can't pin width across the transition.
  const w = btn.offsetWidth;
  if (w <= 0) return;
  slot.style.width = `${w}px`;
}

/**
 * Start the auto-revert timer. Any existing timer for this form is cleared
 * first so rapid re-submits don't stack timers.
 */
function scheduleAutoRevert(form: HTMLFormElement, input: HTMLInputElement): void {
  clearAutoRevert(form);
  const id = window.setTimeout(() => revertToIdle(form, input), AUTO_REVERT_MS);
  revertTimers.set(form, id);
}

function clearAutoRevert(form: HTMLFormElement): void {
  const existing = revertTimers.get(form);
  if (existing !== undefined) {
    clearTimeout(existing);
    revertTimers.delete(form);
  }
}

/**
 * Return the form to idle from success. Clears the input value so the user can
 * type a new email. Safe to call from either the auto-revert timer or a click
 * on the success circle.
 */
function revertToIdle(form: HTMLFormElement, input: HTMLInputElement): void {
  clearAutoRevert(form);
  // Skip DOM work if the form has been detached — a stale timer firing on a
  // removed form would otherwise clobber a fresh hydrate on the same page.
  if (!form.isConnected) return;
  input.value = '';
  setState(form, 'idle');
}

/**
 * Auto-hydrate on load. The module is also exported so tests can drive it
 * explicitly without touching the document-level listener.
 */
function autoHydrate(): void {
  const forms = document.querySelectorAll<HTMLFormElement>(
    'form.moss-subscribe-form[data-moss-hosted="true"]',
  );
  forms.forEach(hydrateSubscribeForm);
}

if (typeof document !== 'undefined') {
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', autoHydrate);
  } else {
    autoHydrate();
  }
  // idiomorph preview updates patch the DOM without re-running inline
  // scripts, so a `:::subscribe` block that appears (or is reused) after a
  // morph never gets its submit listener bound unless we re-sweep here.
  // `autoHydrate` is safe to call repeatedly — `hydratedForms` skips forms
  // that are already bound.
  document.addEventListener('moss-morph-patched', autoHydrate);
}
