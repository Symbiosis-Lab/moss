import { describe, it, expect, afterEach } from 'vitest';
import type { PageFacts } from '@symbiosis-lab/moss-ui';
import { ChipBar } from '../chip-bar';
import { FrontmatterStore } from '../frontmatter-store';
import type { PagePosition } from '@symbiosis-lab/moss-ui';

class StubResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as any).ResizeObserver = StubResizeObserver;

const SCHEMA: any = {
  generator: 'test',
  version: '1',
  shortcodes: {},
  frontmatter: {
    fields: {
      title: { widget: 'text-input', type: 'string', group: 'Common' },
      layout: { widget: 'select', type: 'string', enum_values: ['page', 'article'], group: 'Common' },
      sort: { widget: 'select', type: 'string', enum_values: ['date', 'title'], group: 'Common', applies_when: [{ position: 'folder' }, { position: 'home' }] },
      start: { widget: 'text-input', type: 'string', group: 'Common' },
      end: { widget: 'text-input', type: 'string', group: 'Common', applies_when: [{ set: 'start' }] },
      listed: { widget: 'checkbox', type: 'boolean', group: 'Common' },
    },
  },
};

const mounted: ChipBar[] = [];
afterEach(() => { mounted.splice(0).forEach((b) => b.destroy()); });

function addRows(initial: Record<string, unknown>, position?: PagePosition, auto?: PageFacts['auto'], suggestions?: Record<string, string>): Map<string, HTMLElement> {
  document.body.innerHTML = '';
  const store = new FrontmatterStore({ initial, requestSave: async () => {} });
  const bar = new ChipBar(SCHEMA, store, {});
  bar.setPageFacts({ position: position ?? null, auto: auto ?? {} });
  if (suggestions) bar.setSuggestions(suggestions);
  const container = document.createElement('div');
  document.body.appendChild(container);
  bar.render(container);
  mounted.push(bar);
  (document.querySelector('.chip-unified-btn') as HTMLElement).click();
  const rows = new Map<string, HTMLElement>();
  for (const r of document.querySelectorAll<HTMLElement>('.omenu .orow-add')) {
    rows.set(r.querySelector('.orow-add-label')!.textContent!, r);
  }
  return rows;
}

describe('add list with page facts', () => {
  it('offers every field when the page facts are unknown', () => {
    expect([...addRows({}).keys()]).toEqual(['Title', 'Layout', 'Sort', 'Start', 'End', 'Listed']);
  });

  it('leaves out a field that cannot change a page in this position', () => {
    expect(addRows({}, 'page', {}).has('Sort')).toBe(false);
    expect(addRows({}, 'folder', {}).has('Sort')).toBe(true);
  });

  it('offers a field that depends on another once that one is in the file', () => {
    expect(addRows({}, 'page', {}).has('End')).toBe(false);
    expect(addRows({ start: '2026-11-01' }, 'page', {}).has('End')).toBe(true);
    expect(addRows({ start: '' }, 'page', {}).has('End')).toBe(false);
  });

  it('shows the automatic value on its row, and only there', () => {
    const rows = addRows({}, 'page', { layout: 'article' });
    expect(rows.get('Layout')!.querySelector('.orow-add-auto')?.textContent).toBe('auto: article');
    expect(document.querySelectorAll('.orow-add-auto').length).toBe(1);
  });

  it('reads an automatic on/off value as words', () => {
    const rows = addRows({}, 'page', { listed: true });
    expect(rows.get('Listed')!.querySelector('.orow-add-auto')?.textContent).toBe('auto: On');
  });

  it('never shows an automatic value for a field the document already suggests', () => {
    const rows = addRows({ inferred: ['title'] }, 'page', { title: 'From the file name' }, { title: 'From the heading' });
    expect(rows.has('Title')).toBe(false);
    expect(addRows({}, 'page', { title: 'From the file name' }).get('Title')!.querySelector('.orow-add-auto')).not.toBeNull();
  });
});
