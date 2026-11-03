import { autoHydrate, claim, inPreview, lockSlotWidth, setState } from '../subscribe/hosted-form.js';

type Answer = {
  ok?: boolean;
  error?: string;
  fields?: Record<string, string>;
};

/** Seam for tests: jsdom cannot navigate. */
export const navigation = {
  to(url: string): void {
    window.location.assign(url);
  },
};

const text = (form: HTMLFormElement, key: string): string =>
  form.querySelector<HTMLElement>(`[data-copy="${key}"]`)?.textContent ?? '';

const root = (form: HTMLFormElement): HTMLElement => form.closest<HTMLElement>('.moss-form') ?? form;

const status = (form: HTMLFormElement, message: string): void => {
  const line = form.querySelector<HTMLElement>('.moss-form__status');
  if (line) line.textContent = message;
};

const fieldBox = (form: HTMLFormElement, name: string): HTMLElement | null =>
  form.querySelector<HTMLElement>(`[data-field="${CSS.escape(name)}"]`);

const controlsOf = (box: HTMLElement): HTMLElement[] =>
  Array.from(box.querySelectorAll<HTMLElement>('input, select, textarea')).filter(
    (c) => (c as HTMLInputElement).type !== 'hidden',
  );

function clearField(box: HTMLElement): void {
  const error = box.querySelector<HTMLElement>('.moss-form__error');
  if (error) {
    error.hidden = true;
    error.textContent = '';
  }
  for (const c of controlsOf(box)) {
    c.removeAttribute('aria-invalid');
    if (error && c.getAttribute('aria-describedby')) {
      const ids = c.getAttribute('aria-describedby')!.split(' ').filter((id) => id !== error.id);
      if (ids.length) c.setAttribute('aria-describedby', ids.join(' '));
      else c.removeAttribute('aria-describedby');
    }
  }
}

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
    const error = box.querySelector<HTMLElement>('.moss-form__error');
    if (error) {
      error.textContent = text(form, code === 'required' ? 'required' : 'format');
      error.hidden = false;
    }
    for (const c of controlsOf(box)) {
      c.setAttribute('aria-invalid', 'true');
      if (error) {
        const ids = (c.getAttribute('aria-describedby') ?? '').split(' ').filter(Boolean);
        if (!ids.includes(error.id)) ids.push(error.id);
        c.setAttribute('aria-describedby', ids.join(' '));
      }
    }
    refused.push(box);
  });
  const first = refused[0] && controlsOf(refused[0])[0];
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

  try {
    const res = await fetch(form.action, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded', Accept: 'application/json' },
      body: body(form),
    });
    const answer: Answer = await res.json().catch(() => ({}));
    if (res.ok && answer.ok !== false) {
      navigation.to(next);
      return;
    }
    setState(root(form), 'error');
    if (res.status === 422 && answer.fields && showFieldErrors(form, answer.fields)) return;
    if (res.status === 422) status(form, text(form, 'invalid'));
    else if (res.status === 429) status(form, text(form, 'rate_limited'));
    else if (res.status === 404) status(form, text(form, 'unknown_form'));
    else status(form, text(form, 'network'));
  } catch {
    setState(root(form), 'error');
    status(form, text(form, 'network'));
  }
}

/**
 * Fills visible fields from the page's address (`?session=Family`). Sets a
 * value or ticks a matching choice and nothing else, so a link cannot reach
 * `_next`, `_hp` or any hidden input.
 */
export function prefill(form: HTMLFormElement, search: string): void {
  const params = new URLSearchParams(search);
  for (const name of new Set(params.keys())) {
    if (name === '_next' || name === '_hp') continue;
    const values = params.getAll(name);
    const controls = Array.from(form.elements).filter(
      (el): el is HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement =>
        (el as HTMLInputElement).name === name &&
        (el as HTMLInputElement).type !== 'hidden' &&
        !el.closest('.moss-form__hp'),
    );
    for (const el of controls) {
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

autoHydrate<HTMLFormElement>('form[data-moss-form]', (f) => hydrateForm(f));
