// Suggested values on the Title, Description and Date chips: what the page's embedded
// files give, shown muted until the author sets the field.

import { t } from "../../i18n/index";

/** Fields whose chip can show a value the build derives from the page's embeds. */
const SUGGESTIBLE = ['title', 'description', 'date'];

export type Suggestions = Record<string, string>;

export const sameSuggestions = (a: Suggestions, b: Suggestions): boolean => SUGGESTIBLE.every(k => a[k] === b[k]);

/** The value a chip shows while the page leaves `key` unset: only for a field listed under `inferred`, never over the author's own value. */
export function suggestionFor(key: string, values: Record<string, unknown>, suggestions: Suggestions): string | undefined {
  if (!SUGGESTIBLE.includes(key) || values[key] !== undefined) return undefined;
  const listed = values.inferred;
  return Array.isArray(listed) && listed.includes(key) ? suggestions[key] || undefined : undefined;
}

/** Paint `text` on a chip as a suggestion: muted, with the source named in its tooltip. */
export function paintSuggestion(chip: HTMLElement, valueEl: HTMLElement, text: string): void {
  valueEl.classList.remove('chip-value-mono');
  const span = document.createElement('span');
  span.className = 'chip-value-suggested';
  span.textContent = text;
  valueEl.replaceChildren(span);
  Object.assign(chip.dataset, { tooltip: t('chip.suggestion.hint'), tooltipStyle: 'muted', tooltipPosition: 'bottom' });
}
