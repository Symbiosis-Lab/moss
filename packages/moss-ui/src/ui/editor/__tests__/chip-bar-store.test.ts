// Smoke tests for ChipBar + FrontmatterStore wiring.
// Verifies the store is the single source of truth for frontmatter values:
// ChipBar subscribes to the store and re-renders on changes; edits flow
// through store.setField; external updates flow through store.onFileChanged.

import { describe, it, expect, vi } from 'vitest';
import { ChipBar } from '../chip-bar';
import { FrontmatterStore } from '../frontmatter-store';

// jsdom does not implement ResizeObserver; ChipBar.renderChips uses it.
class StubResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as any).ResizeObserver = StubResizeObserver;

/** Minimal ContentSchema that satisfies ChipBar's rendering path. */
const MINIMAL_SCHEMA: any = {
  generator: 'test',
  version: '1',
  shortcodes: { delimiters: [':::', ':::'], definitions: {} },
  frontmatter: {
    fields: {
      title: { type: 'string', widget: 'text-input', priority: 10 },
      draft: { type: 'boolean', widget: 'checkbox', priority: 60 },
    },
    internal_fields: [],
  },
};

describe('ChipBar + FrontmatterStore wiring', () => {
  it('renders without error when given a store', () => {
    const store = new FrontmatterStore({
      initial: { title: 'Test' },
      requestSave: vi.fn().mockResolvedValue(undefined),
    });
    const bar = new ChipBar(MINIMAL_SCHEMA, store);
    const container = document.createElement('div');
    bar.render(container);
    // At least the spacer + unified button is rendered.
    expect(container.childElementCount).toBeGreaterThan(0);
    // Chip for `title` is present.
    expect(container.querySelector('.chip[data-key="title"]')).not.toBeNull();
    bar.destroy();
    store.destroy();
  });

  it('store.setField notifies ChipBar subscriber — chip re-renders', () => {
    const write = vi.fn().mockResolvedValue(undefined);
    const store = new FrontmatterStore({
      initial: { title: 'A' },
      requestSave: write,
    });
    const bar = new ChipBar(MINIMAL_SCHEMA, store);
    const container = document.createElement('div');
    bar.render(container);

    // Initial state: title chip is rendered.
    expect(container.querySelector('.chip[data-key="title"]')).not.toBeNull();

    // External setField on the store (simulate: user edits via the store directly,
    // or store receives a debounced-then-resolved write). The subscriber should
    // trigger re-render.
    store.setField('title', 'B');

    // The chip bar should still render the title chip (value changed to 'B').
    expect(container.querySelector('.chip[data-key="title"]')).not.toBeNull();
    expect(bar.getValues().title).toBe('B');

    bar.destroy();
    store.destroy();
  });

  it('store.onFileChanged updates ChipBar without scheduling a write', () => {
    const write = vi.fn().mockResolvedValue(undefined);
    const store = new FrontmatterStore({
      initial: { title: 'Original' },
      requestSave: write,
    });
    const bar = new ChipBar(MINIMAL_SCHEMA, store);
    const container = document.createElement('div');
    bar.render(container);

    write.mockClear();

    // Simulate an external file-changed event (e.g., git pull, external editor).
    store.onFileChanged({ title: 'Updated by disk' });

    // ChipBar reflects the updated value.
    expect(bar.getValues().title).toBe('Updated by disk');
    // No write should be scheduled — the values came FROM disk.
    expect(write).not.toHaveBeenCalled();

    bar.destroy();
    store.destroy();
  });

  it('destroying ChipBar unsubscribes from store — no further re-renders after destroy', () => {
    const write = vi.fn().mockResolvedValue(undefined);
    const store = new FrontmatterStore({
      initial: { title: 'A' },
      requestSave: write,
    });
    const bar = new ChipBar(MINIMAL_SCHEMA, store);
    const container = document.createElement('div');
    bar.render(container);

    // Destroy the chip bar — should unsubscribe from the store.
    bar.destroy();
    expect(container.innerHTML).toBe('');

    // Subsequent store changes should not throw (subscriber was removed).
    expect(() => store.setField('title', 'After destroy')).not.toThrow();

    store.destroy();
  });
});
