import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import { ChipBar } from '../chip-bar';
import { FrontmatterStore } from '../frontmatter-store';
import type { ContentSchema } from '@symbiosis-lab/moss-ui';

vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} });

// `start` as the built-in schema declares it: a text field whose format is
// the only sign that it holds a date with an optional time.
const SCHEMA = {
  frontmatter: {
    fields: {
      start: { type: 'string', widget: 'text-input', format: 'event-time', group: 'Event' },
    },
  },
} as unknown as ContentSchema;

describe('an event-time chip edits with a date and an optional time', () => {
  let container: HTMLElement;
  let bar: ChipBar;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
  });

  afterEach(() => {
    bar.destroy();
    document.body.innerHTML = '';
  });

  function open(start?: string) {
    const write = vi.fn().mockResolvedValue(undefined);
    const store = new FrontmatterStore({ initial: start === undefined ? {} : { start }, requestSave: write });
    bar = new ChipBar(SCHEMA, store);
    bar.render(container);
    if (start === undefined) {
      (container.querySelector('.chip-unified-btn') as HTMLElement).click();
      const row = Array.from(document.querySelectorAll('.omenu .orow-add'))
        .find(r => r.textContent?.includes('Start')) as HTMLElement;
      row.click();
    } else {
      (container.querySelector('.chip[data-key="start"]') as HTMLElement).click();
    }
    const date = document.body.querySelector('.pop input[type="date"]') as HTMLInputElement;
    const time = document.body.querySelector('.pop input[type="time"]') as HTMLInputElement;
    const set = (el: HTMLInputElement, value: string) => {
      el.value = value;
      el.dispatchEvent(new Event('change', { bubbles: true }));
    };
    const press = (el: HTMLInputElement, key: string) =>
      el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
    return { store, write, date, time, set, press };
  }

  test('a date alone shows no time, and a new date stays a date alone', () => {
    const { store, date, time, set } = open('2026-12-01');
    expect([date.value, time.value]).toEqual(['2026-12-01', '']);
    expect(date.getAttribute('aria-label')).toBe('Start');
    expect(time.getAttribute('aria-label')).toBe('Time');

    set(date, '2026-12-05');
    expect(store.getValues().start).toBe('2026-12-05');
  });

  test('adding a time writes the form the build reads; clearing it goes back to the date', () => {
    const { store, time, set } = open('2026-12-01');
    set(time, '19:30:00');
    expect(store.getValues().start).toBe('2026-12-01 19:30');
    set(time, '');
    expect(store.getValues().start).toBe('2026-12-01');
  });

  test('a changed value keeps the separator the file used', () => {
    const { store, date, time, set } = open('2026-12-01T19:30');
    expect([date.value, time.value]).toEqual(['2026-12-01', '19:30']);
    set(date, '2026-12-02');
    expect(store.getValues().start).toBe('2026-12-02T19:30');
  });

  test.each(['2026-12-01', '2026-12-01 19:30', '2026-12-01T19:30', '2026-12-1'])(
    'opening %s and leaving by Enter, Escape or × writes nothing',
    (start) => {
      for (const leave of ['Enter', 'Escape', '×']) {
        const { store, write, date, press } = open(start);
        if (leave === '×') (document.body.querySelector('.pop-x') as HTMLElement).click();
        else press(date, leave);
        expect(document.body.querySelector('.pop')).toBeNull();
        expect(store.getValues().start).toBe(start);
        expect(write).not.toHaveBeenCalled();
        bar.destroy();
      }
    },
  );

  test('a value the build rejects opens empty; a time alone neither writes nor removes it', () => {
    const { store, write, date, time, set } = open('2026-12-1');
    expect([date.value, time.value]).toEqual(['', '']);
    set(time, '19:30');
    expect(store.getValues().start).toBe('2026-12-1');
    expect(write).not.toHaveBeenCalled();

    set(date, '2026-12-01');
    expect(store.getValues().start).toBe('2026-12-01 19:30');
  });

  test('a half-typed date or time reads as empty and counts as unchanged', () => {
    const { store, write, date, time, set } = open('2026-12-01 19:30');
    const halfTyped = (el: HTMLInputElement, badInput: boolean) =>
      Object.defineProperty(el, 'validity', { value: { badInput }, configurable: true });

    halfTyped(date, true);
    set(date, '');
    expect(store.getValues().start).toBe('2026-12-01 19:30');
    expect(write).not.toHaveBeenCalled();
    halfTyped(date, false);

    // The other control still writes, keeping the half-typed one's last value.
    halfTyped(time, true);
    time.value = '';
    set(date, '2026-12-02');
    expect(store.getValues().start).toBe('2026-12-02 19:30');
  });

  test('clearing the date removes the field', () => {
    const { store, date, set } = open('2026-12-01 19:30');
    set(date, '');
    expect('start' in store.getValues()).toBe(false);
  });

  test('adding the field suggests today and writes it only on Enter', () => {
    const { store, write, date, time, press } = open();
    expect(date.value).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(time.value).toBe('');
    expect(store.getValues()).not.toHaveProperty('start');
    expect(write).not.toHaveBeenCalled();

    press(time, 'Enter');
    expect(store.getValues().start).toBe(date.value);
    expect(document.body.querySelector('.pop')).toBeNull();
  });
});
