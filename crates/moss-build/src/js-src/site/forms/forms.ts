import { autoHydrate, claim, inPreview, lockSlotWidth, setState } from '../subscribe/hosted-form.js';
import { hydrateSubscribeForm } from '../subscribe/subscribe.js';
import { solveCaptcha } from '../subscribe/captcha.js';
import { clearField, controls, copy, markField } from './fields.js';

type Answer = {
  ok?: boolean;
  error?: string;
  fields?: Record<string, string>;
  need_captcha?: boolean;
  captcha_url?: string;
};

/** Seam for tests: jsdom cannot navigate. */
export const navigation = {
  to(url: string): void {
    window.location.assign(url);
  },
};

const root = (form: HTMLFormElement): HTMLElement => form.closest<HTMLElement>('.moss-form') ?? form;

const status = (form: HTMLFormElement, message: string): void => {
  const line = form.querySelector<HTMLElement>('.moss-form__status');
  if (line) line.textContent = message;
};

function clearErrors(form: HTMLFormElement): void {
  form.querySelectorAll<HTMLElement>('[data-field]').forEach(clearField);
  status(form, '');
}

/** Marks the refused fields, moves focus to the first, and returns whether any was found. */
function showFieldErrors(form: HTMLFormElement, fields: Record<string, string>): boolean {
  const refused: HTMLElement[] = [];
  form.querySelectorAll<HTMLElement>('[data-field]').forEach((box) => {
    const code = fields[box.dataset.field ?? ''];
    if (!code) return;
    markField(form, box, code === 'required' ? 'required' : 'format');
    refused.push(box);
  });
  const first = refused[0] && controls(refused[0])[0];
  if (first) first.focus();
  return refused.length > 0;
}

function body(form: HTMLFormElement): string {
  const params = new URLSearchParams();
  new FormData(form).forEach((value, key) => {
    if (typeof value === 'string') params.append(key, value);
  });
  return params.toString();
}

async function send(form: HTMLFormElement): Promise<void> {
  const next = form.dataset.next ?? '';
  clearErrors(form);
  setState(root(form), 'loading');
  const btn = form.querySelector<HTMLButtonElement>('button[type="submit"]');
  if (btn) lockSlotWidth(form, btn);

  // The preview's form has no live service behind it: show the whole flow.
  if (inPreview()) {
    window.setTimeout(() => navigation.to(next), 600);
    return;
  }

  const post = async (): Promise<[Response, Answer]> => {
    const res = await fetch(form.action, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded', Accept: 'application/json' },
      body: body(form),
    });
    return [res, await res.json().catch(() => ({}))];
  };
  try {
    let [res, answer] = await post();
    // The service asks for its bot check only when posts look like abuse. Two checks a press at most.
    for (let checks = 0; res.status === 403 && answer.need_captcha && answer.captcha_url && checks < 2; checks++) {
      if (!(await solveCaptcha(answer.captcha_url))) {
        setState(root(form), 'idle');
        return;
      }
      [res, answer] = await post();
    }
    if (res.ok && answer.ok !== false) {
      navigation.to(next);
      return;
    }
    setState(root(form), 'error');
    if (res.status === 422 && answer.fields && showFieldErrors(form, answer.fields)) return;
    if (res.status === 422) status(form, copy(form, 'invalid'));
    else if (res.status === 429) status(form, copy(form, 'rate_limited'));
    else if (res.status === 404) status(form, copy(form, 'unknown_form'));
    else status(form, copy(form, 'network'));
  } catch {
    setState(root(form), 'error');
    status(form, copy(form, 'network'));
  }
}

/**
 * Fills visible fields from the page's address (`?session=Family`). Sets a
 * value or ticks a matching choice and nothing else, so a link cannot reach
 * `_next` or any hidden input.
 */
export function prefill(form: HTMLFormElement, search: string): void {
  const params = new URLSearchParams(search);
  for (const name of new Set(params.keys())) {
    const values = params.getAll(name);
    const targets = Array.from(form.elements).filter(
      (el): el is HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement =>
        (el as HTMLInputElement).name === name &&
        (el as HTMLInputElement).type !== 'hidden',
    );
    for (const el of targets) {
      const type = (el as HTMLInputElement).type;
      if (type === 'radio' || type === 'checkbox') {
        if (values.includes((el as HTMLInputElement).value)) (el as HTMLInputElement).checked = true;
      } else if (el instanceof HTMLSelectElement) {
        if (Array.from(el.options).some((o) => o.value === values[0])) el.value = values[0];
      } else {
        el.value = values[0];
      }
    }
  }
}

/** Binds one form. Safe to call again on the same element. */
export function hydrateForm(form: HTMLFormElement, search: string = window.location.search): void {
  if (!claim(form)) return;
  prefill(form, search);
  form.addEventListener('submit', (e) => {
    e.preventDefault();
    if (root(form).dataset.state === 'loading') return;
    void send(form);
  });
  const clear = (e: Event) => {
    const box = (e.target as HTMLElement).closest<HTMLElement>('[data-field]');
    if (box && (e.target as HTMLElement).getAttribute('aria-invalid')) clearField(box);
  };
  form.addEventListener('input', clear);
  form.addEventListener('change', clear);
}

// The one script for every moss form: a request form and a subscribe form.
autoHydrate<HTMLFormElement>('form[data-moss-form]', (f) => hydrateForm(f));
autoHydrate<HTMLFormElement>('form.moss-subscribe-form[data-moss-hosted="true"]', hydrateSubscribeForm);
