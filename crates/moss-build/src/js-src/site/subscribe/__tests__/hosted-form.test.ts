import { describe, expect, test } from 'vitest';
import { claim, lockSlotWidth, setState } from '../hosted-form.js';

function mount(direction: string) {
  document.body.innerHTML = `<form style="display:flex;flex-direction:${direction}"><span class="moss-btn-slot"><button type="submit"></button></span></form>`;
  const form = document.querySelector('form')!;
  const btn = form.querySelector('button')!;
  Object.defineProperty(btn, 'offsetWidth', { value: 120 });
  return { form, btn, slot: form.querySelector<HTMLElement>('.moss-btn-slot')! };
}

describe('hosted form helpers', () => {
  test('an element is claimed once', () => {
    const el = document.createElement('form');
    expect(claim(el)).toBe(true);
    expect(claim(el)).toBe(false);
  });

  test('only loading disables the button for assistive tech: on success it resets the form', () => {
    const { form, btn } = mount('row');
    for (const [state, disabled] of [['loading', true], ['success', false], ['error', false], ['idle', false]] as const) {
      setState(form, state);
      expect(form.dataset.state).toBe(state);
      expect(btn.hasAttribute('aria-disabled')).toBe(disabled);
    }
  });

  test('the slot is pinned to the idle button width in a row and left fluid in a column', () => {
    const row = mount('row');
    lockSlotWidth(row.form, row.btn);
    expect(row.slot.style.width).toBe('120px');
    const column = mount('column');
    lockSlotWidth(column.form, column.btn);
    expect(column.slot.style.width).toBe('');
  });
});
