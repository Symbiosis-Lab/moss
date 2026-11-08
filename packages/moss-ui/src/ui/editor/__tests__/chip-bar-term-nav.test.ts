import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { ChipBar } from '../chip-bar';
import { FrontmatterStore } from '../frontmatter-store';
import type { ContentSchema } from '@symbiosis-lab/moss-ui';
import { host } from "../../host";

// Term navigation (cmd+click on a name chip): claimed terms open the claiming
// page's source, unclaimed terms navigate the main preview to the generated
// URL.
//
// Which fields carry terms comes from the build's kinds table, never from a
// field-name list here — a site declaring `[terms.people] fields = ["author",
// "editor"]` makes `editor:` a term field and `/people/` its namespace.

const resolveTerm = vi.fn();
const emitSpy = vi.spyOn(host.bus, 'emit');

// The two built-in kinds, as a build writes them for a site that declares
// nothing. Names stay empty so datalist completion is inert unless a test
// asks for it.
const BUILT_IN_KINDS = {
  authors: { fields: ['author'], names: [] as string[] },
  tags: { fields: ['tags'], names: [] as string[] },
};

function mockInvoke(
  resolveImpl: (args: { field: string; name: string }) => Promise<unknown>,
  kinds: Record<string, { fields: string[]; names: string[] }> = BUILT_IN_KINDS,
): void {
  host.engine.call = async (cmd) => (cmd === 'list_vault_terms' ? { kinds } : undefined) as never;
  resolveTerm.mockImplementation(async (field: string, name: string) => ({ status: 'ok', data: await resolveImpl({ field, name }) }));
}

class MockResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
  constructor(_cb: () => void) {}
}
vi.stubGlobal('ResizeObserver', MockResizeObserver);

function makeSchema(): ContentSchema {
  return {
    generator: 'moss',
    version: '1.0',
    frontmatter: {
      fields: {
        author: {
          type: 'oneOf',
          widget: 'tag-input',
          description: '',
          priority: 70,
          group: 'This Page',
        } as any,
        editor: {
          type: 'oneOf',
          widget: 'tag-input',
          description: '',
          priority: 71,
          group: 'This Page',
        } as any,
        title: {
          type: 'string',
          widget: 'text-input',
          description: '',
          priority: 10,
          group: 'This Page',
        } as any,
        tags: {
          type: 'array',
          widget: 'tag-input',
          description: '',
          priority: 80,
          group: 'This Page',
        } as any,
      },
      internal_fields: [],
    },
    shortcodes: null,
  } as ContentSchema;
}

function metaClick(el: Element): void {
  el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, metaKey: true }));
}

async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

function resolveTermCalls(): unknown[][] {
  return resolveTerm.mock.calls.map(([field, name]) => ['resolve_term', { field, name }]);
}

describe('ChipBar term navigation', () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
    resolveTerm.mockReset();
    host.resolveTerm = resolveTerm;
    mockInvoke(() => Promise.resolve(undefined));
    emitSpy.mockClear();
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  // `render()` refreshes the kinds table; cmd+click reads it synchronously,
  // so every mount here settles that fetch first, exactly as a file-open does
  // long before the user reaches for a chip.
  async function mount(values: Record<string, unknown>): Promise<ChipBar> {
    const store = new FrontmatterStore({ initial: values, requestSave: vi.fn() });
    const bar = new ChipBar(makeSchema(), store);
    bar.render(container);
    await flush();
    return bar;
  }

  function chip(key: string): HTMLElement {
    const el = container.querySelector(`.chip[data-key="${key}"]`);
    expect(el).not.toBeNull();
    return el as HTMLElement;
  }

  it('cmd+click on a claimed author opens the claiming source file', async () => {
    mockInvoke(() => Promise.resolve({ claimed_by: '/vault/關於/ma.md', url: '/authors/馬欣宜/' }));
    const bar = await mount({ author: '馬欣宜' });
    const selected = vi.fn();
    document.addEventListener('editor:file-selected', selected);

    metaClick(chip('author'));
    await flush();

    expect(resolveTermCalls()).toEqual([['resolve_term', { field: 'author', name: '馬欣宜' }]]);
    expect(selected).toHaveBeenCalledTimes(1);
    expect((selected.mock.calls[0][0] as CustomEvent).detail).toEqual({
      filePath: '/vault/關於/ma.md',
    });
    expect(emitSpy).not.toHaveBeenCalled();
    document.removeEventListener('editor:file-selected', selected);
    bar.destroy();
  });

  it('in a host that cannot resolve terms, cmd+click is an ordinary click', async () => {
    host.resolveTerm = undefined;
    mockInvoke(() => Promise.resolve({ claimed_by: '/vault/關於/ma.md', url: '/authors/馬欣宜/' }));
    const bar = await mount({ author: '馬欣宜' });
    const e = new MouseEvent('click', { bubbles: true, cancelable: true, metaKey: true });
    chip('author').dispatchEvent(e);
    await flush();
    expect(e.defaultPrevented).toBe(false);
    expect(resolveTermCalls()).toEqual([]);
    bar.destroy();
  });

  it('cmd+click on an unclaimed tag navigates the preview to the generated URL', async () => {
    mockInvoke(() => Promise.resolve({ claimed_by: null, url: '/tags/城市/' }));
    const bar = await mount({ tags: ['城市', '散文'] });

    metaClick(chip('tags'));
    await flush();

    // The bar chip resolves the field's first value.
    expect(resolveTermCalls()).toEqual([['resolve_term', { field: 'tags', name: '城市' }]]);
    expect(emitSpy).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/tags/城市/' });
    bar.destroy();
  });

  it('cmd+click on an individual tag token resolves that token, not the first value', async () => {
    mockInvoke(() => Promise.resolve({ claimed_by: null, url: '/tags/散文/' }));
    const bar = await mount({ tags: ['城市', '散文'] });
    // Open the tag editor popover, then meta-click the second token.
    chip('tags').dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    const tokens = document.querySelectorAll('.pop-tag');
    expect(tokens.length).toBe(2);
    metaClick(tokens[1]);
    await flush();

    expect(resolveTermCalls()).toEqual([['resolve_term', { field: 'tags', name: '散文' }]]);
    expect(emitSpy).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/tags/散文/' });
    bar.destroy();
  });

  it('a plain click still opens the chip editor, never resolving a term', async () => {
    const bar = await mount({ author: '馬欣宜' });
    chip('author').dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    expect(resolveTermCalls()).toEqual([]);
    // The popover overlay mounts at document level.
    expect(document.querySelector('.pop-input')).not.toBeNull();
    bar.destroy();
  });

  it('a field no kind feeds is not a term gesture — the editor opens instead', async () => {
    const bar = await mount({ title: 'A piece' });

    metaClick(chip('title'));
    await flush();

    expect(resolveTermCalls()).toEqual([]);
    expect(emitSpy).not.toHaveBeenCalled();
    expect(document.querySelector('.pop-input')).not.toBeNull();
    bar.destroy();
  });

  it('a declared kind makes editor: a term field, resolving into its namespace', async () => {
    mockInvoke(
      () => Promise.resolve({ claimed_by: null, url: '/people/馬欣宜/' }),
      { people: { fields: ['author', 'editor'], names: [] }, tags: { fields: ['tags'], names: [] } },
    );
    const bar = await mount({ editor: '馬欣宜' });

    metaClick(chip('editor'));
    await flush();

    expect(resolveTermCalls()).toEqual([['resolve_term', { field: 'editor', name: '馬欣宜' }]]);
    expect(emitSpy).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/people/馬欣宜/' });
    bar.destroy();
  });

  it('completion offers the kind\'s whole name list to every field it feeds', async () => {
    mockInvoke(
      () => Promise.resolve(undefined),
      { people: { fields: ['author', 'editor'], names: ['馬欣宜', '顧玉玲'] }, tags: { fields: ['tags'], names: ['城市'] } },
    );
    const bar = await mount({ editor: ['馬欣宜'] });

    chip('editor').dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));

    const options = () => [...document.querySelectorAll('datalist option')].map((o) => (o as HTMLOptionElement).value);
    await vi.waitFor(() => expect(options()).toEqual(['馬欣宜', '顧玉玲']));
    bar.destroy();
  });

  it('an Err from resolve (no kind feeds the field) does nothing', async () => {
    mockInvoke(() => Promise.reject("field 'author' belongs to no term kind"));
    const bar = await mount({ author: 'X' });
    const selected = vi.fn();
    document.addEventListener('editor:file-selected', selected);

    metaClick(chip('author'));
    await flush();

    expect(selected).not.toHaveBeenCalled();
    expect(emitSpy).not.toHaveBeenCalled();
    document.removeEventListener('editor:file-selected', selected);
    bar.destroy();
  });
});
