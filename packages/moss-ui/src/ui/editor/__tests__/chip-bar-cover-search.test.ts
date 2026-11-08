import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { ChipBar } from '../chip-bar';
import { FrontmatterStore } from '../frontmatter-store';
import type { ContentSchema } from '@symbiosis-lab/moss-ui';
import { engine } from '../../engine';

// Mock the engine — the cover field's edit input drives the SAME
// backend ranker as the CM6 `![[asset]]` wikilink completion
// (`engine.wikilinkCompletions`), plus the existing asset-resolve RPC used
// by the read-only chip display.
vi.mock('../../engine', () => {
  const editorResolveAsset = vi.fn();
  const wikilinkCompletions = vi.fn();
  return {
    engine: { editorResolveAsset, wikilinkCompletions },
  };
});

// ── ResizeObserver mock (jsdom doesn't have it) ──────────────────────────
class MockResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
  constructor(_cb: () => void) {}
}
vi.stubGlobal('ResizeObserver', MockResizeObserver);

function makeFilePickerSchema(): ContentSchema {
  return {
    generator: 'moss',
    version: '1.0',
    frontmatter: {
      fields: {
        cover: {
          type: 'string',
          widget: 'file-picker',
          description: 'Cover image path',
          priority: 60,
          group: 'Occasional',
        } as any,
      },
      internal_fields: [],
    },
    shortcodes: null,
  } as ContentSchema;
}

/**
 * Advance past the file-picker search debounce (SEARCH_DEBOUNCE_MS = 150 in
 * chip-bar.ts) so a search triggered by an 'input' event has fired, and drain
 * the mocked RPC promises it awaits.
 *
 * FAKE timers: the real-timer form cost 200 ms per call across ~23 calls
 * (4.6 s of the frontend suite for 19 tests that assert no timing at all).
 * The async variant is required — `runSearch` awaits
 * `engine.wikilinkCompletions`, and the sync variant would not drain it.
 */
async function flush(): Promise<void> {
  await vi.advanceTimersByTimeAsync(200);
}

describe('ChipBar cover field — file-picker edit search', () => {
  let container: HTMLElement;

  beforeEach(() => {
    vi.useFakeTimers();
    container = document.createElement('div');
    document.body.appendChild(container);
    vi.mocked(engine.editorResolveAsset).mockReset().mockResolvedValue({ status: 'ok', data: null } as any);
    vi.mocked(engine.wikilinkCompletions).mockReset().mockResolvedValue({ status: 'ok', data: [] } as any);
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.innerHTML = '';
  });

  function openCoverEditor(initial: Record<string, unknown>) {
    const store = new FrontmatterStore({ initial, requestSave: vi.fn().mockResolvedValue(undefined) });
    const bar = new ChipBar(makeFilePickerSchema(), store, { getActivePath: () => '/x/post.md' });
    bar.render(container);
    const chip = container.querySelector('.chip[data-key="cover"]') as HTMLElement;
    chip.click();
    const input = document.body.querySelector('.pop .pop-input') as HTMLInputElement;
    return { store, bar, input };
  }

  it('offers Browse only where the host can turn a picked file into a reference', async () => {
    const { host } = await import("../../host");
    const kept = host.assetRefForPath;
    try {
      host.assetRefForPath = async () => null;
      const offered = openCoverEditor({ cover: 'a.jpg' });
      expect(document.body.querySelector('.pop .asset-browse')).not.toBeNull();
      offered.bar.destroy();
      document.body.innerHTML = '';
      container = document.createElement('div');
      document.body.appendChild(container);

      host.assetRefForPath = undefined;
      const absent = openCoverEditor({ cover: 'a.jpg' });
      expect(document.body.querySelector('.pop .asset-browse')).toBeNull();
      expect(document.body.querySelector('.pop .pop-input--has-browse')).toBeNull();
      absent.bar.destroy();
    } finally {
      host.assetRefForPath = kept;
    }
  });

  it('pre-fills the edit input with the unwrapped value (strips [[ ]])', () => {
    const { input, bar } = openCoverEditor({ cover: '[[DSCF4053.jpeg]]' });
    expect(input).not.toBeNull();
    expect(input.value).toBe('DSCF4053.jpeg');
    bar.destroy();
  });

  it('opening the popover immediately searches with the current value (browse-on-open)', async () => {
    const { bar } = openCoverEditor({ cover: '[[DSCF4053.jpeg]]' });
    await flush();
    expect(vi.mocked(engine.wikilinkCompletions)).toHaveBeenCalledWith(
      'DSCF4053.jpeg', '/x/post.md', 'embed', null, null,
    );
    bar.destroy();
  });

  it('renders whatever the (server-side-filtered) RPC returns, with no client-side kind filter', async () => {
    // The backend now does the asset-vs-page filtering via `allowed_kinds`
    // (schema `file_kinds`) — chip-bar no longer post-filters to
    // `kind === 'asset'`. A page slipping through here (e.g. an unrestricted
    // field) is rendered as-is; that's the RPC's contract to honor, not
    // chip-bar's job to re-enforce.
    vi.mocked(engine.wikilinkCompletions).mockResolvedValue({
      status: 'ok',
      data: [
        { insert: 'DSCF4053.jpeg', label: 'DSCF4053', detail: '.jpeg', kind: 'asset' },
        { insert: 'DSCF4099.png', label: 'DSCF4099', detail: '.png', kind: 'asset' },
      ],
    } as any);
    const { input, bar } = openCoverEditor({ cover: '' });
    input.value = 'DSC';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();

    const items = document.body.querySelectorAll('.pop-file-picker-item');
    expect(items.length).toBe(2);
    expect(items[0].textContent).toContain('DSCF4053');
    expect(items[1].textContent).toContain('DSCF4099');
    bar.destroy();
  });

  it('shows an empty-state message and no rows when nothing matches', async () => {
    vi.mocked(engine.wikilinkCompletions).mockResolvedValue({ status: 'ok', data: [] } as any);
    const { input, bar } = openCoverEditor({ cover: '' });
    input.value = 'zzz-nope';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();

    expect(document.body.querySelectorAll('.pop-file-picker-item').length).toBe(0);
    expect(document.body.querySelector('.pop-file-picker-empty')).not.toBeNull();
    bar.destroy();
  });

  it('mousedown on a result writes the wrapped wikilink value, closes the popover, and preventDefaults', async () => {
    vi.mocked(engine.wikilinkCompletions).mockResolvedValue({
      status: 'ok',
      data: [{ insert: 'DSCF4053.jpeg', label: 'DSCF4053', detail: '.jpeg', kind: 'asset' }],
    } as any);
    const { input, store, bar } = openCoverEditor({ cover: '' });
    input.value = 'DSC';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();

    const item = document.body.querySelector('.pop-file-picker-item') as HTMLElement;
    expect(item).not.toBeNull();
    // Selection commits on MOUSEDOWN + preventDefault, not click — the real
    // WebKit webview replaces the hovered row mid-press so a `click` is never
    // synthesized (jsdom's `.click()` also fires no mousedown). Dispatch a
    // cancelable mousedown and assert the handler called preventDefault, which
    // is what keeps the input focused and lands the pick.
    const md = new MouseEvent('mousedown', { bubbles: true, cancelable: true });
    item.dispatchEvent(md);
    expect(md.defaultPrevented).toBe(true);

    expect(store.getValues().cover).toBe('[[DSCF4053.jpeg]]');
    expect(document.body.querySelector('.pop')).toBeNull();
    bar.destroy();
  });

  it('hover then select the SAME node still commits (node-churn guard)', async () => {
    // Regression guard: the old mouseenter handler rebuilt the whole list
    // (innerHTML = ''), destroying the row under the cursor between press and
    // release so WebKit never fired a click. Now hover updates in place, so the
    // node the pointer is on survives to receive the mousedown.
    vi.mocked(engine.wikilinkCompletions).mockResolvedValue({
      status: 'ok',
      data: [
        { insert: 'a.jpeg', label: 'a', detail: '.jpeg', kind: 'asset' },
        { insert: 'b.jpeg', label: 'b', detail: '.jpeg', kind: 'asset' },
      ],
    } as any);
    const { input, store, bar } = openCoverEditor({ cover: '' });
    input.value = 'j';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();

    const before = document.body.querySelectorAll('.pop-file-picker-item')[1] as HTMLElement;
    before.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }));

    // Re-query AFTER the hover: if the node were rebuilt, this would be a
    // different element and the mousedown below would land on a stale node.
    const after = document.body.querySelectorAll('.pop-file-picker-item')[1] as HTMLElement;
    expect(after).toBe(before); // hover must NOT rebuild the row node
    after.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));

    expect(store.getValues().cover).toBe('[[b.jpeg]]');
    expect(document.body.querySelector('.pop')).toBeNull();
    bar.destroy();
  });

  it('a path-qualified hit writes the resolver-correct reference, not the bare stem', async () => {
    // A frontmatter wikilink resolves like a body one:
    // `resolve_frontmatter_wikilinks` calls `graph.resolve_path(ref, source)`
    // on every frontmatter wikilink. Its filename branch is stem-keyed with
    // tiebreaks that always return SOME answer and never report ambiguity, so
    // a bare stem silently re-points the day a same-named file appears
    // elsewhere in the vault. The backend's `insert` names the file the author
    // chose, path-qualified because the query was.
    vi.mocked(engine.wikilinkCompletions).mockResolvedValue({
      status: 'ok',
      data: [
        {
          insert: '/people/頭像-李柏萱.png',
          label: '頭像-李柏萱.png',
          detail: '/people/頭像-李柏萱.png',
          kind: 'asset',
        },
      ],
    } as any);
    const { input, store, bar } = openCoverEditor({ cover: '' });
    input.value = 'people/頭';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();

    const row = document.body.querySelector('.pop-file-picker-item') as HTMLElement;
    row.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));

    expect(store.getValues().cover).toBe('[[/people/頭像-李柏萱.png]]');
    // …and the field shows what was written, so the author can see it.
    expect(input.value).toBe('/people/頭像-李柏萱.png');
    bar.destroy();
  });

  it('the input keeps focus through a mouseenter and through ArrowDown nav', async () => {
    // The keyboard-nav (ArrowUp/Down/Enter) is bound to the INPUT, so it only
    // works while the input stays focused. A hover or an arrow keypress must
    // never move focus away (a mousedown without preventDefault on a
    // non-focusable <div> row would blur to document.body on the real webview).
    vi.mocked(engine.wikilinkCompletions).mockResolvedValue({
      status: 'ok',
      data: [
        { insert: 'a.jpeg', label: 'a', detail: '.jpeg', kind: 'asset' },
        { insert: 'b.jpeg', label: 'b', detail: '.jpeg', kind: 'asset' },
      ],
    } as any);
    const { input, bar } = openCoverEditor({ cover: '' });
    input.value = 'j';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();

    input.focus();
    expect(document.activeElement).toBe(input);

    const row = document.body.querySelectorAll('.pop-file-picker-item')[1] as HTMLElement;
    row.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }));
    expect(document.activeElement).toBe(input); // hover kept focus

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    expect(document.activeElement).toBe(input); // ArrowDown kept focus
    bar.destroy();
  });

  it('keyboard: ArrowDown highlights the next item, Enter selects it', async () => {
    vi.mocked(engine.wikilinkCompletions).mockResolvedValue({
      status: 'ok',
      data: [
        { insert: 'a.jpeg', label: 'a', detail: '.jpeg', kind: 'asset' },
        { insert: 'b.jpeg', label: 'b', detail: '.jpeg', kind: 'asset' },
      ],
    } as any);
    const { input, store, bar } = openCoverEditor({ cover: '' });
    input.value = 'j';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    let items = document.body.querySelectorAll('.pop-file-picker-item');
    expect(items[0].classList.contains('is-highlighted')).toBe(false);
    expect(items[1].classList.contains('is-highlighted')).toBe(true);

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(store.getValues().cover).toBe('[[b.jpeg]]');
    bar.destroy();
  });

  it('keyboard: ArrowDown wraps from the last item back to the first', async () => {
    vi.mocked(engine.wikilinkCompletions).mockResolvedValue({
      status: 'ok',
      data: [
        { insert: 'a.jpeg', label: 'a', detail: '.jpeg', kind: 'asset' },
        { insert: 'b.jpeg', label: 'b', detail: '.jpeg', kind: 'asset' },
      ],
    } as any);
    const { input, bar } = openCoverEditor({ cover: '' });
    input.value = 'j';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();

    // Down, Down: 0 -> 1 -> wraps to 0.
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    const items = document.body.querySelectorAll('.pop-file-picker-item');
    expect(items[0].classList.contains('is-highlighted')).toBe(true);
    expect(items[1].classList.contains('is-highlighted')).toBe(false);
    bar.destroy();
  });

  it('keyboard: ArrowUp from the first item wraps to the last', async () => {
    vi.mocked(engine.wikilinkCompletions).mockResolvedValue({
      status: 'ok',
      data: [
        { insert: 'a.jpeg', label: 'a', detail: '.jpeg', kind: 'asset' },
        { insert: 'b.jpeg', label: 'b', detail: '.jpeg', kind: 'asset' },
      ],
    } as any);
    const { input, store, bar } = openCoverEditor({ cover: '' });
    input.value = 'j';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();

    // Starting highlight is index 0 — ArrowUp must wrap to the LAST item.
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }));
    const items = document.body.querySelectorAll('.pop-file-picker-item');
    expect(items[0].classList.contains('is-highlighted')).toBe(false);
    expect(items[1].classList.contains('is-highlighted')).toBe(true);

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(store.getValues().cover).toBe('[[b.jpeg]]');
    bar.destroy();
  });

  it('Enter during IME composition does not select the highlighted dropdown item', async () => {
    vi.mocked(engine.wikilinkCompletions).mockResolvedValue({
      status: 'ok',
      data: [{ insert: 'a.jpeg', label: 'a', detail: '.jpeg', kind: 'asset' }],
    } as any);
    const { input, store, bar } = openCoverEditor({ cover: '' });
    input.value = 'a';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush(); // typing itself already committed the raw '[[a]]' per-keystroke

    // A CJK IME's composition-commit Enter must NOT be treated as a real
    // Enter (matching the same guard in url-autocomplete.ts / cm-wikilink-complete):
    // it must NOT overwrite the value with the highlighted dropdown item
    // ('a.jpeg') or close the popover.
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, isComposing: true }));
    expect(store.getValues().cover).toBe('[[a]]');
    expect(document.body.querySelector('.pop')).not.toBeNull(); // popover stays open
    bar.destroy();
  });

  it('Enter with no dropdown matches and an EMPTY input REMOVES the key — no cover:"" residue', async () => {
    // Clearing removes the key (transactional-add decision: absence is the
    // canonical empty; `cover: ""` takes the cover branch with a broken path).
    vi.mocked(engine.wikilinkCompletions).mockResolvedValue({ status: 'ok', data: [] } as any);
    const { input, store, bar } = openCoverEditor({ cover: '[[old.jpg]]' });
    input.value = '';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect('cover' in store.getValues()).toBe(false);
    bar.destroy();
  });

  it('mouse hover (mouseenter) updates the highlighted item', async () => {
    vi.mocked(engine.wikilinkCompletions).mockResolvedValue({
      status: 'ok',
      data: [
        { insert: 'a.jpeg', label: 'a', detail: '.jpeg', kind: 'asset' },
        { insert: 'b.jpeg', label: 'b', detail: '.jpeg', kind: 'asset' },
      ],
    } as any);
    const { input, bar } = openCoverEditor({ cover: '' });
    input.value = 'j';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();

    let items = document.body.querySelectorAll('.pop-file-picker-item');
    items[1].dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }));

    items = document.body.querySelectorAll('.pop-file-picker-item');
    expect(items[1].classList.contains('is-highlighted')).toBe(true);
    expect(items[0].classList.contains('is-highlighted')).toBe(false);
    bar.destroy();
  });

  it('Escape closes the popover without writing a value', async () => {
    const { input, store, bar } = openCoverEditor({ cover: '[[a.jpeg]]' });
    await flush();
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(document.body.querySelector('.pop')).toBeNull();
    expect(store.getValues().cover).toBe('[[a.jpeg]]');
    bar.destroy();
  });

  it('Enter with no dropdown matches commits the raw typed value, wrapped as a wikilink', async () => {
    vi.mocked(engine.wikilinkCompletions).mockResolvedValue({ status: 'ok', data: [] } as any);
    const { input, store, bar } = openCoverEditor({ cover: '' });
    input.value = 'manual-name.jpg';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(store.getValues().cover).toBe('[[manual-name.jpg]]');
    expect(document.body.querySelector('.pop')).toBeNull();
    bar.destroy();
  });

  it('gracefully handles a failed completions RPC (no crash, empty dropdown)', async () => {
    vi.mocked(engine.wikilinkCompletions).mockRejectedValue(new Error('boom'));
    const { input, bar } = openCoverEditor({ cover: '' });
    input.value = 'x';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();

    expect(document.body.querySelectorAll('.pop-file-picker-item').length).toBe(0);
    bar.destroy();
  });

  it('passes the schema file_kinds through to wikilinkCompletions as allowed_kinds', async () => {
    const schema: ContentSchema = {
      generator: 'moss',
      version: '1.0',
      frontmatter: {
        fields: {
          cover: {
            type: 'string',
            widget: 'file-picker',
            description: 'Cover image path',
            priority: 60,
            group: 'Occasional',
            file_kinds: ['Image', 'Video'],
          } as any,
        },
        internal_fields: [],
      },
      shortcodes: null,
    } as ContentSchema;
    const store = new FrontmatterStore({ initial: { cover: '' }, requestSave: vi.fn().mockResolvedValue(undefined) });
    const bar = new ChipBar(schema, store, { getActivePath: () => '/x/post.md' });
    bar.render(container);
    const chip = container.querySelector('.chip[data-key="cover"]') as HTMLElement;
    chip.click();
    const input = document.body.querySelector('.pop .pop-input') as HTMLInputElement;
    input.value = 'DSC';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();
    expect(vi.mocked(engine.wikilinkCompletions)).toHaveBeenCalledWith(
      'DSC', '/x/post.md', 'embed', null, ['Image', 'Video'],
    );
    bar.destroy();
  });

  it('a stale in-flight response does not clobber a newer one (race guard)', async () => {
    // Open the popover and let its browse-on-open search settle first (using
    // the default empty-array mock) so it can't interfere with the two
    // queued responses below.
    const { input, bar } = openCoverEditor({ cover: '' });
    await flush();

    // Queue two responses: the FIRST search (fired after the first debounce
    // window elapses) resolves SLOWEST; the SECOND search (fired after a
    // later keystroke's own debounce window elapses, while the first is
    // still pending) resolves fast. The guard must keep the second (newer)
    // result even though the first (stale) one resolves later. Each keystroke
    // is followed by a full flush() (> SEARCH_DEBOUNCE_MS) so its debounce
    // timer actually fires the search rather than being cancelled by the
    // next keystroke.
    let resolveFirst: (v: any) => void = () => {};
    const firstPromise = new Promise((r) => { resolveFirst = r; });
    vi.mocked(engine.wikilinkCompletions)
      .mockImplementationOnce(() => firstPromise as any)
      .mockImplementationOnce(async () => ({
        status: 'ok',
        data: [{ insert: 'second.jpeg', label: 'second', detail: '.jpeg', kind: 'asset' }],
      } as any));

    input.value = 'f';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush(); // first search fires (slow — still pending)

    input.value = 'fi';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await flush(); // second search fires AND resolves (fast)

    // Now let the stale first request resolve.
    resolveFirst({
      status: 'ok',
      data: [{ insert: 'first.jpeg', label: 'first', detail: '.jpeg', kind: 'asset' }],
    });
    await flush();

    const items = document.body.querySelectorAll('.pop-file-picker-item');
    expect(items.length).toBe(1);
    expect(items[0].textContent).toContain('second');
    bar.destroy();
  });
});
