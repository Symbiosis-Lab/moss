import './find-pill.css';
import { t } from '../i18n';

export interface FindPillOptions {
  mode: 'find-only' | 'find-replace';
  onQueryChange: (q: string) => void;
  onFindNext: () => void;
  onFindPrev: () => void;
  onReplace?: (replacement: string) => void;
  onReplaceAll?: (replacement: string) => void;
  onClose: () => void;
}

export interface FindPill {
  el: HTMLElement;
  setCounter(current: number, total: number): void;
  clearCounter(): void;
  setQueryState(state: 'normal' | 'no-matches'): void;
  focusInput(): void;
  destroy(): void;
}

export function createFindPill(options: FindPillOptions): FindPill {
  const el = document.createElement('div');
  el.className = 'find-pill';

  // ── Find row ──────────────────────────────────────────────────────────
  const findRow = document.createElement('div');
  findRow.className = 'find-pill__row';

  const findLabel = document.createElement('span');
  findLabel.className = 'find-pill__label';
  findLabel.textContent = t('find.label');

  const findInput = document.createElement('input');
  findInput.type = 'text';
  findInput.className = 'find-pill__input find-pill__input--find';
  findInput.placeholder = t('find.placeholder');
  findInput.autocomplete = 'off';
  findInput.spellcheck = false;

  const counter = document.createElement('span');
  counter.className = 'find-pill__counter';

  // Icon-only buttons: data-tooltip names the action, data-tooltip-kbd teaches
  // the keyboard twin bound on the input above (Enter / ⇧Enter / Escape) —
  // design-vocabulary "tooltips teach the shortcut".
  const prevBtn = document.createElement('button');
  prevBtn.className = 'find-pill__btn find-pill__btn--prev';
  prevBtn.setAttribute('aria-label', t('find.prev'));
  prevBtn.setAttribute('data-tooltip', t('find.prev'));
  // Bottom: the pill is a horizontal row pinned to the surface's top edge,
  // so a side tooltip covers the input and a top one has no room.
  prevBtn.setAttribute('data-tooltip-position', 'bottom');
  prevBtn.setAttribute('data-tooltip-kbd', '⇧↩');
  prevBtn.textContent = '↑';
  prevBtn.type = 'button';

  const nextBtn = document.createElement('button');
  nextBtn.className = 'find-pill__btn find-pill__btn--next';
  nextBtn.setAttribute('aria-label', t('find.next'));
  nextBtn.setAttribute('data-tooltip', t('find.next'));
  nextBtn.setAttribute('data-tooltip-position', 'bottom');
  nextBtn.setAttribute('data-tooltip-kbd', '↩');
  nextBtn.textContent = '↓';
  nextBtn.type = 'button';

  const closeBtn = document.createElement('button');
  closeBtn.className = 'find-pill__btn find-pill__btn--close';
  closeBtn.setAttribute('aria-label', t('find.close'));
  closeBtn.setAttribute('data-tooltip', t('find.close'));
  closeBtn.setAttribute('data-tooltip-position', 'bottom');
  closeBtn.setAttribute('data-tooltip-kbd', '⎋');
  closeBtn.textContent = '×';
  closeBtn.type = 'button';

  if (options.mode === 'find-replace') {
    findRow.append(findLabel, findInput, counter, prevBtn, nextBtn, closeBtn);
  } else {
    findRow.append(findInput, counter, prevBtn, nextBtn, closeBtn);
  }
  el.appendChild(findRow);

  // ── Replace row (find-replace mode only) ─────────────────────────────
  let replaceInput: HTMLInputElement | null = null;
  if (options.mode === 'find-replace') {
    const divider = document.createElement('div');
    divider.className = 'find-pill__divider';
    el.appendChild(divider);

    const replaceRow = document.createElement('div');
    replaceRow.className = 'find-pill__row';

    const replaceLabel = document.createElement('span');
    replaceLabel.className = 'find-pill__label';
    replaceLabel.textContent = t('find.replace_label');

    replaceInput = document.createElement('input');
    replaceInput.type = 'text';
    replaceInput.className = 'find-pill__input find-pill__input--replace';
    replaceInput.placeholder = t('find.replace_placeholder');
    replaceInput.autocomplete = 'off';
    replaceInput.spellcheck = false;

    const replaceBtn = document.createElement('button');
    replaceBtn.className = 'find-pill__btn find-pill__btn--replace';
    replaceBtn.textContent = t('find.replace_btn');
    replaceBtn.type = 'button';

    const replaceAllBtn = document.createElement('button');
    replaceAllBtn.className = 'find-pill__btn find-pill__btn--replace-all';
    replaceAllBtn.textContent = t('find.replace_all');
    replaceAllBtn.type = 'button';

    replaceRow.append(replaceLabel, replaceInput, replaceBtn, replaceAllBtn);
    el.appendChild(replaceRow);

    replaceBtn.addEventListener('click', () => {
      options.onReplace?.(replaceInput!.value);
    });
    replaceAllBtn.addEventListener('click', () => {
      options.onReplaceAll?.(replaceInput!.value);
    });
    replaceInput.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') { e.preventDefault(); options.onClose(); }
      if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); options.onFindNext(); }
      if (e.key === 'Enter' && e.shiftKey) { e.preventDefault(); options.onFindPrev(); }
      if (e.key === 'Tab') { e.preventDefault(); findInput.focus(); }
    });
  }

  // ── Event listeners ───────────────────────────────────────────────────
  findInput.addEventListener('input', () => options.onQueryChange(findInput.value));
  findInput.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); options.onClose(); }
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); options.onFindNext(); }
    if (e.key === 'Enter' && e.shiftKey) { e.preventDefault(); options.onFindPrev(); }
    if (e.key === 'Tab' && replaceInput) { e.preventDefault(); replaceInput.focus(); }
  });
  nextBtn.addEventListener('click', () => options.onFindNext());
  prevBtn.addEventListener('click', () => options.onFindPrev());
  closeBtn.addEventListener('click', () => options.onClose());

  return {
    el,
    setCounter(current: number, total: number) {
      if (total === 0) {
        counter.textContent = t('find.no_results');
      } else {
        counter.textContent = t('find.count', { current, total });
      }
    },
    clearCounter() {
      counter.textContent = '';
    },
    setQueryState(state: 'normal' | 'no-matches') {
      findInput.classList.toggle('find-pill__input--no-matches', state === 'no-matches');
    },
    focusInput() {
      findInput.focus();
      findInput.select();
    },
    destroy() {
      el.remove();
    },
  };
}
