import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ChipBar } from '../chip-bar';
import { FrontmatterStore } from '../frontmatter-store';

// jsdom does not implement ResizeObserver; ChipBar.renderChips uses it.
class StubResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as any).ResizeObserver = StubResizeObserver;

const SCHEMA_FIXTURE: any = {
  generator: 'test',
  version: '1',
  shortcodes: {},
  frontmatter: {
    fields: {
      title: { name: 'title', widget: 'text-input', type: 'string', priority: 10, required: true, description: '', group: 'Common' },
    },
  },
};

function makeStore(initial: Record<string, unknown> = {}) {
  const write = vi.fn().mockResolvedValue(undefined);
  const store = new FrontmatterStore({ initial, requestSave: write });
  return { store, write };
}

describe('ChipBar.setFieldValue', () => {
  let renderer: ChipBar;
  let container: HTMLDivElement;
  let store: FrontmatterStore;
  let write: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    document.body.innerHTML = '';
    ({ store, write } = makeStore({}));
    renderer = new ChipBar(SCHEMA_FIXTURE, store);
    container = document.createElement('div');
    document.body.appendChild(container);
    renderer.render(container);
  });

  it('updates the in-memory value', () => {
    renderer.setFieldValue('title', 'Hello');
    expect(renderer.getValues().title).toBe('Hello');
  });

  it('triggers a write (immediate) — equivalent to the old onChange fire', () => {
    renderer.setFieldValue('title', 'Hello');
    // setFieldValue calls store.setField with immediate: true → write fn is called.
    expect(write).toHaveBeenCalled();
  });

  it('promotes a hidden field when value becomes non-empty', () => {
    expect(container.querySelector('.chip[data-key="title"]')).toBeNull();
    renderer.setFieldValue('title', 'Hello');
    expect(container.querySelector('.chip[data-key="title"]')).not.toBeNull();
  });

  it('preserves empty string as a value (does not delete)', () => {
    renderer.setFieldValue('title', 'Hello');
    renderer.setFieldValue('title', '');
    const values = renderer.getValues();
    expect(Object.prototype.hasOwnProperty.call(values, 'title')).toBe(true);
    expect(values.title).toBe('');
  });

  it('deletes the field when value is undefined', () => {
    renderer.setFieldValue('title', 'Hello');
    renderer.setFieldValue('title', undefined);
    const values = renderer.getValues();
    expect(Object.prototype.hasOwnProperty.call(values, 'title')).toBe(false);
  });
});
