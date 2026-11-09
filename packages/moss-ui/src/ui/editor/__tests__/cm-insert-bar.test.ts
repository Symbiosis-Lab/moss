import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import { EditorView } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import {
  insertBarContext,
  INSERT_BAR_ESSENTIALS,
  INSERT_BAR_MORE,
  insertAtCaret,
  buildGalleryInsert,
  insertHeroAtCaret,
  mediaEmbeds,
  InsertBar,
  insertBarExtension,
  type InsertBarHooks,
} from '../cm-insert-bar';
import { slashMenuField, setSlashMenu } from '../cm-slash-menu';
import { SLASH_CATALOG } from '../cm-shortcode-catalog';
import { markdown } from '@codemirror/lang-markdown';
import { TaskList } from '@lezer/markdown';
import { wikilinkConfig } from '@symbiosis-lab/moss-syntax';

function makeState(doc: string, anchor: number, head?: number) {
  return EditorState.create({
    doc,
    selection: { anchor, head: head ?? anchor },
    extensions: [slashMenuField],
  });
}

// ── insertBarContext — presence = focus, contents = caret context ─────────

describe('insertBarContext', () => {
  test("'insert' on an empty doc with the caret at 0 (new-file case)", () => {
    expect(insertBarContext(makeState('', 0), true)).toBe('insert');
  });

  test("'insert' on an empty line between paragraphs", () => {
    expect(insertBarContext(makeState('para\n\npara', 5), true)).toBe('insert');
  });

  test("'insert' on a whitespace-only line", () => {
    expect(insertBarContext(makeState('  ', 1), true)).toBe('insert');
  });

  test("'format' when the caret line has text", () => {
    expect(insertBarContext(makeState('hello', 3), true)).toBe('format');
  });

  test("'format' for a range selection, even across empty lines", () => {
    expect(insertBarContext(makeState('para\n\n', 0, 6), true)).toBe('format');
  });

  test('null when the editor is not focused', () => {
    expect(insertBarContext(makeState('', 0), false)).toBeNull();
    expect(insertBarContext(makeState('hello', 3), false)).toBeNull();
  });

  test('null while the slash menu is open', () => {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = new EditorView({ state: makeState('', 0), parent: container });
    view.dispatch({
      effects: setSlashMenu.of({ active: true, query: '', triggerFrom: 0, selectedIndex: 0 }),
    });
    expect(insertBarContext(view.state, true)).toBeNull();
    view.destroy();
    container.remove();
  });

  test('null when the state is read-only', () => {
    const state = EditorState.create({
      doc: '',
      selection: { anchor: 0 },
      extensions: [EditorState.readOnly.of(true)],
    });
    expect(insertBarContext(state, true)).toBeNull();
  });
});

// ── insertBarContext — the page never depends on what is under the caret ──
// A caret on a link, a
// task, a picture, a footnote or a comment must not select a fourth, "reference"
// page (Open / Edit… / Unlink / Toggle). The bar now shows insert or format
// from the line alone.

function stateWithDoc(doc: string) {
  return EditorState.create({ doc, extensions: [markdown({ extensions: [TaskList, wikilinkConfig] }), slashMenuField] });
}

describe('insertBarContext — only the line decides', () => {
  test.each([
    ['a link', '[text](url)', 2],
    ['a link boundary', '[text](url)', 0],
    ['a task', '- [ ] todo', 4],
    ['a picture', '![[old.png]]', 4],
    ['a footnote reference', 'a[^1]\n\n[^1]: note', 3],
    ['a comment', '{==text==}{>>note<<}', 4],
  ])('a caret on %s gets the format page like any text', (_n, doc, at) => {
    const state = stateWithDoc(doc);
    expect(insertBarContext(state.update({ selection: { anchor: at } }).state, true)).toBe('format');
  });

  test('an empty line is the insert page and a selection is the format page', () => {
    expect(insertBarContext(stateWithDoc(''), true)).toBe('insert');
    expect(insertBarContext(stateWithDoc('[text](url)').update({ selection: { anchor: 0, head: 4 } }).state, true)).toBe('format');
  });

  test('the bar has no reference page', () => {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = new EditorView({ state: stateWithDoc('[text](url)').update({ selection: { anchor: 2 } }).state, parent: container });
    const bar = new InsertBar(view, { pickMedia: async () => null });
    bar.sync(true);
    expect(bar.activePage).toBe('format');
    expect(bar.dom.querySelector('.insert-bar-page--reference, [data-ref-action], .insert-bar-chevron-format')).toBeNull();
    bar.destroy();
    view.destroy();
    container.remove();
  });
});

// ── Catalog split — one catalog, two faces ────────────────────────────────

describe('insert bar catalog split', () => {
  test('essentials are h2, media, ul, bq (media is picker-driven, not a slash item)', () => {
    expect(INSERT_BAR_ESSENTIALS).toEqual(['h2', 'media', 'ul', 'bq']);
    for (const name of INSERT_BAR_ESSENTIALS) {
      if (name === 'media') continue;
      expect(SLASH_CATALOG.find((s) => s.name === name)).toBeTruthy();
    }
  });

  test('more page holds the eleven curated chips, all resolvable in SLASH_CATALOG', () => {
    expect(INSERT_BAR_MORE).toEqual([
      'ol', 'callout', 'hr', 'gallery', 'hero', 'grid', 'buttons',
      'subscribe', 'recent', 'code', 'math',
    ]);
    for (const name of INSERT_BAR_MORE) {
      expect(SLASH_CATALOG.find((s) => s.name === name)).toBeTruthy();
    }
  });

  test('bar + deliberate exclusions cover the whole slash catalog', () => {
    // img → replaced by Media; imath → mid-line construct; h1/h3/task → raw markdown / slash;
    // form → slash only.
    const covered = new Set([
      ...INSERT_BAR_ESSENTIALS, ...INSERT_BAR_MORE,
      'img', 'imath', 'h1', 'h3', 'task', 'form',
    ]);
    for (const spec of SLASH_CATALOG) {
      expect(covered.has(spec.name)).toBe(true);
    }
  });
});

// ── Insert helpers ────────────────────────────────────────────────────────

function makeView(doc: string, anchor: number, head?: number) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const view = new EditorView({ state: makeState(doc, anchor, head), parent: container });
  return {
    view,
    cleanup: () => {
      view.destroy();
      container.remove();
    },
  };
}

describe('insertAtCaret', () => {
  test('h2 on an empty line inserts "## " with the caret after the prefix', () => {
    const { view, cleanup } = makeView('', 0);
    insertAtCaret(view, SLASH_CATALOG.find((s) => s.name === 'h2')!);
    expect(view.state.doc.toString()).toBe('## ');
    expect(view.state.selection.main.anchor).toBe(3);
    cleanup();
  });

  test('insert on an empty line between paragraphs lands at the caret', () => {
    const { view, cleanup } = makeView('above\n\nbelow', 6);
    insertAtCaret(view, SLASH_CATALOG.find((s) => s.name === 'ul')!);
    expect(view.state.doc.toString()).toBe('above\n- \nbelow');
    expect(view.state.selection.main.anchor).toBe(8);
    cleanup();
  });

  test('subscribe expands the fenced template with the first field selected', () => {
    const { view, cleanup } = makeView('', 0);
    insertAtCaret(view, SLASH_CATALOG.find((s) => s.name === 'subscribe')!);
    const doc = view.state.doc.toString();
    expect(doc.startsWith(':::subscribe')).toBe(true);
    const sel = view.state.selection.main;
    expect(view.state.doc.sliceString(sel.from, sel.to)).toBe('Subscribe');
    cleanup();
  });
});

describe('picker-backed builders', () => {
  test('buildGalleryInsert puts one embed line per picked file', () => {
    expect(buildGalleryInsert(['![](a.jpg)', '![](b%202.png)'])).toBe(
      ':::gallery {per-line=3}\n![](a.jpg)\n![](b%202.png)\n:::',
    );
  });

  test('mediaEmbeds writes the standard form from the engine destination', async () => {
    const dest = async (t: string) => `../${t}`;
    expect(await mediaEmbeds(['assets/a.jpg', 'assets/b 2.png'], async (t) => `../${t.replace(' ', '%20')}`))
      .toEqual(['![](../assets/a.jpg)', '![](../assets/b%202.png)']);
    expect(await mediaEmbeds(['assets/a.jpg'], dest)).toEqual(['![](../assets/a.jpg)']);
  });

  test('mediaEmbeds falls back to a wikilink when the backend gives no destination', async () => {
    expect(await mediaEmbeds(['assets/a.jpg'], async () => null)).toEqual(['![[a.jpg]]']);
  });

  test('insertHeroAtCaret quotes image= and selects the Title placeholder', () => {
    const { view, cleanup } = makeView('', 0);
    insertHeroAtCaret(view, ['cover.jpg'], ['![](cover.jpg)']);
    const doc = view.state.doc.toString();
    // ASCII bareword — `renderShortcodeAttrValue` quotes only what the attr
    // grammar cannot read bare.
    expect(doc.startsWith(':::hero {image=cover.jpg}')).toBe(true);
    const sel = view.state.selection.main;
    expect(view.state.doc.sliceString(sel.from, sel.to)).toBe('Title');
    cleanup();
  });

  test('insertHeroAtCaret survives spaces and CJK in the picked filename', () => {
    // The attr grammar's bareword is ASCII-only — unquoted values break on
    // real-world names (macOS screenshots, CJK photos). Quoting is required,
    // and a literal `"` is ESCAPED, not stripped: `read_quoted` in attrs.rs
    // takes any char after a backslash, so the filename round-trips intact.
    const { view, cleanup } = makeView('', 0);
    insertHeroAtCaret(view, ['Screenshot 2026-07-27 at 09.15.30（山中"来信”）.png'], []);
    const doc = view.state.doc.toString();
    expect(doc.startsWith(':::hero {image="Screenshot 2026-07-27 at 09.15.30（山中\\"来信”）.png"}')).toBe(true);
    cleanup();
  });

  test('insertHeroAtCaret with several picks emits body-line slides', () => {
    // Multi-image hero grammar: consecutive leading media lines are the
    // crossfading slides; videos join as ambient loops.
    const { view, cleanup } = makeView('', 0);
    insertHeroAtCaret(view, ['a.jpg', 'clip 1.mp4', 'b.png'], ['![](a.jpg)', '![](clip%201.mp4)', '![](b.png)']);
    const doc = view.state.doc.toString();
    expect(doc.startsWith(':::hero\n![](a.jpg)\n![](clip%201.mp4)\n![](b.png)\n# Title')).toBe(true);
    const sel = view.state.selection.main;
    expect(view.state.doc.sliceString(sel.from, sel.to)).toBe('Title');
    cleanup();
  });
});

// ── InsertBar — DOM controller ────────────────────────────────────────────

describe('InsertBar controller', () => {
  let view: EditorView;
  let container: HTMLDivElement;
  let bar: InsertBar;
  let hooks: InsertBarHooks;
  let picked: string[] | null;
  let commented: number;
  let afterInserts: number;

  beforeEach(() => {
    vi.useFakeTimers();
    picked = ['posts/photo.jpg'];
    commented = 0;
    afterInserts = 0;
    hooks = {
      pickMedia: vi.fn(async () => picked),
      // The engine's answer (asset_ref_for_path) for a page in `posts/`.
      destinationFor: vi.fn(async (t: string) => encodeURI(t.startsWith('posts/') ? t.slice(6) : `../${t}`)),
      onComment: () => { commented++; },
      afterMediaInsert: () => { afterInserts++; },
    };
    container = document.createElement('div');
    document.body.appendChild(container);
    view = new EditorView({ state: makeState('', 0), parent: container });
    bar = new InsertBar(view, hooks);
  });

  afterEach(() => {
    bar.destroy();
    view.destroy();
    container.remove();
    vi.useRealTimers();
  });

  test('mounts hidden, falling back to document.body when there is no CM pane', () => {
    expect(document.body.contains(bar.dom)).toBe(true);
    expect(bar.dom.classList.contains('visible')).toBe(false);
    expect(bar.dom.getAttribute('aria-hidden')).toBe('true');
  });

  // The bar is anchored INSIDE the CM pane so it rides above .create-status
  // without measuring it. The fallback above is what jsdom exercises, so the
  // real target needs its own test or the anchoring silently regresses to
  // body-mounted (viewport-anchored) and overlaps the create status line again.
  test('mounts into the pane\'s band, which is what divides the row', () => {
    const pane = document.createElement('div');
    pane.id = 'moss-editor-cm';
    document.body.appendChild(pane);
    const paneView = new EditorView({ state: makeState('', 0), parent: pane });
    const paneBar = new InsertBar(paneView, hooks);
    try {
      expect(pane.contains(paneBar.dom)).toBe(true);
      // The BAND, not the pane: the bar's width fit reads its column gap and
      // the cluster's floor off that element, so a bar parked one level up
      // would measure a grid it is not in.
      expect(paneBar.dom.parentElement).toBe(pane.querySelector('.editor-band'));
    } finally {
      paneBar.destroy();
      paneView.destroy();
      pane.remove();
    }
  });

  // The catalog page's fixed lead is wrapped so fitWidth can measure it.
  // Summing its children instead undercounts by 8px per .insert-bar-sep
  // (1px wide, 4px margins each side) — the miscount that let the row's end
  // fall outside the pill's overflow:hidden and render a half-drawn divider
  // with a barely-clickable control beyond it.
  test('the catalog page wraps its fixed lead for measurement', () => {
    // All three pages are always in the DOM (they cross-fade), so this needs no
    // page switch — which keeps the test off InsertBar's private API.
    const page = bar.dom.querySelector('.insert-bar-page--catalog') as HTMLElement;
    const lead = page.querySelector('.insert-bar-lead');
    expect(lead).not.toBeNull();
    expect(lead!.querySelector('.insert-bar-back')).not.toBeNull();
    // The chip list stays a direct child after the lead — it is the only part
    // that may be squeezed, and it now runs to the end of the row.
    const kids = Array.from(page.children).map((el) => el.className);
    expect(kids).toEqual(['insert-bar-lead', 'insert-bar-chips']);
  });

  test('appears with the insert page when focused on an empty line — no delay', () => {
    bar.sync(true);
    expect(bar.dom.classList.contains('visible')).toBe(true);
    expect(bar.dom.getAttribute('aria-hidden')).toBe('false');
    expect(bar.activePage).toBe('insert');
  });

  test('switches to the format page when the caret line has text', () => {
    view.dispatch({ changes: { from: 0, insert: 'hello' } });
    bar.sync(true);
    expect(bar.dom.classList.contains('visible')).toBe(true);
    expect(bar.activePage).toBe('format');
  });

  test('keeps its page across an edit made elsewhere', () => {
    view.dispatch({ changes: { from: 0, insert: 'hello' } });
    bar.sync(true);
    expect(bar.activePage).toBe('format');
    // The caret stays on the "hello" line (anchor 0) while a newline lands
    // after it — the page must recompute from the content and still be
    // 'format', with no dim-driven staleness to paper over a wrong answer.
    view.dispatch({ changes: { from: 5, insert: '\n' }, selection: { anchor: 0 } });
    bar.sync(true);
    expect(bar.activePage).toBe('format');
  });

  test('hiding keeps the current page until the fade-out completes, then resets', () => {
    // An immediate reset plays visibly under the 160ms exit fade — the page
    // must stay put while fading and reset silently afterwards.
    view.dispatch({ changes: { from: 0, insert: 'hello' } });
    bar.sync(true);
    expect(bar.activePage).toBe('format');
    bar.sync(false);
    expect(bar.dom.classList.contains('visible')).toBe(false);
    expect(bar.activePage).toBe('format');
    vi.advanceTimersByTime(250);
    expect(bar.activePage).toBe('insert');
  });

  test('blur then quick refocus in text never passes through the insert page', () => {
    view.dispatch({ changes: { from: 0, insert: 'hello' } });
    bar.sync(true);
    bar.sync(false);
    expect(bar.activePage).toBe('format');
    bar.sync(true);
    expect(bar.dom.classList.contains('visible')).toBe(true);
    expect(bar.activePage).toBe('format');
    // The deferred reset must have been cancelled by the re-show.
    vi.advanceTimersByTime(400);
    expect(bar.activePage).toBe('format');
  });

  test('hides on blur and resets an open catalog after the fade', () => {
    bar.sync(true);
    bar.openMore();
    expect(bar.activePage).toBe('catalog');
    bar.sync(false);
    expect(bar.dom.classList.contains('visible')).toBe(false);
    vi.advanceTimersByTime(250);
    expect(bar.activePage).toBe('insert');
  });

  test('re-showing into insert context does not resurrect a catalog from before the hide', () => {
    bar.sync(true);
    bar.openMore();
    bar.sync(false);
    bar.sync(true); // refocus on the empty line before the reset timer fires
    expect(bar.activePage).toBe('insert');
  });

  test('hides while the slash menu is open', () => {
    bar.sync(true);
    view.dispatch({
      effects: setSlashMenu.of({ active: true, query: '', triggerFrom: 0, selectedIndex: 0 }),
    });
    bar.sync(true);
    expect(bar.dom.classList.contains('visible')).toBe(false);
  });

  test('catalog page survives insert-context syncs (stays open while deciding)', () => {
    bar.sync(true);
    bar.openMore();
    bar.sync(true);
    expect(bar.activePage).toBe('catalog');
  });

  // The catalog row is for buttons. The `/` fast path is taught on the ⋯
  // button's tooltip instead — at the moment of intent, for no row space.
  test('the ⋯ button teaches the / fast path, and the row carries no prose', () => {
    const more = bar.dom.querySelector('.insert-bar-more') as HTMLElement;
    expect(more.getAttribute('data-tooltip')).toBeTruthy();
    expect(more.getAttribute('data-tooltip-kbd')).toBe('/');
    const catalog = bar.dom.querySelector('.insert-bar-page--catalog') as HTMLElement;
    expect(catalog.querySelector('span:not(.insert-bar-chip span)')).toBeNull();
  });

  test('insert page renders essentials + ⋯; catalog renders the labeled chips', () => {
    const btns = bar.dom.querySelectorAll('.insert-bar-page--insert button[data-cmd]');
    expect([...btns].map((b) => b.getAttribute('data-cmd'))).toEqual([...INSERT_BAR_ESSENTIALS]);
    for (const b of btns) {
      expect(b.getAttribute('aria-label')).toBeTruthy();
      expect(b.getAttribute('data-tooltip')).toBeTruthy();
    }
    const chips = bar.dom.querySelectorAll('.insert-bar-chip[data-cmd]');
    expect([...chips].map((c) => c.getAttribute('data-cmd'))).toEqual([...INSERT_BAR_MORE]);
    for (const c of chips) {
      expect(c.querySelector('svg')).toBeTruthy();
      expect(c.textContent!.trim().length).toBeGreaterThan(0);
    }
  });

  test('format page renders heading, quote, + the six ftbar actions', () => {
    const btns = bar.dom.querySelectorAll('.insert-bar-page--format button[data-fmt]');
    expect([...btns].map((b) => b.getAttribute('data-fmt'))).toEqual([
      'heading', 'quote', 'bold', 'italic', 'strike', 'code', 'link', 'comment',
    ]);
  });

  test('every format button carries the tooltip shortcut chip of its keymap twin', () => {
    // Chips must mirror the live bindings (cm-format-keys.ts / cm-comment.ts);
    // a wrong chip is worse than none.
    const chips = Object.fromEntries(
      [...bar.dom.querySelectorAll('.insert-bar-page--format button[data-fmt]')]
        .map((b) => [b.getAttribute('data-fmt'), b.getAttribute('data-tooltip-kbd')]),
    );
    expect(chips).toEqual({
      heading: '⌘⌥2', quote: '⌘⇧9', bold: '⌘B', italic: '⌘I',
      strike: '⌘⇧S', code: '⌘~', link: '⌘K', comment: '⌘⇧M',
    });
  });

  test('Escape returns from the catalog page and reports handled', () => {
    bar.openMore();
    expect(bar.handleEscape()).toBe(true);
    expect(bar.activePage).toBe('insert');
    expect(bar.handleEscape()).toBe(false);
  });

  test('clicking H inserts "## " at the caret', () => {
    bar.sync(true);
    (bar.dom.querySelector('button[data-cmd="h2"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(view.state.doc.toString()).toBe('## ');
  });

  test('media click routes through pickMedia and inserts the standard form', async () => {
    bar.sync(true);
    (bar.dom.querySelector('button[data-cmd="media"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await bar.pendingInsert;
    expect(hooks.pickMedia).toHaveBeenCalledWith('media');
    expect(view.state.doc.toString()).toBe('![](photo.jpg)');
    expect(afterInserts).toBe(1);
  });

  test('a Media pick in another folder takes the destination the engine gives for it', async () => {
    picked = ['assets/clips/intro.mp4'];
    bar.sync(true);
    (bar.dom.querySelector('button[data-cmd="media"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await bar.pendingInsert;
    expect(hooks.destinationFor).toHaveBeenCalledWith('assets/clips/intro.mp4');
    expect(view.state.doc.toString()).toBe('![](../assets/clips/intro.mp4)');
  });

  test('cancelled picker inserts nothing', async () => {
    picked = null;
    bar.sync(true);
    (bar.dom.querySelector('button[data-cmd="media"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await bar.pendingInsert;
    expect(view.state.doc.toString()).toBe('');
    expect(afterInserts).toBe(0);
  });

  test('gallery chip builds the block from a multi-pick', async () => {
    picked = ['posts/a.jpg', 'assets/b.png'];
    bar.sync(true);
    bar.openMore();
    (bar.dom.querySelector('.insert-bar-chip[data-cmd="gallery"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await bar.pendingInsert;
    expect(hooks.pickMedia).toHaveBeenCalledWith('gallery');
    expect(view.state.doc.toString()).toBe(':::gallery {per-line=3}\n![](a.jpg)\n![](../assets/b.png)\n:::');
  });

  test('gallery chip writes the standard form for a parenthesised file name', async () => {
    picked = ['posts/photo (1).jpg', 'assets/b.png'];
    bar.sync(true);
    bar.openMore();
    (bar.dom.querySelector('.insert-bar-chip[data-cmd="gallery"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await bar.pendingInsert;
    expect(view.state.doc.toString()).toBe(':::gallery {per-line=3}\n![](photo%20(1).jpg)\n![](../assets/b.png)\n:::');
  });

  test('hero chip with several picks writes standard-form slides', async () => {
    picked = ['posts/a.jpg', 'assets/b.png'];
    bar.sync(true);
    bar.openMore();
    (bar.dom.querySelector('.insert-bar-chip[data-cmd="hero"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await bar.pendingInsert;
    expect(view.state.doc.toString().startsWith(':::hero\n![](a.jpg)\n![](../assets/b.png)\n# Title')).toBe(true);
  });

  test('hero chip fills image= from a single pick with Title selected', async () => {
    picked = ['assets/cover.jpg'];
    bar.sync(true);
    bar.openMore();
    (bar.dom.querySelector('.insert-bar-chip[data-cmd="hero"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await bar.pendingInsert;
    expect(hooks.pickMedia).toHaveBeenCalledWith('hero');
    expect(view.state.doc.toString().startsWith(':::hero {image=cover.jpg}')).toBe(true);
    const sel = view.state.selection.main;
    expect(view.state.doc.sliceString(sel.from, sel.to)).toBe('Title');
  });

  test('a plain chip (divider) inserts through the slash machinery', () => {
    bar.sync(true);
    bar.openMore();
    (bar.dom.querySelector('.insert-bar-chip[data-cmd="hr"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(view.state.doc.toString()).toBe('---');
  });

  test('heading button on a text line turns it into an H2 via toggleHeading', () => {
    view.dispatch({ changes: { from: 0, insert: 'hello' }, selection: { anchor: 3 } });
    bar.sync(true);
    (bar.dom.querySelector('button[data-fmt="heading"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(view.state.doc.toString()).toBe('## hello');
  });

  test('quote button on a text line prefixes it with "> " via toggleBlockquote', () => {
    view.dispatch({ changes: { from: 0, insert: 'hello' }, selection: { anchor: 3 } });
    bar.sync(true);
    (bar.dom.querySelector('button[data-fmt="quote"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(view.state.doc.toString()).toBe('> hello');
  });

  test('bold button on a range wraps it via the formatMark engine', () => {
    view.dispatch({ changes: { from: 0, insert: 'hello' }, selection: { anchor: 0, head: 5 } });
    bar.sync(true);
    (bar.dom.querySelector('button[data-fmt="bold"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(view.state.doc.toString()).toBe('**hello**');
  });

  test('bold button at a collapsed caret mid-word toggles the word (⌘B parity)', () => {
    view.dispatch({ changes: { from: 0, insert: 'hello' }, selection: { anchor: 3 } });
    bar.sync(true);
    (bar.dom.querySelector('button[data-fmt="bold"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(view.state.doc.toString()).toBe('**hello**');
  });

  test('comment button routes to the onComment hook', () => {
    view.dispatch({ changes: { from: 0, insert: 'hello' }, selection: { anchor: 0, head: 5 } });
    bar.sync(true);
    (bar.dom.querySelector('button[data-fmt="comment"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(commented).toBe(1);
  });

  test('mousedown anywhere on the bar is prevented (editor keeps focus + caret)', () => {
    const ev = new MouseEvent('mousedown', { bubbles: true, cancelable: true });
    bar.dom.querySelector('button[data-cmd="h2"]')!.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
  });

  test('destroy removes the DOM node', () => {
    const dom = bar.dom;
    bar.destroy();
    expect(document.body.contains(dom)).toBe(false);
  });
});

// ── insertBarExtension — lifecycle through the real plugin path ───────────

describe('insertBarExtension lifecycle', () => {
  function mount(hooks: InsertBarHooks) {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = new EditorView({
      state: EditorState.create({
        doc: '',
        selection: { anchor: 0 },
        extensions: [slashMenuField, insertBarExtension(hooks)],
      }),
      parent: container,
    });
    return { view, container };
  }
  const stubHooks: InsertBarHooks = { pickMedia: async () => null };

  test('view.destroy() removes the bar from document.body', () => {
    const { view, container } = mount(stubHooks);
    expect(document.querySelectorAll('.insert-bar').length).toBe(1);
    view.destroy();
    container.remove();
    expect(document.querySelectorAll('.insert-bar').length).toBe(0);
  });

  test('an editor remount never stacks bars', () => {
    const a = mount(stubHooks);
    a.view.destroy();
    a.container.remove();
    const b = mount(stubHooks);
    expect(document.querySelectorAll('.insert-bar').length).toBe(1);
    b.view.destroy();
    b.container.remove();
  });
});
