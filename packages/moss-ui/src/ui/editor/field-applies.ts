import { t } from "../i18n/index";
import type { FieldDefinition, PageFacts, PagePosition } from '@symbiosis-lab/moss-ui';

export const NO_PAGE_FACTS: PageFacts = { position: null, auto: {} };

/** A field with no `applies_when` applies everywhere. */
export function appliesHere(
  def: Pick<FieldDefinition, 'applies_when'> | null,
  position: PagePosition | null,
  values: Record<string, unknown>,
): boolean {
  const when = def?.applies_when;
  if (!when || !position) return true;
  return when.some((c) =>
    'position' in c ? c.position === position : values[c.set] != null && values[c.set] !== '');
}

/** The trailing "auto: …" on an add-list row, or null when the build picks nothing. */
export function autoHint(auto: PageFacts['auto'], key: string): HTMLElement | null {
  const value = auto[key];
  if (typeof value !== 'string' && typeof value !== 'boolean') return null;
  const hint = document.createElement('span');
  hint.className = 'orow-add-auto';
  const text = typeof value === 'boolean' ? t(value ? 'chip.ui.on' : 'chip.ui.off') : value;
  hint.textContent = t('chip.ui.auto_label', { value: text });
  return hint;
}
