import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import { ChipBar } from '../chip-bar';
import { FrontmatterStore } from '../frontmatter-store';
import type { ContentSchema } from '@symbiosis-lab/moss-ui';

// The union chip (Children, Series) states its value once, in words, on the
// pill ("Children · On"), and the popover is a three-way choice
// [On] [Off] [Folder…]. The old chip said it three times (checkbox icon, a
// "—"/"✓" value, an On/Off toggle) and the icon looked like a separate control.

class MockResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
  constructor(_cb: () => void) {}
}
vi.stubGlobal('ResizeObserver', MockResizeObserver);

const SCHEMA = {
  frontmatter: {
    fields: {
      children: {
        type: 'one-of',
        widget: 'union',
        priority: 10,
        one_of: [{ type: 'boolean', widget: 'checkbox' }, { type: 'string', widget: 'wikilink-picker' }],
      },
      series: {
        type: 'one-of',
        widget: 'union',
        priority: 10,
        one_of: [{ type: 'boolean', widget: 'checkbox' }, { type: 'array', widget: 'wikilink-list-picker' }],
      },
    },
  },
} as unknown as ContentSchema;

function mount(values: Record<string, unknown>) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const store = new FrontmatterStore({ initial: values, requestSave: vi.fn().mockResolvedValue(undefined) });
  const bar = new ChipBar(SCHEMA, store);
  bar.render(container);
  const chip = (key = 'children') => container.querySelector(`.chip[data-key="${key}"]`) as HTMLElement;
  return { bar, store, container, chip };
}

function open(values: Record<string, unknown>, key = 'children') {
  const m = mount(values);
  m.chip(key).click();
  return { ...m, pop: document.querySelector('.pop') as HTMLElement };
}

const segs = (pop: HTMLElement) => Array.from(pop.querySelectorAll<HTMLElement>('.pop-union-seg'));
const selected = (pop: HTMLElement) => segs(pop).filter(s => s.getAttribute('aria-checked') === 'true').map(s => s.textContent);
const picker = (pop: HTMLElement) => pop.querySelector('.pop-union-picker') as HTMLElement;

describe('union chip pill', () => {
  beforeEach(() => { document.body.innerHTML = ''; });
  afterEach(() => { document.body.innerHTML = ''; });

  test.each([
    [{ children: true }, 'On'],
    [{ children: false }, 'Off'],
    [{ children: '[[News]]' }, 'News'],
    [{ children: '[[content/posts/News]]' }, 'News'],
  ])('%j reads %s, with no icon', (values, text) => {
    const { chip } = mount(values);
    expect(chip().querySelector('.chip-key')?.textContent).toBe('Children');
    expect(chip().querySelector('.chip-value')?.textContent).toBe(text);
    expect(chip().querySelector('.chip-icon')).toBeNull();
  });

  test('a folder keeps its full value in the tooltip', () => {
    const { chip } = mount({ children: '[[content/posts/News]]' });
    expect(chip().querySelector('.chip-value')?.getAttribute('title')).toBe('content/posts/News');
  });

  test('a list value (series) joins its entries', () => {
    const { chip } = mount({ series: ['[[A]]', '[[B]]'] });
    expect(chip('series').querySelector('.chip-value')?.textContent).toBe('A, B');
  });

  test('a provisional chip (missing value) previews the default On without writing it, and its popover selects On', () => {
    const { bar, store, container } = mount({});
    // The add row's "Children" entry creates the provisional ghost.
    (container.querySelector('.chip-unified-btn') as HTMLElement).click();
    const row = Array.from(document.querySelectorAll<HTMLElement>('*')).find(
      el => el.children.length === 0 && el.textContent === 'Children' && !el.closest('.chip-track'),
    );
    row?.click();
    const ghost = container.querySelector('.chip--provisional[data-key="children"]');
    expect(ghost?.querySelector('.chip-value')?.textContent).toBe('On');
    expect(store.getValues()).not.toHaveProperty('children');
    expect(selected(document.querySelector('.pop') as HTMLElement)).toEqual(['On']);
    bar.destroy();
  });
});

describe('union chip popover', () => {
  beforeEach(() => { document.body.innerHTML = ''; });
  afterEach(() => { document.body.innerHTML = ''; });

  test('offers On / Off / Folder… and opens on the selected segment, not the folder field', () => {
    const { pop } = open({ children: true });
    expect(segs(pop).map(s => s.textContent)).toEqual(['On', 'Off', 'Folder…']);
    expect(selected(pop)).toEqual(['On']);
    expect(picker(pop).classList.contains('is-hidden')).toBe(true);
    // Autofocusing the folder input pops its suggestion list over the choice.
    expect(document.activeElement).not.toBe(pop.querySelector('.pop-union-folder'));
  });

  test('false selects Off', () => {
    const { pop } = open({ children: false });
    expect(selected(pop)).toEqual(['Off']);
    expect(picker(pop).classList.contains('is-hidden')).toBe(true);
  });

  test('a path selects Folder… and shows the picker with the folder', () => {
    const { pop } = open({ children: '[[News]]' });
    expect(selected(pop)).toEqual(['Folder…']);
    expect(picker(pop).classList.contains('is-hidden')).toBe(false);
    expect((pop.querySelector('.pop-union-folder') as HTMLInputElement).value).toBe('News');
  });

  test('Off from a path replaces it with false and hides the picker', () => {
    const { pop, store } = open({ children: '[[News]]' });
    segs(pop)[1].click();
    expect(store.getValues().children).toBe(false);
    expect(picker(pop).classList.contains('is-hidden')).toBe(true);
    expect((pop.querySelector('.pop-union-folder') as HTMLInputElement).value).toBe('');
  });

  test('On from a path replaces it with true', () => {
    const { pop, store } = open({ children: '[[News]]' });
    segs(pop)[0].click();
    expect(store.getValues().children).toBe(true);
  });

  test('Folder… reveals the picker without writing; naming a folder writes the wikilink', () => {
    const { pop, store, chip } = open({ children: false });
    segs(pop)[2].click();
    expect(picker(pop).classList.contains('is-hidden')).toBe(false);
    expect(store.getValues().children).toBe(false);
    const inp = pop.querySelector('.pop-union-folder') as HTMLInputElement;
    inp.value = 'News';
    inp.dispatchEvent(new Event('input'));
    expect(store.getValues().children).toBe('[[News]]');
    expect(chip().querySelector('.chip-value')?.textContent).toBe('News');
  });

  test('clearing the folder writes true, never an empty string', () => {
    const { pop, store } = open({ children: '[[News]]' });
    const inp = pop.querySelector('.pop-union-folder') as HTMLInputElement;
    inp.value = '';
    inp.dispatchEvent(new Event('input'));
    expect(store.getValues().children).toBe(true);
  });

  test('arrow keys move between segments and select; Escape closes', () => {
    const { pop, store } = open({ children: true });
    const [on, off] = segs(pop);
    on.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    expect(selected(pop)).toEqual(['Off']);
    expect(store.getValues().children).toBe(false);
    expect(document.activeElement).toBe(off);
    expect(off.tabIndex).toBe(0);
    expect(on.tabIndex).toBe(-1);
    off.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(document.querySelector('.pop')).toBeNull();
  });

  test('series: Off then List… then adding C writes [C], not the stale [A, B, C]', () => {
    const { pop, store } = open({ series: ['[[A]]', '[[B]]'] }, 'series');
    segs(pop)[1].click();
    expect(store.getValues().series).toBe(false);
    segs(pop)[2].click();
    const inp = pop.querySelector('.pop-tag-input') as HTMLInputElement;
    expect(pop.querySelectorAll('.pop-tag')).toHaveLength(0);
    inp.value = 'C';
    inp.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    expect(store.getValues().series).toEqual(['[[C]]']);
  });

  test('series: removing the last entry moves the segment to On, matching the pill', () => {
    const { pop, store, chip } = open({ series: ['[[A]]'] }, 'series');
    (pop.querySelector('.pop-tag-x') as HTMLElement).click();
    expect(store.getValues().series).toBe(true);
    expect(selected(pop)).toEqual(['On']);
    expect(chip('series').querySelector('.chip-value')?.textContent).toBe('On');
  });

  test('Folder… with an empty input, then closing, changes nothing', () => {
    const { pop, store, chip } = open({ children: false });
    segs(pop)[2].click();
    (pop.querySelector('.pop-x') as HTMLElement).click();
    expect(store.getValues().children).toBe(false);
    expect(chip().querySelector('.chip-value')?.textContent).toBe('Off');
  });

  test.each([
    ['ArrowLeft', 0, 'Folder…'], // wraps from first to last
    ['End', 0, 'Folder…'],
    ['Home', 2, 'On'],
    ['ArrowRight', 2, 'On'], // wraps from last to first
  ])('%s from segment %i selects %s', (k, from, want) => {
    const { pop } = open({ children: from === 0 ? true : '[[News]]' });
    segs(pop)[from].dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true }));
    expect(selected(pop)).toEqual([want]);
  });

  test('series adapts the third segment to its list member', () => {
    const { pop, store } = open({ series: ['[[A]]'] }, 'series');
    expect(segs(pop).map(s => s.textContent)).toEqual(['On', 'Off', 'List…']);
    expect(selected(pop)).toEqual(['List…']);
    segs(pop)[0].click();
    expect(store.getValues().series).toBe(true);
  });
});
