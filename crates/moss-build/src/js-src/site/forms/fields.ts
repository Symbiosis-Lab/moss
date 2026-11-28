/**
 * Marking a form's fields, for every moss form: what the browser checks before
 * sending and what the service refuses after. A field is a `[data-field]` box
 * with its controls and a `.moss-form__error` line; its words come from the
 * form's hidden `data-copy` spans, in the page's language.
 */

export const copy = (form: HTMLFormElement, key: string): string =>
  form.querySelector<HTMLElement>(`[data-copy="${key}"]`)?.textContent ?? '';

/** A field's own controls, without its "Other" box. */
export const controls = (field: Element): HTMLInputElement[] =>
  Array.from(field.querySelectorAll<HTMLInputElement>('input, select, textarea')).filter(
    (c) => c.type !== 'hidden' && !c.closest('.moss-form__other'),
  );

const errorOf = (field: HTMLElement): HTMLElement | null => field.querySelector<HTMLElement>(':scope > .moss-form__error');

export function clearField(field: HTMLElement): void {
  const error = errorOf(field);
  if (error) {
    error.hidden = true;
    error.textContent = '';
  }
  for (const c of controls(field)) {
    c.removeAttribute('aria-invalid');
    const ids = (c.getAttribute('aria-describedby') ?? '').split(' ').filter((i) => i && i !== error?.id);
    if (ids.length) c.setAttribute('aria-describedby', ids.join(' '));
    else c.removeAttribute('aria-describedby');
  }
}

/** Shows the copy `key` under the field and ties it to each control. */
export function markField(form: HTMLFormElement, field: HTMLElement, key: string): void {
  const error = errorOf(field);
  if (error) {
    error.textContent = copy(form, key);
    error.hidden = false;
  }
  for (const c of controls(field)) {
    c.setAttribute('aria-invalid', 'true');
    if (error) {
      const ids = (c.getAttribute('aria-describedby') ?? '').split(' ').filter(Boolean);
      if (!ids.includes(error.id)) ids.push(error.id);
      c.setAttribute('aria-describedby', ids.join(' '));
    }
  }
}
