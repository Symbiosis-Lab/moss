import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { EditorState, type Extension } from '@codemirror/state';
import { EditorView, showTooltip } from '@codemirror/view';
import { undo, history } from '@codemirror/commands';
import { markdown } from '@codemirror/lang-markdown';
import { GFM } from '@lezer/markdown';
import { wikilinkConfig, footnoteConfig } from '@symbiosis-lab/moss-syntax';
import {
  referenceCardExtension, closeCard, cardProviders, cardSpecAt, cardField, openCardAt, assetCardProvider,
  footnoteCardProvider, linkCardProvider, pageLinkCardProvider,
  currentArticleCardProvider, isCurrentArticleMapAt, withLinkControls, unlinkAt, type CardProvider, type CardSpec,
} from '../cm-reference-card';
import { chordTrackingExtension } from '../cm-link-nav';
import { createApplyEmbedWidth } from '../embed-width';
import { isMacPlatform } from "../../utils/kbd-label";
import { t } from "../../i18n/index";

function fakeProvider(spec: { title: string } | null) {
  return () => (spec ? { title: spec.title, body: document.createElement('div') } : null);
}

describe('card StateField', () => {
  test('starts closed', () => {
    const view = new EditorView({ state: EditorState.create({ extensions: [referenceCardExtension()] }) });
    expect(view.state.facet(showTooltip).filter(Boolean)).toHaveLength(0);
    view.destroy();
  });
  test('closeCard clears an open card', () => {
    const view = new EditorView({
      state: EditorState.create({
        extensions: [referenceCardExtension(), cardProviders.of(fakeProvider({ title: 'x' }))],
      }),
    });
    // openCardAt (written later) is the only way in today; for this step
    // this is a no-op close on an already-closed field.
    closeCard(view);
    expect(view.state.facet(showTooltip).filter(Boolean)).toHaveLength(0);
    view.destroy();
  });
});

describe('provider routing', () => {
  test('first matching provider wins; a null provider is skipped', () => {
    const a: CardProvider = () => null;
    const b: CardProvider = (_s, pos) => (pos === 5 ? { title: 'b', body: document.createElement('div') } : null);
    const view = new EditorView({
      state: EditorState.create({ doc: 'x'.repeat(10), extensions: [referenceCardExtension(), cardProviders.of(a), cardProviders.of(b)] }),
    });
    expect(cardSpecAt(view.state, 5)?.title).toBe('b');
    expect(cardSpecAt(view.state, 0)).toBeNull();
    view.destroy();
  });
});

// The doc for every dwell test below. The link `[a](https://x)` spans [0, 14):
// only its URL is its destination (URL_AT); the brackets and text are inert
// (BRACKET, TEXT). `[[Page]]` spans [19, 27) and its page name is active (WIKI).
const DWELL_DOC = '[a](https://x) and [[Page]] here';
const TEXT = 1;
const BRACKET = 0;
const URL_AT = 8;
const WIKI = 22;
const FAR = 31;

/** One fake provider covering both title cases the dwell tests assert on: a
 *  position over the link ("link") and over the wikilink ("wikilink"). */
const dwellFakeProvider: CardProvider = (_state, pos) => {
  if (pos >= 0 && pos <= 14) return { title: 'link', body: bodyOf('about the link') };
  if (pos >= 19 && pos <= 27) return { title: 'wikilink', body: bodyOf('about the page') };
  return null;
};
function bodyOf(text: string): HTMLElement {
  const body = document.createElement('div');
  body.textContent = text;
  return body;
}

describe('dwell arming', () => {
  let container: HTMLElement;
  let view: EditorView;

  beforeEach(() => {
    vi.useFakeTimers();
    container = document.createElement('div');
    document.body.appendChild(container);
  });
  afterEach(() => {
    vi.useRealTimers();
    view?.destroy();
    document.body.innerHTML = '';
  });

  function mountView(): EditorView {
    view = new EditorView({
      state: EditorState.create({
        doc: DWELL_DOC,
        extensions: [
          markdown({ extensions: [wikilinkConfig] }),
          referenceCardExtension(),
          chordTrackingExtension(),
          cardProviders.of(dwellFakeProvider),
        ],
      }),
      parent: container,
    });
    return view;
  }

  function fireMouseMove(v: EditorView, pos: number): void {
    v.posAtCoords = () => pos;
    v.dom.dispatchEvent(new MouseEvent('mousemove', { bubbles: true }));
  }

  const CHORD_KEY = isMacPlatform() ? 'Meta' : 'Control';
  function chordDown(v: EditorView): void {
    v.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: CHORD_KEY, bubbles: true }));
  }

  test('plain hover over a link\'s URL opens after 150ms', () => {
    const v = mountView();
    fireMouseMove(v, URL_AT);
    expect(v.state.field(cardField)).toBeNull();
    vi.advanceTimersByTime(150);
    expect(v.state.field(cardField)?.spec.title).toBe('link');
  });

  test.each([['its opening bracket', BRACKET], ['its text', TEXT]])('hover over a link\'s %s opens nothing: the destination is the link, not its words', (_where, pos) => {
    const v = mountView();
    fireMouseMove(v, pos);
    vi.advanceTimersByTime(500);
    expect(v.state.field(cardField)).toBeNull();
  });

  test('plain hover on a wikilink\'s page name opens after dwell, no chord needed', () => {
    const v = mountView();
    fireMouseMove(v, WIKI);
    vi.advanceTimersByTime(150);
    expect(v.state.field(cardField)?.spec.title).toBe('wikilink');
  });

  test('plain hover on a wikilink\'s brackets opens nothing', () => {
    const v = mountView();
    fireMouseMove(v, DWELL_DOC.indexOf('[[') + 1);
    vi.advanceTimersByTime(500);
    expect(v.state.field(cardField)).toBeNull();
  });

  test('holding the chord widens nothing: the text still opens no card', () => {
    const v = mountView();
    chordDown(v);
    fireMouseMove(v, TEXT);
    vi.advanceTimersByTime(500);
    expect(v.state.field(cardField)).toBeNull();
  });
});

describe('hover on what no longer hides its source', () => {
  let view: EditorView;
  afterEach(() => { vi.useRealTimers(); view?.destroy(); document.body.innerHTML = ''; });

  function mount(doc: string, provider: CardProvider): EditorView {
    vi.useFakeTimers();
    view = new EditorView({
      state: EditorState.create({
        doc,
        extensions: [markdown({ extensions: [wikilinkConfig, footnoteConfig] }), referenceCardExtension(), chordTrackingExtension(), cardProviders.of(provider)],
      }),
      parent: document.body,
    });
    return view;
  }

  test('plain hover on a footnote reference\'s characters opens its card, found by position', () => {
    const doc = 'A note.[^1]\n\n[^1]: The note.';
    const v = mount(doc, footnoteCardProvider);
    v.posAtCoords = () => doc.indexOf('[^1]') + 2;
    v.dom.dispatchEvent(new MouseEvent('mousemove', { bubbles: true }));
    vi.advanceTimersByTime(150);
    expect(v.state.field(cardField)?.spec.title).toBe('[^1]');
    expect(v.state.field(cardField)?.spec.body.textContent).toBe('The note.');
  });

  test('plain hover on the rendered block below an embed line opens the asset card for that embed', () => {
    const doc = '![Alt](images/logo.png)\n\ntext';
    const cache = { get: () => ({ kind: { kind: 'image' } }) } as any;
    const v = mount(doc, assetCardProvider(cache));
    const row = document.createElement('div');
    row.className = 'cm-lp-image-row';
    v.dom.appendChild(row); // outside contentDOM: jsdom cannot measure a foreign block there
    v.posAtDOM = () => doc.indexOf('\n'); // a block widget's position is its source line's end
    row.dispatchEvent(new MouseEvent('mousemove', { bubbles: true }));
    vi.advanceTimersByTime(150);
    expect(v.state.field(cardField)?.spec.title).toBe('images/logo.png');
  });

  test('hover on the same embed\'s source opens its card over the path only, not the alt text', () => {
    const doc = '![Alt](images/logo.png)\n\ntext';
    const cache = { get: () => ({ kind: { kind: 'image' } }) } as any;
    const v = mount(doc, assetCardProvider(cache));
    v.posAtCoords = () => doc.indexOf('Alt') + 1;
    v.dom.dispatchEvent(new MouseEvent('mousemove', { bubbles: true }));
    vi.advanceTimersByTime(500);
    expect(v.state.field(cardField)).toBeNull();
    v.posAtCoords = () => doc.indexOf('logo');
    v.dom.dispatchEvent(new MouseEvent('mousemove', { bubbles: true }));
    vi.advanceTimersByTime(150);
    expect(v.state.field(cardField)?.spec.title).toBe('images/logo.png');
  });
});

describe('an inline picture shows its picture in the hover card', () => {
  let view: EditorView;
  afterEach(() => { vi.useRealTimers(); view?.destroy(); document.body.innerHTML = ''; });
  const IMAGE_ENV = {
    kind: { kind: 'image' },
    resolved: { absolute_path: '/vault/images/logo.png', request_url: 'images/logo.png', mime_type: 'image/png', size_bytes: 1024, width: 16, height: 9 },
  };
  const doc = 'Some prose ![[images/logo.png]] and more.';

  function hover(at: number, extensions: Extension[] = []): EditorView {
    vi.useFakeTimers();
    view = new EditorView({
      state: EditorState.create({
        doc,
        extensions: [
          markdown({ extensions: [wikilinkConfig] }), referenceCardExtension(), chordTrackingExtension(),
          cardProviders.of(assetCardProvider({ get: () => IMAGE_ENV } as any)), ...extensions,
        ],
      }),
      parent: document.body,
    });
    view.posAtCoords = () => at;
    view.dom.dispatchEvent(new MouseEvent('mousemove', { bubbles: true }));
    vi.advanceTimersByTime(150);
    return view;
  }

  test('plain hover over the path opens a card holding the picture', () => {
    const v = hover(doc.indexOf('logo') + 1);
    const card = v.state.field(cardField);
    expect(card?.spec.title).toBe('images/logo.png');
    expect(card?.spec.body.querySelector('img.asset-preview-thumb')).not.toBeNull();
  });

  test.each([['the opening brackets', doc.indexOf('![[') + 1], ['the closing brackets', doc.indexOf(']]') + 1]])('hover over %s opens nothing', (_where, at) => {
    expect(hover(at).state.field(cardField)).toBeNull();
  });

  test('plain hover on the prose around it opens nothing', () => {
    const v = hover(2);
    expect(v.state.field(cardField)).toBeNull();
  });
});

describe('persistence and dismissal', () => {
  let container: HTMLElement;
  let view: EditorView;

  beforeEach(() => {
    vi.useFakeTimers();
    container = document.createElement('div');
    document.body.appendChild(container);
  });
  afterEach(() => {
    vi.useRealTimers();
    view?.destroy();
    document.body.innerHTML = '';
  });

  function mountView(): EditorView {
    view = new EditorView({
      state: EditorState.create({
        doc: DWELL_DOC,
        extensions: [
          markdown({ extensions: [wikilinkConfig] }),
          referenceCardExtension(),
          chordTrackingExtension(),
          cardProviders.of(dwellFakeProvider),
        ],
      }),
      parent: container,
    });
    return view;
  }

  function fireMouseMove(v: EditorView, pos: number): void {
    v.posAtCoords = () => pos;
    v.dom.dispatchEvent(new MouseEvent('mousemove', { bubbles: true }));
  }

  test('leaving the link\'s URL closes the card, even onto the same link\'s text', () => {
    const v = mountView();
    fireMouseMove(v, URL_AT);
    vi.advanceTimersByTime(150);
    expect(v.state.field(cardField)).not.toBeNull();
    fireMouseMove(v, TEXT);
    expect(v.state.field(cardField)).toBeNull();
  });

  test('Escape closes an open card', () => {
    const v = mountView();
    fireMouseMove(v, URL_AT);
    vi.advanceTimersByTime(150);
    v.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(v.state.field(cardField)).toBeNull();
  });

  test('Escape with no card open is left unhandled for a host listening above the editor', () => {
    const v = mountView();
    const ev = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
    v.contentDOM.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(false);
  });

  test('scrolling the editor closes an open card', () => {
    const v = mountView();
    fireMouseMove(v, URL_AT);
    vi.advanceTimersByTime(150);
    v.scrollDOM.dispatchEvent(new Event('scroll'));
    expect(v.state.field(cardField)).toBeNull();
  });
});

describe('openCardAt', () => {
  test('a click-opened card renders its fields and actions', () => {
    const commit = vi.fn();
    const run = vi.fn();
    const provider: CardProvider = () => ({
      title: 'Link', body: document.createElement('div'),
      fields: [{ label: 'URL', value: 'https://x', commit }],
      actions: [{ label: 'Unlink', run }],
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = new EditorView({
      state: EditorState.create({ doc: 'x', extensions: [referenceCardExtension(), cardProviders.of(provider)] }),
      parent: container,
    });
    expect(openCardAt(view, 0)).toBe(true);
    const dom = view.dom.querySelector('.cm-reference-card')!;
    expect(dom.querySelector('input')!.getAttribute('value')).toBe('https://x');
    const btn = dom.querySelector('button') as HTMLButtonElement;
    btn.click();
    expect(run).toHaveBeenCalledWith(view);
    view.destroy();
    document.body.innerHTML = '';
  });
});

describe('click-opened card dismissal', () => {
  const provider: CardProvider = () => ({ title: 'Link', body: document.createElement('div') });
  function mountOpen(): EditorView {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = new EditorView({
      state: EditorState.create({ doc: 'x', extensions: [referenceCardExtension(), cardProviders.of(provider)] }),
      parent: container,
    });
    openCardAt(view, 0);
    return view;
  }

  test('a mousedown outside the card closes it', async () => {
    const view = mountOpen();
    await new Promise((r) => setTimeout(r, 0));
    expect(view.dom.querySelector('.cm-reference-card')).not.toBeNull();
    document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(view.dom.querySelector('.cm-reference-card')).toBeNull();
    view.destroy();
    document.body.innerHTML = '';
  });

  test('a mousedown inside the card leaves it open', async () => {
    const view = mountOpen();
    await new Promise((r) => setTimeout(r, 0));
    view.dom.querySelector('.cm-reference-card')!.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(view.dom.querySelector('.cm-reference-card')).not.toBeNull();
    view.destroy();
    document.body.innerHTML = '';
  });
});

describe('asset provider', () => {
  test('renders buildHoverDom for an image target', () => {
    const cache = {
      get: () => ({
        kind: { kind: 'image' },
        resolved: {
          absolute_path: '/a.png', request_url: '/a.png', mime_type: 'image/png',
          width: 10, height: 10, size_bytes: 1,
        } as never,
      }),
    };
    const doc = '![a](a.png)';
    const view = new EditorView({ state: EditorState.create({ doc, extensions: [markdown()] }) });
    const provider = assetCardProvider(cache);
    const spec = provider(view.state, doc.indexOf('a.png') + 1);
    expect(spec?.body.querySelector('.asset-preview')).toBeTruthy();
    view.destroy();
  });
});

describe('footnote provider', () => {
  test('a reference shows the note text', () => {
    const doc = 'See[^1] it.\n\n[^1]: The note body.';
    const view = new EditorView({
      state: EditorState.create({ doc, extensions: [markdown({ extensions: [wikilinkConfig, footnoteConfig] })] }),
    });
    const spec = footnoteCardProvider(view.state, doc.indexOf('[^1]') + 1);
    expect(spec?.body.textContent).toContain('The note body.');
    view.destroy();
  });

  test("a definition's marker shows the sentence citing it", () => {
    const doc = 'See[^1] it.\n\n[^1]: The note body.';
    const view = new EditorView({
      state: EditorState.create({ doc, extensions: [markdown({ extensions: [wikilinkConfig, footnoteConfig] })] }),
    });
    const spec = footnoteCardProvider(view.state, doc.indexOf('[^1]:') + 1);
    expect(spec?.body.textContent).toContain('See[^1] it.');
    view.destroy();
  });
});

/** A markdown+wikilink state, for the providers below that only read the
 *  link/embed target under `pos` — no view needed since they never touch
 *  the DOM beyond building the card body. */
function stateWithDoc(doc: string): EditorState {
  return EditorState.create({ doc, extensions: [markdown({ extensions: [wikilinkConfig] })] });
}

describe('the last-resort link provider', () => {
  test('any link no other provider claims gets a card titled with its address', () => {
    expect(linkCardProvider(stateWithDoc('[a](nowhere.md)'), 5)?.title).toBe('nowhere.md');
    expect(linkCardProvider(stateWithDoc('plain text'), 3)).toBeNull();
  });
});

describe('a hover card opens only for what the line does not already show', () => {
  let view: EditorView;
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); view?.destroy(); document.body.innerHTML = ''; });

  const EXTERNAL = { kind: { kind: 'external', data: { url: 'https://example.com/events' } } };
  const PAGE = { kind: { kind: 'link' }, anchor: null };
  const info = (description: string | null) => ({ status: 'ok', data: { title: 'About', description, source_path: 'about.md', generated: false, heading: null } });

  /** The link providers in the order and wrapping the editor registers them. */
  function mount(doc: string, env: unknown, call = vi.fn().mockResolvedValue(info(null))): EditorView {
    const cache = { get: () => env } as any;
    const withControls = (base: CardProvider): CardProvider => (state, pos) => {
      const spec = base(state, pos);
      return spec && withLinkControls(spec, state, pos, cache);
    };
    view = new EditorView({
      state: EditorState.create({
        doc,
        extensions: [
          markdown({ extensions: [wikilinkConfig, footnoteConfig] }), referenceCardExtension(),
          cardProviders.of(footnoteCardProvider),
          cardProviders.of(withControls(pageLinkCardProvider(cache, call, { fromFile: () => 'note.md' }))),
          cardProviders.of(withControls(linkCardProvider)),
        ],
      }),
      parent: document.body,
    });
    return view;
  }
  function pointAt(v: EditorView, pos: number): void {
    v.posAtCoords = () => pos;
    v.dom.dispatchEvent(new MouseEvent('mousemove', { bubbles: true }));
  }
  const card = (v: EditorView) => v.dom.querySelector('.cm-reference-card');

  test('an external link\'s URL opens no card on hover; ⌘K still opens its fields and actions', async () => {
    const doc = 'See [the events](https://example.com/events) soon.';
    const v = mount(doc, EXTERNAL);
    const at = doc.indexOf('example.com') + 3;
    pointAt(v, at);
    await vi.advanceTimersByTimeAsync(1000);
    expect(card(v)).toBeNull();

    expect(openCardAt(v, at)).toBe(true);
    expect((card(v)!.querySelector('input') as HTMLInputElement).getAttribute('value')).toBe('https://example.com/events');
    expect([...card(v)!.querySelectorAll('button')].map((b) => b.textContent)).toEqual([t('reference.open'), t('reference.unlink')]);
  });

  test('a page link opens once its description arrives, and shows only that', async () => {
    const doc = 'See [[about]] soon.';
    let arrive!: (value: unknown) => void;
    const v = mount(doc, PAGE, vi.fn(() => new Promise((resolve) => { arrive = resolve; })));
    pointAt(v, doc.indexOf('about') + 2);
    await vi.advanceTimersByTimeAsync(150);
    expect(card(v)).toBeNull();
    arrive(info('The about page.'));
    await vi.advanceTimersByTimeAsync(0);
    expect(card(v)!.textContent).toBe('The about page.');
  });

  test('a page with no description opens no card', async () => {
    const doc = 'See [[about]] soon.';
    const call = vi.fn().mockResolvedValue(info(null));
    const v = mount(doc, PAGE, call);
    pointAt(v, doc.indexOf('about') + 2);
    await vi.advanceTimersByTimeAsync(1000);
    expect(call).toHaveBeenCalledOnce();
    expect(card(v)).toBeNull();
  });

  test('a description that arrives after the pointer has left opens nothing', async () => {
    const doc = 'See [[about]] soon.';
    let arrive!: (value: unknown) => void;
    const v = mount(doc, PAGE, vi.fn(() => new Promise((resolve) => { arrive = resolve; })));
    pointAt(v, doc.indexOf('about') + 2);
    await vi.advanceTimersByTimeAsync(150);
    pointAt(v, 1);
    arrive(info('The about page.'));
    await vi.advanceTimersByTimeAsync(1000);
    expect(card(v)).toBeNull();
  });

  test('a lookup that fails opens nothing and stops waiting, so the next rest asks again', async () => {
    const doc = 'See [[about]] soon.';
    const call = vi.fn().mockRejectedValue(new Error('offline'));
    const v = mount(doc, PAGE, call);
    pointAt(v, doc.indexOf('about') + 1);
    await vi.advanceTimersByTimeAsync(150);
    expect(card(v)).toBeNull();
    pointAt(v, doc.indexOf('about') + 3);
    await vi.advanceTimersByTimeAsync(150);
    expect(call).toHaveBeenCalledTimes(2);
    expect(card(v)).toBeNull();
  });

  test('moving along the same page name while waiting asks once', async () => {
    const doc = 'See [[about]] soon.';
    let arrive!: (value: unknown) => void;
    const call = vi.fn(() => new Promise((resolve) => { arrive = resolve; }));
    const v = mount(doc, PAGE, call);
    pointAt(v, doc.indexOf('about') + 1);
    await vi.advanceTimersByTimeAsync(150);
    pointAt(v, doc.indexOf('about') + 3);
    await vi.advanceTimersByTimeAsync(150);
    expect(call).toHaveBeenCalledOnce();
    arrive(info('The about page.'));
    await vi.advanceTimersByTimeAsync(0);
    expect(card(v)).not.toBeNull();
  });

  test('a footnote\'s hover card is the note alone: no heading repeating the marker, no shortcut line', async () => {
    const doc = 'A note.[^1]\n\n[^1]: The note.';
    const v = mount(doc, undefined);
    pointAt(v, doc.indexOf('[^1]') + 2);
    await vi.advanceTimersByTimeAsync(150);
    expect(card(v)!.textContent).toBe('The note.');
  });
});

describe('a link inside a table cell', () => {
  // A table is styled source now, not a card of its own: the cell's link is
  // answered by the link's own provider, the same as outside a table.
  const doc = '| a | b |\n| - | - |\n| [x](https://example.com) | [[about]] |\n';
  const stateWith = (cache: unknown, provider: (c: any) => any) =>
    EditorState.create({
      doc,
      extensions: [markdown({ extensions: [GFM, wikilinkConfig] }), cardProviders.of(provider(cache))],
    });

  test('an external link gets the last-resort link card', () => {
    const spec = cardSpecAt(stateWith(null, () => linkCardProvider), doc.indexOf('[x]') + 1);
    expect(spec?.title).toBe('https://example.com');
  });

  test('a page link gets the page-link card', () => {
    const cache = { get: () => ({ kind: { kind: 'link' }, anchor: null }) };
    const call = vi.fn().mockResolvedValue({ status: 'ok', data: { title: 'About', description: 'The about page.', source_path: 'about.md', generated: false, heading: null } });
    const provider = (c: any) => pageLinkCardProvider(c, call, { fromFile: () => 'note.md' });
    const spec = cardSpecAt(stateWith(cache, provider), doc.indexOf('[[about]]') + 3);
    expect(spec?.title).toBe('about');
  });
});

describe('page-link provider', () => {
  const opts = { fromFile: () => 'note.md' };

  test('fills the card body once the backend call resolves', async () => {
    const call = vi.fn().mockResolvedValue({
      status: 'ok',
      data: { title: 'About', description: 'The about page.', source_path: 'about.md', generated: false, heading: null },
    });
    const cache = { get: () => ({ kind: { kind: 'link' }, anchor: null }) } as any;
    const provider = pageLinkCardProvider(cache, call, opts);
    const spec = provider(stateWithDoc('[[about]]'), 3);
    // Synchronous placeholder from the cache, before the RPC resolves.
    expect(spec?.title).toBe('about');
    await Promise.resolve();
    await Promise.resolve();
    expect(spec?.body.textContent).toContain('The about page.');
    expect(call).toHaveBeenCalledWith('note.md', 'about');
  });

  test('a generated page notes it in the body', async () => {
    const call = vi.fn().mockResolvedValue({
      status: 'ok',
      data: { title: 'Tag: x', description: null, source_path: null, generated: true, heading: null },
    });
    const cache = { get: () => ({ kind: { kind: 'link' }, anchor: null }) } as any;
    const spec = pageLinkCardProvider(cache, call, opts)(stateWithDoc('[[tags/x]]'), 3);
    await Promise.resolve();
    await Promise.resolve();
    expect(spec?.body.textContent).toContain(t('completion.generated'));
  });

  test('an #anchor link asks the heading question, never the description', async () => {
    const call = vi.fn().mockResolvedValue({
      status: 'ok',
      data: { title: 'About', description: 'ignored', source_path: 'about.md', generated: false, heading: 'Facilities' },
    });
    const cache = { get: () => ({ kind: { kind: 'anchor' }, anchor: 'facilities' }) } as any;
    const spec = pageLinkCardProvider(cache, call, opts)(stateWithDoc('[[about#facilities]]'), 3);
    await Promise.resolve();
    await Promise.resolve();
    expect(spec?.body.textContent).toBe('Facilities');
  });

  test('a non-link/moved/anchor kind is not this provider\'s to show', () => {
    const call = vi.fn();
    const cache = { get: () => ({ kind: { kind: 'external' }, anchor: null }) } as any;
    expect(pageLinkCardProvider(cache, call, opts)(stateWithDoc('[[x]]'), 2)).toBeNull();
    expect(call).not.toHaveBeenCalled();
  });
});

describe('editable link card — withLinkControls', () => {
  afterEach(() => { document.body.innerHTML = ''; });

  /** A document containing one `[text](url)` link, wired exactly the way
   *  cm-editor.ts wraps a link-kind provider with `withLinkControls` — a
   *  trivial base provider stands in for page/external/diagnostic here,
   *  since this exercises the SHARED wrapper rather than any one of the
   *  three. `extractLinkTargets` and `referenceSiteAt` run for REAL (markdown() parses
   *  the link), so the destination span `withLinkControls` commits to is
   *  genuine, not mocked. */
  function mountLinkView(doc: string, opts?: { extensions?: Extension[] }): EditorView {
    const base: CardProvider = (): CardSpec => ({ title: 'link', body: document.createElement('div') });
    const provider: CardProvider = (state, pos) => {
      const spec = base(state, pos);
      // No "Open" action is exercised below, so a cache that never resolves
      // anything is enough — real resolvedCache wiring is cm-editor.ts's job.
      return spec && withLinkControls(spec, state, pos, { get: () => undefined });
    };
    const container = document.createElement('div');
    document.body.appendChild(container);
    return new EditorView({
      state: EditorState.create({
        doc,
        extensions: [markdown(), referenceCardExtension(), cardProviders.of(provider), ...(opts?.extensions ?? [])],
      }),
      parent: container,
    });
  }

  test('the URL field commits only the destination range as one undoable change', () => {
    const view = mountLinkView('[text](old-url)', { extensions: [history()] });
    openCardAt(view, view.state.doc.toString().indexOf('text') + 1);
    const input = view.dom.querySelector('.cm-reference-card input') as HTMLInputElement;
    input.value = 'new-url';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    expect(view.state.doc.toString()).toBe('[text](new-url)');
    undo(view);
    expect(view.state.doc.toString()).toBe('[text](old-url)');
    view.destroy();
  });

  test('Escape leaves the document unchanged even after typing into the field', () => {
    const view = mountLinkView('[text](old-url)');
    openCardAt(view, view.state.doc.toString().indexOf('text') + 1);
    const input = view.dom.querySelector('.cm-reference-card input') as HTMLInputElement;
    input.value = 'something-typed';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(view.state.doc.toString()).toBe('[text](old-url)');
    view.destroy();
  });

  test('Unlink removes the brackets and keeps the text', () => {
    const view = mountLinkView('[text](old-url)');
    openCardAt(view, view.state.doc.toString().indexOf('text') + 1);
    const unlinkBtn = [...view.dom.querySelectorAll('.cm-reference-card button')]
      .find((b) => b.textContent === t('reference.unlink'))!;
    (unlinkBtn as HTMLButtonElement).click();
    expect(view.state.doc.toString()).toBe('text');
    view.destroy();
  });
});

describe('editable image/media card — assetCardProvider opts', () => {
  afterEach(() => { document.body.innerHTML = ''; });

  /** A document containing one `![[file]]` / `![[file|width]]` wikilink
   *  embed, wired the way cm-editor.ts registers `assetCardProvider` with
   *  `opts` (page path, project root, and the built `applyEmbedWidth` —
   *  `setImageWidth` here is a bare `vi.fn()`, never the engine command:
   *  `createApplyEmbedWidth` is the same factory cm-editor.ts uses, so this
   *  exercises the real width-preset wiring. */
  function mountEmbedView(doc: string, setImageWidth = vi.fn<(md: string, width: string | null) => Promise<string>>()): EditorView {
    const cache = { get: () => ({ kind: { kind: 'image' } }) } as any;
    const provider = assetCardProvider(cache, {
      pagePath: () => 'note.md', projectRoot: () => '/vault',
      applyWidth: createApplyEmbedWidth(setImageWidth),
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    return new EditorView({
      state: EditorState.create({
        doc,
        extensions: [markdown({ extensions: [wikilinkConfig] }), referenceCardExtension(), cardProviders.of(provider)],
      }),
      parent: container,
    });
  }

  test('the path field commits only the target range, leaving an existing width suffix untouched', () => {
    const view = mountEmbedView('![[old.png|50%]]');
    openCardAt(view, 3);
    const input = view.dom.querySelector('.cm-reference-card input') as HTMLInputElement;
    input.value = 'new.png';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    expect(view.state.doc.toString()).toBe('![[new.png|50%]]');
    view.destroy();
  });

  test('a width preset writes |NN%', async () => {
    const setImageWidth = vi.fn<(md: string, width: string | null) => Promise<string>>()
      .mockResolvedValue('![[old.png|75%]]');
    const view = mountEmbedView('![[old.png]]', setImageWidth);
    openCardAt(view, 3);
    const btn = [...view.dom.querySelectorAll('.cm-reference-card button')]
      .find((b) => b.textContent === '75%')! as HTMLButtonElement;
    btn.click();
    await Promise.resolve();
    await Promise.resolve();
    expect(setImageWidth).toHaveBeenCalledWith('![[old.png]]', '75%');
    expect(view.state.doc.toString()).toBe('![[old.png|75%]]');
    view.destroy();
  });
});

describe('unlinkAt', () => {
  const stateOf = (doc: string) => EditorState.create({ doc, extensions: [markdown({ extensions: [wikilinkConfig] })] });
  const doc = 'see [text](https://x) here';
  test('unlinkAt keeps the label and drops the brackets and URL', () => {
    const view = new EditorView({ state: stateOf(doc) });
    expect(unlinkAt(view, 14)).toBe(true);
    expect(view.state.doc.toString()).toBe('see text here');
    expect(unlinkAt(view, 2)).toBe(false);
    view.destroy();
  });
});


describe('current article map card', () => {
  const doc = '![[#|style:map|align-right 50%]]';
  function state(text = doc) { return EditorState.create({ doc: text, extensions: [markdown({ extensions: [wikilinkConfig] })] }); }
  test('identifies the current article and shows live locations in declared order', () => {
    const reveal = vi.fn();
    let locations = ['台北', '京都'];
    const provider = currentArticleCardProvider(() => ({ title: '尋找謝燁', path: '獎項/尋找謝燁.md', locations, revealLocations: reveal }));
    const spec = provider(state(), 3)!;
    expect(spec.title).toContain('尋找謝燁');
    expect(spec.body.textContent).toContain('This map uses this article’s locations');
    expect(spec.body.textContent).toContain('台北');
    expect(spec.body.textContent!.indexOf('台北')).toBeLessThan(spec.body.textContent!.indexOf('京都'));
    expect(spec.body.textContent).toContain('獎項/尋找謝燁.md');
    expect(spec.body.textContent).not.toContain('not found');
    locations = ['東京'];
    expect(provider(state(), 3)!.body.textContent).toContain('東京');
    const view = new EditorView({ state: state() });
    spec.actions![0].run(view);
    expect(reveal).toHaveBeenCalledOnce();
    view.destroy();
  });
  test('empty locations give a truthful next action; anchors and other articles retain their providers', () => {
    const provider = currentArticleCardProvider(() => ({ title: 'Article', path: 'article.md', locations: [], revealLocations: vi.fn() }));
    expect(provider(state(), 3)!.body.textContent).toContain('Add locations to this article');
    expect(provider(state(), 3)!.actions).toEqual([]);
    expect(provider(state('![[#section|style:map]]'), 3)).toBeNull();
    expect(provider(state('![[other.md|style:map]]'), 3)).toBeNull();
    expect(provider(state('[[#]]'), 2)).toBeNull();
    for (const doc of ['![](#)', '![](# "style:map")', '![[#]]', '![[#|style:list]]', '![[#|style:maple]]', '![[#|style:map|style:list]]']) {
      expect(provider(state(doc), doc.indexOf('#'))).toBeNull();
      expect(isCurrentArticleMapAt(state(doc), doc.indexOf('#'))).toBe(false);
    }
  });
});


test('heading-only embeds never get a missing-media card', () => {
  const state = EditorState.create({ doc: '![[#section]]', extensions: [markdown({ extensions: [wikilinkConfig] })] });
  expect(assetCardProvider({ get: () => ({ kind: { kind: 'anchor' } }) })(state, 3)).toBeNull();
});


test('map matching reads the same parsed style modifiers for hover and follow', () => {
  for (const doc of ['![[#|style:map|align-right 50%]]', '![[#|align-right 50%|style:map]]', '![[#|style:list|style:map]]', '![[#|style:map,sort:title|align-right 50%]]']) {
    const state = EditorState.create({ doc, extensions: [markdown({ extensions: [wikilinkConfig] })] });
    expect(isCurrentArticleMapAt(state, doc.indexOf('#'))).toBe(true);
  }
});
