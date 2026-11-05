import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createFindPill } from '../find-pill';

describe('createFindPill', () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('creates an element with a find input', () => {
    const pill = createFindPill({
      mode: 'find-only',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    expect(container.querySelector('.find-pill')).not.toBeNull();
    expect(container.querySelector('.find-pill__input--find')).not.toBeNull();
    pill.destroy();
  });

  it('icon buttons carry tooltips that teach their keyboard twins', () => {
    const pill = createFindPill({
      mode: 'find-only',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    const kbd = (sel: string) =>
      (container.querySelector(sel) as HTMLElement).getAttribute('data-tooltip-kbd');
    expect(kbd('.find-pill__btn--prev')).toBe('⇧↩');
    expect(kbd('.find-pill__btn--next')).toBe('↩');
    expect(kbd('.find-pill__btn--close')).toBe('⎋');
    // data-tooltip names the action (portal tooltip, never title=)
    expect(
      (container.querySelector('.find-pill__btn--next') as HTMLElement).getAttribute('data-tooltip'),
    ).not.toBeNull();
    pill.destroy();
  });

  it('does not render a replace row in find-only mode', () => {
    const pill = createFindPill({
      mode: 'find-only',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    expect(container.querySelector('.find-pill__input--replace')).toBeNull();
    pill.destroy();
  });

  it('renders a replace row in find-replace mode', () => {
    const pill = createFindPill({
      mode: 'find-replace',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onReplace: vi.fn(),
      onReplaceAll: vi.fn(),
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    expect(container.querySelector('.find-pill__input--replace')).not.toBeNull();
    expect(container.querySelector('.find-pill__btn--replace')).not.toBeNull();
    expect(container.querySelector('.find-pill__btn--replace-all')).not.toBeNull();
    pill.destroy();
  });

  it('calls onQueryChange when find input changes', () => {
    const onQueryChange = vi.fn();
    const pill = createFindPill({
      mode: 'find-only',
      onQueryChange,
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    const input = container.querySelector('.find-pill__input--find') as HTMLInputElement;
    input.value = 'hello';
    input.dispatchEvent(new Event('input'));
    expect(onQueryChange).toHaveBeenCalledWith('hello');
    pill.destroy();
  });

  it('calls onFindNext when Enter is pressed in find input', () => {
    const onFindNext = vi.fn();
    const pill = createFindPill({
      mode: 'find-only',
      onQueryChange: vi.fn(),
      onFindNext,
      onFindPrev: vi.fn(),
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    const input = container.querySelector('.find-pill__input--find') as HTMLInputElement;
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onFindNext).toHaveBeenCalled();
    pill.destroy();
  });

  it('calls onFindPrev when Shift+Enter is pressed in find input', () => {
    const onFindPrev = vi.fn();
    const pill = createFindPill({
      mode: 'find-only',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev,
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    const input = container.querySelector('.find-pill__input--find') as HTMLInputElement;
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true, bubbles: true }));
    expect(onFindPrev).toHaveBeenCalled();
    pill.destroy();
  });

  it('calls onClose when Escape is pressed', () => {
    const onClose = vi.fn();
    const pill = createFindPill({
      mode: 'find-only',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onClose,
    });
    container.appendChild(pill.el);
    const input = container.querySelector('.find-pill__input--find') as HTMLInputElement;
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(onClose).toHaveBeenCalled();
    pill.destroy();
  });

  it('calls onClose when × button is clicked', () => {
    const onClose = vi.fn();
    const pill = createFindPill({
      mode: 'find-only',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onClose,
    });
    container.appendChild(pill.el);
    const closeBtn = container.querySelector('.find-pill__btn--close') as HTMLButtonElement;
    closeBtn.click();
    expect(onClose).toHaveBeenCalled();
    pill.destroy();
  });

  it('setCounter updates counter text', () => {
    const pill = createFindPill({
      mode: 'find-only',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    pill.setCounter(2, 5);
    const counter = container.querySelector('.find-pill__counter') as HTMLElement;
    expect(counter.textContent).toBe('2 of 5');
    pill.destroy();
  });

  it('setCounter hides counter when total is 0', () => {
    const pill = createFindPill({
      mode: 'find-only',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    pill.setCounter(0, 0);
    const counter = container.querySelector('.find-pill__counter') as HTMLElement;
    expect(counter.textContent).toBe('0 results');
    pill.destroy();
  });

  it('destroy removes element from DOM', () => {
    const pill = createFindPill({
      mode: 'find-only',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    pill.destroy();
    expect(container.querySelector('.find-pill')).toBeNull();
  });

  it('Tab in find input moves focus to replace input in find-replace mode', () => {
    const pill = createFindPill({
      mode: 'find-replace',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onReplace: vi.fn(),
      onReplaceAll: vi.fn(),
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    const findInput = container.querySelector('.find-pill__input--find') as HTMLInputElement;
    const replaceInput = container.querySelector('.find-pill__input--replace') as HTMLInputElement;
    const focusSpy = vi.spyOn(replaceInput, 'focus');
    findInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
    expect(focusSpy).toHaveBeenCalled();
    pill.destroy();
  });

  it('clearCounter hides the counter', () => {
    const pill = createFindPill({
      mode: 'find-only',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    pill.setCounter(2, 5);
    pill.clearCounter();
    const counter = container.querySelector('.find-pill__counter') as HTMLElement;
    expect(counter.textContent).toBe('');
    pill.destroy();
  });

  it('setQueryState adds no-matches class', () => {
    const pill = createFindPill({
      mode: 'find-only',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    pill.setQueryState('no-matches');
    const input = container.querySelector('.find-pill__input--find') as HTMLInputElement;
    expect(input.classList.contains('find-pill__input--no-matches')).toBe(true);
    pill.destroy();
  });

  it('setQueryState removes no-matches class on normal', () => {
    const pill = createFindPill({
      mode: 'find-only',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    pill.setQueryState('no-matches');
    pill.setQueryState('normal');
    const input = container.querySelector('.find-pill__input--find') as HTMLInputElement;
    expect(input.classList.contains('find-pill__input--no-matches')).toBe(false);
    pill.destroy();
  });

  it('Tab in replace input moves focus back to find input', () => {
    const pill = createFindPill({
      mode: 'find-replace',
      onQueryChange: vi.fn(),
      onFindNext: vi.fn(),
      onFindPrev: vi.fn(),
      onReplace: vi.fn(),
      onReplaceAll: vi.fn(),
      onClose: vi.fn(),
    });
    container.appendChild(pill.el);
    const findInput = container.querySelector('.find-pill__input--find') as HTMLInputElement;
    const replaceInput = container.querySelector('.find-pill__input--replace') as HTMLInputElement;
    const focusSpy = vi.spyOn(findInput, 'focus');
    replaceInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
    expect(focusSpy).toHaveBeenCalled();
    pill.destroy();
  });
});
