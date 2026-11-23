/**
 * The subscribe form that asks questions: moss checks it before sending, in the
 * page's words, so a required "pick several" group (which the browser cannot
 * require) is checked like every other field, and every error looks the same.
 */

import { clearField as clear, controls, copy, markField as mark } from '../forms/fields.js';

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

/** The "Other" box of a group while its choice is picked, else null. */
function openOther(field: Element): HTMLInputElement | null {
  const box = field.querySelector<HTMLElement>('.moss-form__other');
  const choice = box?.previousElementSibling?.querySelector<HTMLInputElement>('input');
  return choice?.checked ? box!.querySelector<HTMLInputElement>('input') : null;
}

/** The copy key of what is wrong with one field, or null. */
function problem(field: HTMLElement): string | null {
  const inputs = controls(field);
  if (field.dataset.required === 'true' && !inputs.some((c) => c.checked)) return 'pick_one';
  const other = openOther(field);
  if (other && !other.value.trim()) return 'required';
  if (field.dataset.required === 'true') return null;
  const first = inputs[0];
  if (!first) return null;
  const value = first.value.trim();
  if (first.required && !value) return 'required';
  if (first.type === 'email' && value && !EMAIL.test(value)) return 'email';
  return null;
}

/** The words that name a field in the "still to fill in" line. */
function nameOf(field: HTMLElement): string {
  const label = field.querySelector('legend, .moss-form__label');
  const own = label?.cloneNode(true) as HTMLElement | undefined;
  own?.querySelector('.moss-form__optional')?.remove();
  return own?.textContent?.trim() ?? '';
}

/**
 * Checks every field. On a problem it marks each one, lists them under the
 * button as links, focuses the first, and returns false.
 */
export function checkQuestions(form: HTMLFormElement): boolean {
  const status = form.querySelector<HTMLElement>('.moss-form__status');
  const wrong: HTMLElement[] = [];
  form.querySelectorAll<HTMLElement>('[data-field]').forEach((field) => {
    clear(field);
    const key = problem(field);
    if (key) {
      mark(form, field, key);
      wrong.push(field);
    }
  });
  if (status) status.replaceChildren();
  if (!wrong.length) return true;
  if (status) {
    status.append(copy(form, 'left'), ' ');
    wrong.forEach((field, i) => {
      const link = document.createElement('a');
      const target = openOther(field) ?? controls(field)[0];
      link.href = `#${target?.id ?? ''}`;
      link.textContent = nameOf(field);
      link.addEventListener('click', (e) => {
        e.preventDefault();
        target?.focus();
      });
      if (i) status.append('、');
      status.append(link);
    });
  }
  (openOther(wrong[0]) ?? controls(wrong[0])[0])?.focus();
  return false;
}

/** Live: a field's error goes as soon as it is put right. */
export function watchQuestions(form: HTMLFormElement): void {
  form.noValidate = true;
  // A choice that opens a box says so, and says whether it is open.
  form.querySelectorAll<HTMLElement>('.moss-form__other').forEach((box) => {
    const choice = box.previousElementSibling?.querySelector<HTMLInputElement>('input');
    const input = box.querySelector('input');
    if (!choice || !input) return;
    choice.setAttribute('aria-controls', input.id);
    const sync = (): void => choice.setAttribute('aria-expanded', String(choice.checked));
    sync();
    choice.addEventListener('change', sync);
  });
  const recheck = (e: Event): void => {
    const field = (e.target as Element).closest<HTMLElement>('[data-field]');
    if (field?.querySelector('[aria-invalid]') && !problem(field)) clear(field);
  };
  form.addEventListener('input', recheck);
  form.addEventListener('change', recheck);
}

/** The answers to send, by key: a pick-several group's as a list, an Other box only while its choice is picked. */
export function answersOf(form: HTMLFormElement): Record<string, string | string[]> {
  const out: Record<string, string | string[]> = {};
  form.querySelectorAll<HTMLElement>('[data-field]:not([data-field="email"])').forEach((field) => {
    for (const c of [...controls(field), openOther(field)]) {
      if (!c?.name || ((c.type === 'checkbox' || c.type === 'radio') && !c.checked)) continue;
      const v = c.value.trim();
      if (!v) continue;
      if (c.type === 'checkbox') ((out[c.name] ??= []) as string[]).push(v);
      else out[c.name] = v;
    }
  });
  return out;
}

/** The address the confirmation went to, shown under "Check your email". */
export function showAddress(form: HTMLFormElement, email: string): void {
  const status = form.querySelector<HTMLElement>('.moss-subscribe-status');
  if (!status) return;
  let to = status.querySelector<HTMLElement>('.moss-subscribe-status__to');
  if (!to) {
    to = document.createElement('span');
    to.className = 'moss-subscribe-status__to';
    status.append(to);
  }
  to.textContent = email;
}
