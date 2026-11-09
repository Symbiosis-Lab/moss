import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import { ChipBar } from '../chip-bar';
import { FrontmatterStore } from '../frontmatter-store';
import type { ContentSchema } from '@symbiosis-lab/moss-ui';

// One chip-list widget serves every name-list field, whichever of its two
// authored shapes the file holds: `author: 顧玉玲` and `author: [顧玉玲, 陳小華]`
// are the same field to the editor. The round trip is what this file is
// about — a one-name edit must write the bare string back, or opening a
// chip and closing it would silently rewrite the YAML of every single-author
// page in a vault.

class MockResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
  constructor(_cb: () => void) {}
}
vi.stubGlobal('ResizeObserver', MockResizeObserver);

function makeSchema(): ContentSchema {
  return {
    frontmatter: {
      fields: {
        // FieldType serializes `#[serde(rename_all = "lowercase")]` — 'oneof',
        // not 'oneOf'. author/editor/jury are the string-or-list union;
        // tags is a plain array and never collapses (see the tags case below).
        author: { type: 'oneof', widget: 'tag-input', priority: 70, group: 'This Page' },
        tags: { type: 'array', widget: 'tag-input', priority: 64, group: 'This Page' },
      },
    },
  } as unknown as ContentSchema;
}

describe('name-list chip round trip', () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  function mount(initial: Record<string, unknown>) {
    const store = new FrontmatterStore({ initial, requestSave: vi.fn().mockResolvedValue(undefined) });
    const setField = vi.spyOn(store, 'setField');
    const bar = new ChipBar(makeSchema(), store);
    bar.render(container);
    return { store, setField, bar };
  }

  function openEditor(key = 'author'): HTMLInputElement {
    (container.querySelector(`.chip[data-key="${key}"]`) as HTMLElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    return document.body.querySelector('.pop .pop-input') as HTMLInputElement;
  }

  function typeName(inp: HTMLInputElement, name: string): void {
    inp.value = name;
    inp.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  }

  test('a bare string renders as a chip list and opens the chip-list editor', () => {
    const { bar } = mount({ author: '顧玉玲' });

    const names = [...container.querySelectorAll('.chip[data-key="author"] .chip-accent')]
      .map((el) => el.textContent);
    expect(names).toEqual(['顧玉玲']);

    openEditor();
    expect(document.body.querySelector('.pop .pop-tags')).toBeTruthy();
    expect([...document.body.querySelectorAll('.pop-tag')].map((el) => el.textContent))
      .toEqual(['顧玉玲×']);
    bar.destroy();
  });

  test('a second name is written as a list', () => {
    const { setField, store, bar } = mount({ author: '顧玉玲' });
    typeName(openEditor(), '陳小華');

    expect(setField).toHaveBeenLastCalledWith('author', ['顧玉玲', '陳小華'], { immediate: true });
    expect(store.getValues().author).toEqual(['顧玉玲', '陳小華']);
    bar.destroy();
  });

  test('editing back down to one name writes a bare string, not a one-element list', () => {
    const { setField, store, bar } = mount({ author: ['顧玉玲', '陳小華'] });
    // Remove the second name through its × — the same gesture the tag editor
    // has always had, now on a field whose file shape is a scalar.
    const removes = document.body.querySelector('.pop-tag-x') as HTMLElement | null;
    expect(removes).toBeNull(); // the popover is not open yet
    openEditor();
    const xs = [...document.body.querySelectorAll('.pop-tag-x')] as HTMLElement[];
    xs[1].dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));

    expect(setField).toHaveBeenLastCalledWith('author', '顧玉玲', { immediate: true });
    expect(store.getValues().author).toBe('顧玉玲');
    bar.destroy();
  });

  test('the first name typed into an empty field is written as a bare string', () => {
    const { setField, bar } = mount({ author: [] });
    typeName(openEditor(), '顧玉玲');

    expect(setField).toHaveBeenLastCalledWith('author', '顧玉玲', { immediate: true });
    bar.destroy();
  });

  test('tags: the first tag typed into an empty field stays a list, not a bare string', () => {
    // tags is a plain array upstream, not the oneof union author/editor/jury
    // share — a bare `tags: yoga` fails the build's "expected array" check.
    const { setField, store, bar } = mount({ tags: [] });
    typeName(openEditor('tags'), 'yoga');

    expect(setField).toHaveBeenLastCalledWith('tags', ['yoga'], { immediate: true });
    expect(store.getValues().tags).toEqual(['yoga']);
    bar.destroy();
  });
});
