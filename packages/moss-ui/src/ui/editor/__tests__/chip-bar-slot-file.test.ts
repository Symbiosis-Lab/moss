/**
 * The chip bar's read-only state for a layout slot file (`footer.md`).
 *
 * No frontmatter field changes how a footer renders — the slot collector reads
 * only `slot:` (redundant on a root footer.md), the language bucket comes from
 * the PATH, and `slot_only` already forces `draft`/`listed` false. So the bar
 * shows one inert chip and no add-property button; offering `+` would invite
 * fields that silently do nothing.
 *
 * The load-bearing test here is the last one: hiding the chips must not delete
 * the frontmatter. `saveActiveDocument` serializes the store's getValues() over
 * the whole file, so if the store were dropped (or emptied) for slot files, the
 * first autosave would strip the author's frontmatter block — and frontmatter
 * is not visible in CM6, so they would never see it go.
 */
import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import { ChipBar } from '../chip-bar';
import { FrontmatterStore } from '../frontmatter-store';
import type { ContentSchema } from '@symbiosis-lab/moss-ui';

function makeSchema(fields: Record<string, any>): ContentSchema {
  return { frontmatter: { fields } } as ContentSchema;
}

function makeStore(initial: Record<string, unknown> = {}) {
  const write = vi.fn().mockResolvedValue(undefined);
  return { store: new FrontmatterStore({ initial, requestSave: write }), write };
}

class MockResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
  constructor(_cb: () => void) {}
}
vi.stubGlobal('ResizeObserver', MockResizeObserver);

describe('ChipBar — slot file (read-only bar)', () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
  });
  afterEach(() => {
    document.body.innerHTML = '';
  });

  test('renders one static chip and no add-property button', () => {
    const { store } = makeStore({});
    const bar = new ChipBar(makeSchema({}), store, {
      slotRole: { slot: 'footer-left', language: null },
    });
    bar.render(container);

    const chips = container.querySelectorAll('.chip');
    expect(chips.length).toBe(1);
    expect(chips[0].classList.contains('chip--static')).toBe(true);
    expect(container.querySelector('.chip-unified-btn')).toBeNull();
  });

  test('the static chip carries a portal tooltip, not a title attribute', () => {
    const { store } = makeStore({});
    const bar = new ChipBar(makeSchema({}), store, {
      slotRole: { slot: 'footer-left', language: null },
    });
    bar.render(container);

    const chip = container.querySelector('.chip--static') as HTMLElement;
    expect(chip.getAttribute('data-tooltip')).toBeTruthy();
    expect(chip.hasAttribute('title')).toBe(false);
  });

  test('a language-scoped slot file names its language bucket', () => {
    const { store } = makeStore({});
    const bar = new ChipBar(makeSchema({}), store, {
      slotRole: { slot: 'footer-left', language: 'zh-hans' },
    });
    bar.render(container);

    const chip = container.querySelector('.chip--static') as HTMLElement;
    expect(chip.textContent).toContain('简体中文');
  });

  test('existing frontmatter is hidden but NOT dropped from the store', () => {
    // A footer.md carrying leftover frontmatter from the old design-doc example.
    const initial = { title: 'Footer', heading: false };
    const { store } = makeStore(initial);
    const bar = new ChipBar(makeSchema({}), store, {
      slotRole: { slot: 'footer-left', language: null },
    });
    bar.render(container);

    // Hidden from the bar…
    expect(container.querySelectorAll('.chip').length).toBe(1);
    // …but still round-tripping through the store, which is what a save writes.
    expect(store.getValues()).toEqual(initial);
  });

  test('a normal page is unaffected — chips and the + button still render', () => {
    const { store } = makeStore({ title: 'Hello' });
    const bar = new ChipBar(
      makeSchema({ title: { field_type: 'String', widget: 'TextInput', priority: 10 } }),
      store,
      {},
    );
    bar.render(container);

    expect(container.querySelector('.chip--static')).toBeNull();
    expect(container.querySelector('.chip-unified-btn')).not.toBeNull();
  });
});
