import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ChipBar } from '../chip-bar';
import { FrontmatterStore } from '../frontmatter-store';

class StubResizeObserver { observe() {} unobserve() {} disconnect() {} }
(globalThis as any).ResizeObserver = StubResizeObserver;

const field = (name: string, priority: number) => ({ name, widget: 'text-input', type: 'string', priority, required: false, description: '', group: 'Common' });
const SCHEMA: any = {
  generator: 'test', version: '1', shortcodes: {},
  frontmatter: {
    fields: { title: field('title', 10), description: field('description', 20), date: field('date', 30) },
    internal_fields: ['inferred'],
  },
};

function mount(initial: Record<string, unknown>) {
  document.body.innerHTML = '';
  const store = new FrontmatterStore({ initial, requestSave: vi.fn().mockResolvedValue(undefined) });
  const bar = new ChipBar(SCHEMA, store);
  const container = document.createElement('div');
  document.body.appendChild(container);
  bar.render(container);
  return { bar, store, container };
}
const chip = (c: HTMLElement, key: string) => c.querySelector<HTMLElement>(`.chip[data-key="${key}"]`);

describe('internal fields', () => {
  it('never get a chip, though the file holds them', () => {
    const { container, bar } = mount({ inferred: ['title'], title: 'T' });
    expect(chip(container, 'inferred')).toBeNull();
    expect(bar.getValues().inferred).toEqual(['title']);
  });
});

describe('suggested values', () => {
  let ctx: ReturnType<typeof mount>;
  beforeEach(() => { ctx = mount({ inferred: ['title', 'description'] }); });

  it('show muted on the Title and Description chips, with the source in the tooltip', () => {
    ctx.bar.setSuggestions({ title: 'Summary of 2019', description: 'The harvest rose.' });
    const el = chip(ctx.container, 'title')!;
    expect(el.querySelector('.chip-value-suggested')?.textContent).toBe('Summary of 2019');
    expect(el.dataset.tooltip).toBe('from the document');
    expect(chip(ctx.container, 'description')?.querySelector('.chip-value-suggested')?.textContent).toBe('The harvest rose.');
  });

  it('show on the Date chip too', () => {
    const { bar, container } = mount({ inferred: ['date'] });
    bar.setSuggestions({ date: '2019-03-04' });
    expect(chip(container, 'date')?.querySelector('.chip-value-suggested')?.textContent).toBe('2019-03-04');
  });

  it('write nothing to the page', () => {
    ctx.bar.setSuggestions({ title: 'Summary' });
    expect(ctx.store.getValues().title).toBeUndefined();
  });

  it('give way to what the author types, and return when the field is cleared', () => {
    ctx.bar.setSuggestions({ title: 'Summary' });
    ctx.bar.setFieldValue('title', 'Mine');
    expect(chip(ctx.container, 'title')?.querySelector('.chip-value-suggested')).toBeNull();
    expect(chip(ctx.container, 'title')?.textContent).toContain('Mine');
    ctx.bar.setFieldValue('title', undefined);
    expect(chip(ctx.container, 'title')?.querySelector('.chip-value-suggested')?.textContent).toBe('Summary');
  });

  it('are not shown for a field the page does not list under inferred', () => {
    const { bar, container } = mount({ inferred: ['description'] });
    bar.setSuggestions({ title: 'Summary' });
    expect(chip(container, 'title')).toBeNull();
  });
});
