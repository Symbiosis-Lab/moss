// Tests for autocorrect/autocapitalize/spellcheck policy on chip-bar inputs.
// The design:
//   - ALL inputs: autocorrect=off (OS autocorrect silently rewrites values).
//   - text-area (prose widget): keep autocapitalize + spellcheck.
//   - text-input, tag-input, structured pickers (identifier widgets): full
//     suppression — autocorrect=off, autocapitalize=off, spellcheck=false,
//     autocomplete=off.
//   - omenu-search: identifier — full suppression.

import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import { ChipBar } from '../chip-bar';
import { FrontmatterStore } from '../frontmatter-store';
import type { ContentSchema } from '@symbiosis-lab/moss-ui';
import { setLocale } from "../../i18n/index";

function makeSchema(fields: Record<string, any>): ContentSchema {
  return {
    frontmatter: { fields },
  } as ContentSchema;
}

function makeStore(initial: Record<string, unknown> = {}) {
  const write = vi.fn().mockResolvedValue(undefined);
  const store = new FrontmatterStore({ initial, requestSave: write });
  return { store, write };
}

class MockResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
  constructor(_cb: () => void) {}
}
vi.stubGlobal('ResizeObserver', MockResizeObserver);

describe('ChipBar autocorrect policy', () => {
  let container: HTMLElement;

  beforeEach(() => {
    setLocale('en');
    container = document.createElement('div');
    document.body.appendChild(container);
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  // ── text-input (identifier) fields ──────────────────────────────────

  test('url field (text-input) popover input has autocorrect=off, autocapitalize=off, spellcheck=false', () => {
    const schema = makeSchema({
      url: { type: 'string', widget: 'text-input', priority: 40 },
    });
    const { store } = makeStore({ url: '/my-post' });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chip = container.querySelector('.chip[data-key="url"]') as HTMLElement;
    chip.click();

    const pop = document.body.querySelector('.pop') as HTMLElement;
    const input = pop.querySelector('input') as HTMLInputElement;

    expect(input.getAttribute('autocorrect')).toBe('off');
    expect(input.getAttribute('autocapitalize')).toBe('off');
    expect(input.spellcheck).toBe(false);
    expect(input.autocomplete).toBe('off');

    renderer.destroy();
  });

  test('lang field (text-input) popover input has full identifier suppression', () => {
    const schema = makeSchema({
      lang: { type: 'string', widget: 'text-input', priority: 70 },
    });
    const { store } = makeStore({ lang: 'en' });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chip = container.querySelector('.chip[data-key="lang"]') as HTMLElement;
    chip.click();

    const pop = document.body.querySelector('.pop') as HTMLElement;
    const input = pop.querySelector('input') as HTMLInputElement;

    expect(input.getAttribute('autocorrect')).toBe('off');
    expect(input.getAttribute('autocapitalize')).toBe('off');
    expect(input.spellcheck).toBe(false);
    expect(input.autocomplete).toBe('off');

    renderer.destroy();
  });

  test('translationKey field (text-input) popover input has full identifier suppression', () => {
    const schema = makeSchema({
      translationKey: { type: 'string', widget: 'text-input', priority: 80 },
    });
    const { store } = makeStore({ translationKey: 'my-post' });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chip = container.querySelector('.chip[data-key="translationKey"]') as HTMLElement;
    chip.click();

    const pop = document.body.querySelector('.pop') as HTMLElement;
    const input = pop.querySelector('input') as HTMLInputElement;

    expect(input.getAttribute('autocorrect')).toBe('off');
    expect(input.getAttribute('autocapitalize')).toBe('off');
    expect(input.spellcheck).toBe(false);
    expect(input.autocomplete).toBe('off');

    renderer.destroy();
  });

  test('title field (text-input) popover input has full identifier suppression (widget-type discriminator)', () => {
    const schema = makeSchema({
      title: { type: 'string', widget: 'text-input', priority: 10 },
    });
    const { store } = makeStore({ title: 'My Post' });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chip = container.querySelector('.chip[data-key="title"]') as HTMLElement;
    chip.click();

    const pop = document.body.querySelector('.pop') as HTMLElement;
    const input = pop.querySelector('input') as HTMLInputElement;

    // text-input is the identifier widget: full suppression.
    expect(input.getAttribute('autocorrect')).toBe('off');
    expect(input.getAttribute('autocapitalize')).toBe('off');
    expect(input.spellcheck).toBe(false);
    expect(input.autocomplete).toBe('off');

    renderer.destroy();
  });

  // ── text-area (prose) fields ─────────────────────────────────────────

  test('description field (text-area) popover textarea has autocorrect=off but keeps autocapitalize + spellcheck', () => {
    const schema = makeSchema({
      description: { type: 'string', widget: 'text-area', priority: 20 },
    });
    const { store } = makeStore({ description: 'Some excerpt' });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chip = container.querySelector('.chip[data-key="description"]') as HTMLElement;
    chip.click();

    const pop = document.body.querySelector('.pop') as HTMLElement;
    const textarea = pop.querySelector('textarea') as HTMLTextAreaElement;

    // Prose widget: autocorrect off, but autocapitalize and spellcheck kept.
    expect(textarea.getAttribute('autocorrect')).toBe('off');
    expect(textarea.getAttribute('autocapitalize')).toBe('sentences');
    expect(textarea.spellcheck).toBe(true);

    renderer.destroy();
  });

  // ── tag-input (identifier) ───────────────────────────────────────────

  test('tags field (tag-input) add-tag input has full identifier suppression', () => {
    const schema = makeSchema({
      tags: { type: 'array', widget: 'tag-input', items: { type: 'string' }, priority: 50 },
    });
    const { store } = makeStore({ tags: ['hello'] });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chip = container.querySelector('.chip[data-key="tags"]') as HTMLElement;
    chip.click();

    const pop = document.body.querySelector('.pop') as HTMLElement;
    const input = pop.querySelector('input') as HTMLInputElement;

    expect(input.getAttribute('autocorrect')).toBe('off');
    expect(input.getAttribute('autocapitalize')).toBe('off');
    expect(input.spellcheck).toBe(false);
    expect(input.autocomplete).toBe('off');

    renderer.destroy();
  });

  // ── omenu-search ─────────────────────────────────────────────────────

  test('omenu-search input has autocorrect=off, autocapitalize=off, spellcheck=false', () => {
    const schema = makeSchema({
      // One unset schema field so the search box appears in the unified dropdown.
      title: { type: 'string', widget: 'text-input', priority: 10, group: 'Common' },
    });
    // No values set so 'title' appears in the "add property" list.
    const { store } = makeStore({});
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    // Open the unified "+" dropdown.
    const btn = container.querySelector('.chip-unified-btn') as HTMLElement;
    btn.click();

    const menu = document.body.querySelector('.omenu') as HTMLElement;
    expect(menu).not.toBeNull();

    const search = menu.querySelector('.omenu-search') as HTMLInputElement;
    expect(search).not.toBeNull();

    expect(search.getAttribute('autocorrect')).toBe('off');
    expect(search.getAttribute('autocapitalize')).toBe('off');
    expect(search.spellcheck).toBe(false);
    expect(search.autocomplete).toBe('off');

    renderer.destroy();
  });
});
