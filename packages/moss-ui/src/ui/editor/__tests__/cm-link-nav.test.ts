import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import type { EditorReferenceResolution } from '@symbiosis-lab/moss-ui';

import { host } from "../../host";
const emitSpy = vi.spyOn(host.bus, 'emit');

const openUrlSpy = vi.fn().mockResolvedValue(undefined);
vi.mock("../../utils/open-external", () => ({ openExternal: (...args: unknown[]) => openUrlSpy(...args) }));

import { linkNavExtension, chordHeldField, chordTrackingExtension, followTarget, followTargetAt, navigateToReference, followAtCaret, referenceSiteAt, hitSiteAt } from '../cm-link-nav';
import type { ReferenceCacheRead } from '@symbiosis-lab/moss-syntax/cm6';

import { EditorView } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { wikilinkConfig, footnoteConfig, shortcodeBlockConfig } from '@symbiosis-lab/moss-syntax';
import { markdownStyleExtension } from '../cm-markdown-style';
import { isMacPlatform } from "../../utils/kbd-label";

// ── chordTrackingExtension — the shared keydown/keyup pair, factored out of
// linkNavExtension so cm-reference-card.ts's chord+hover arming reads the
// SAME flag the pointer-cursor class uses, instead of a second copy that can
// drift. ──────────────────────────────────────────────────────────────────
describe('chordTrackingExtension', () => {
  function view() {
    return new EditorView({ state: EditorState.create({ extensions: [chordTrackingExtension()] }) });
  }

  test('keydown sets the field and the DOM class; keyup clears both', () => {
    const v = view();
    document.body.appendChild(v.dom);
    expect(v.state.field(chordHeldField)).toBe(false);
    const key = isMacPlatform() ? 'Meta' : 'Control';
    v.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
    expect(v.state.field(chordHeldField)).toBe(true);
    expect(v.dom.classList.contains('cm-meta-held')).toBe(true);
    v.contentDOM.dispatchEvent(new KeyboardEvent('keyup', { key, bubbles: true }));
    expect(v.state.field(chordHeldField)).toBe(false);
    expect(v.dom.classList.contains('cm-meta-held')).toBe(false);
    v.destroy();
    document.body.innerHTML = '';
  });
});

// ── Following an external URL — `followTarget` widens to a third
// shape so an external reference opens through the platform opener instead of
// being treated as unfollowable. ───────────────────────────────────────────
describe('following an external URL', () => {
  beforeEach(() => { openUrlSpy.mockClear(); });

  test('followTarget returns the external branch, not null, for an external kind', () => {
    expect(followTarget({
      kind: { kind: 'external', data: { url: 'https://x' } },
      url: null, resolved_path: null, resolved: null,
    })).toEqual({ external: 'https://x' });
  });

  test('an external target opens via the platform opener', () => {
    const cache: ReferenceCacheRead = {
      get: () => ({
        kind: { kind: 'external', data: { url: 'https://example.com' } },
        url: null, resolved_path: null, resolved: null,
      } as EditorReferenceResolution),
    };
    const opened = navigateToReference(cache, { text: 'https://example.com', isEmbed: false });
    expect(opened).toBe(true);
    expect(openUrlSpy).toHaveBeenCalledWith('https://example.com');
  });
});

// ---------------------------------------------------------------------------
// linkNavExtension — Cmd+Click navigation on resolved links.
//
// The nav reads the SHARED reference cache keyed (text,
// is_embed:false): a link is navigable iff its envelope carries a
// `resolved_path`. We back the cache with a plain Map<string, envelope> here.
//
// The gesture is bound to `mousedown`, so these dispatch `mousedown` — see the
// handler's comment in cm-link-nav.ts for why the press and not the click.
// What jsdom CANNOT check is the reason the binding matters: it has no layout,
// so `posAtCoords` is mocked here and no element is ever destroyed under a
// pointer. Browser tests that follow links on the first press
// verify that pointer behavior.
// ---------------------------------------------------------------------------

/** Build a link envelope (is_embed:false) with the given resolved_path. */
function linkEnvelope(target: string, resolvedPath: string | null): EditorReferenceResolution {
  return {
    target,
    is_embed: false,
    kind: resolvedPath ? { kind: 'link', data: { anchor: null } } : { kind: 'not-found' },
    resolved: null,
    url: resolvedPath ? `/${target}` : null,
    anchor: null,
    resolved_path: resolvedPath,
    message: null,
    candidates: [],
    folder: null,
  };
}

/** Build an EMBED envelope (is_embed:true). `resolvedPath` is the folder's
 *  index source; `assetPath` the asset's own file — the two ways a reference
 *  names something to open. */
function embedEnvelope(
  target: string,
  opts: { resolvedPath?: string | null; assetPath?: string | null; kind?: string } = {},
): EditorReferenceResolution {
  return {
    target,
    is_embed: true,
    kind: { kind: (opts.kind ?? 'folder-listing') } as EditorReferenceResolution['kind'],
    resolved: opts.assetPath
      ? ({ absolute_path: opts.assetPath, request_url: '', mime_type: '', size_bytes: 0,
           provenance: 'literal' } as unknown as EditorReferenceResolution['resolved'])
      : null,
    url: null,
    anchor: null,
    resolved_path: opts.resolvedPath ?? null,
    message: null,
    candidates: [],
    folder: null,
  };
}

describe('linkNavExtension', () => {
  let container: HTMLElement;
  let view: EditorView;
  let store: Map<string, EditorReferenceResolution>;
  let resolvedCache: ReferenceCacheRead;
  let dispatched: CustomEvent[];
  let docHandler: EventListener;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
    store = new Map();
    // BOTH halves of the (text, is_embed) cache: nav now follows embeds too,
    // and the two entries for one text must stay separate.
    resolvedCache = { get: (text, isEmbed) => store.get(`${text} ${isEmbed}`) };
    dispatched = [];

    // Listen for the editor:file-selected custom event (dispatched on document)
    docHandler = ((e: CustomEvent) => {
      dispatched.push(e);
    }) as EventListener;
    document.addEventListener('editor:file-selected', docHandler);
  });

  afterEach(() => {
    if (view) view.destroy();
    document.body.innerHTML = '';
    document.removeEventListener('editor:file-selected', docHandler);
    dispatched = [];
  });

  function createView(doc: string): EditorView {
    // Include the markdown language + wikilinkConfig so Lezer emits Link/Wikilink
    // nodes that extractLinkTargets (used by linkNavExtension) can find.
    const state = EditorState.create({
      doc,
      extensions: [
        markdown({ extensions: [wikilinkConfig] }),
        linkNavExtension(resolvedCache),
      ],
    });
    view = new EditorView({ state, parent: container });
    return view;
  }

  test('Cmd+Click on a resolved link dispatches editor:file-selected', () => {
    const doc = 'See [[My Page]] for details.';
    createView(doc);

    // "My Page" is resolved with an absolute source path.
    store.set('My Page false', linkEnvelope('My Page', '/project/my-page.md'));

    // Position inside the target: after 'See [[' (6) + 2 = 8
    const clickPos = doc.indexOf('[[') + 2 + 2; // 2 past '[[', then 2 into 'My Page'

    // JSDOM doesn't compute real coordinates, so we mock posAtCoords
    // to return the click position directly.
    const event = new MouseEvent('mousedown', {
      metaKey: true,
      bubbles: true,
    });
    view.posAtCoords = () => clickPos;
    view.contentDOM.dispatchEvent(event);

    expect(dispatched).toHaveLength(1);
    expect(dispatched[0].detail.filePath).toBe('/project/my-page.md');
  });

  test('a click that lands at the end of a line does not follow its last link', () => {
    // `posAtCoords` answers `line.to` for a click inside a block widget (an
    // embed card) or in the blank space right of the text — neither is a click
    // on the link that happens to end the line.
    const doc = 'see [the docs](/docs/)';
    createView(doc);
    store.set('/docs/ false', linkEnvelope('/docs/', '/project/docs.md'));

    view.posAtCoords = () => doc.length;
    view.contentDOM.dispatchEvent(new MouseEvent('mousedown', { metaKey: true, bubbles: true }));

    expect(dispatched).toHaveLength(0);
  });

  test('Cmd+Click on an unresolved link does nothing', () => {
    const doc = 'See [[Missing Page]] for details.';
    createView(doc);

    // "Missing Page" is NOT resolved (not-found envelope, no resolved_path).
    store.set('Missing Page false', linkEnvelope('Missing Page', null));

    // Position inside the wikilink target
    const clickPos = doc.indexOf('[[') + 2 + 2;

    const event = new MouseEvent('mousedown', {
      metaKey: true,
      bubbles: true,
    });
    view.posAtCoords = () => clickPos;
    view.contentDOM.dispatchEvent(event);

    expect(dispatched).toHaveLength(0);
  });

  test('a secondary-button press with the chord held does not follow', () => {
    // The gesture listens on mousedown, where the right button arrives too —
    // and on Windows/Linux the chord is Ctrl, which a user holds for reasons
    // of their own. Right-clicking a link must still reach the context menu.
    const doc = 'See [[My Page]] for details';
    createView(doc);
    store.set('My Page false', linkEnvelope('My Page', '/project/my-page.md'));

    view.posAtCoords = () => doc.indexOf('[[') + 2 + 2;
    view.contentDOM.dispatchEvent(
      new MouseEvent('mousedown', { metaKey: true, ctrlKey: true, button: 2, bubbles: true }),
    );

    expect(dispatched).toHaveLength(0);
  });

  test('Cmd+Click on position with no link does nothing', () => {
    const doc = 'Just plain text with no links.';
    createView(doc);

    const event = new MouseEvent('mousedown', {
      metaKey: true,
      bubbles: true,
    });
    view.posAtCoords = () => 5; // position in plain text
    view.contentDOM.dispatchEvent(event);

    expect(dispatched).toHaveLength(0);
  });

  test('plain click (no Cmd/Ctrl) does not dispatch', () => {
    const doc = 'See [[My Page]] for details.';
    createView(doc);

    store.set('My Page false', linkEnvelope('My Page', '/project/my-page.md'));

    const clickPos = doc.indexOf('[[') + 2 + 2;

    const event = new MouseEvent('mousedown', {
      metaKey: false,
      ctrlKey: false,
      bubbles: true,
    });
    view.posAtCoords = () => clickPos;
    view.contentDOM.dispatchEvent(event);

    expect(dispatched).toHaveLength(0);
  });

  test('Ctrl+Click on a resolved link dispatches (Windows/Linux)', () => {
    const doc = 'See [[My Page]] for details.';
    createView(doc);

    store.set('My Page false', linkEnvelope('My Page', '/project/my-page.md'));

    const clickPos = doc.indexOf('[[') + 2 + 2;

    const event = new MouseEvent('mousedown', {
      ctrlKey: true,
      bubbles: true,
    });
    view.posAtCoords = () => clickPos;
    view.contentDOM.dispatchEvent(event);

    expect(dispatched).toHaveLength(1);
    expect(dispatched[0].detail.filePath).toBe('/project/my-page.md');
  });

  test('Cmd+Click on a link to a generated page navigates the preview', () => {
    // A synthesized page (an index-less folder listing, a generated author
    // page) is a deployed URL with no source file: following it shows the
    // page in the main preview instead of opening nothing.
    emitSpy.mockClear();
    const doc = 'By [馬欣宜](/authors/馬欣宜/) here.';
    createView(doc);
    store.set('/authors/馬欣宜/ false', {
      ...linkEnvelope('/authors/馬欣宜/', null),
      kind: { kind: 'link', data: { anchor: null } },
      url: '/authors/馬欣宜/',
    });
    const clickPos = doc.indexOf('/authors/') + 3;
    view.posAtCoords = () => clickPos;
    view.contentDOM.dispatchEvent(new MouseEvent('mousedown', { metaKey: true, bubbles: true }));

    expect(dispatched).toHaveLength(0);
    expect(emitSpy).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/authors/馬欣宜/' });
  });

  test('Cmd+Click on a moved link opens the page it lives at now', () => {
    const doc = 'By [馬欣宜](/authors/馬欣宜/) here.';
    createView(doc);
    store.set('/authors/馬欣宜/ false', {
      ...linkEnvelope('/authors/馬欣宜/', '/vault/about/ma.md'),
      kind: { kind: 'moved', data: { canonical: '/about/ma/' } },
      url: '/about/ma/',
    });
    view.posAtCoords = () => doc.indexOf('/authors/') + 3;
    view.contentDOM.dispatchEvent(new MouseEvent('mousedown', { metaKey: true, bubbles: true }));

    expect(dispatched).toHaveLength(1);
    expect(dispatched[0].detail.filePath).toBe('/vault/about/ma.md');
  });

  test('Cmd+Click on a not-found link does nothing', () => {
    const doc = 'See [[External Link]] for details.';
    createView(doc);

    // Not found: no file, no URL to show.
    store.set('External Link false', linkEnvelope('External Link', null));

    const clickPos = doc.indexOf('[[') + 2 + 2;

    const event = new MouseEvent('mousedown', {
      metaKey: true,
      bubbles: true,
    });
    view.posAtCoords = () => clickPos;
    view.contentDOM.dispatchEvent(event);

    expect(dispatched).toHaveLength(0);
  });

  test('Cmd+Click on a markdown link dispatches correctly', () => {
    const doc = 'Click [here](./page.md) to continue.';
    createView(doc);

    store.set('./page.md false', linkEnvelope('./page.md', '/project/page.md'));

    // A click on the link's URL (the text and brackets are inert)
    const clickPos = doc.indexOf('./page') + 3;

    const event = new MouseEvent('mousedown', {
      metaKey: true,
      bubbles: true,
    });
    view.posAtCoords = () => clickPos;
    view.contentDOM.dispatchEvent(event);

    expect(dispatched).toHaveLength(1);
    expect(dispatched[0].detail.filePath).toBe('/project/page.md');
  });

  test('event is dispatched on the editor DOM element (bubbles up)', () => {
    const doc = 'See [[My Page]] for details.';
    createView(doc);

    store.set('My Page false', linkEnvelope('My Page', '/project/my-page.md'));

    const clickPos = doc.indexOf('[[') + 2 + 2;

    // Listen on document level
    let caughtOnDocument = false;
    const handler = () => { caughtOnDocument = true; };
    document.addEventListener('editor:file-selected', handler);

    const event = new MouseEvent('mousedown', {
      metaKey: true,
      bubbles: true,
    });
    view.posAtCoords = () => clickPos;
    view.contentDOM.dispatchEvent(event);

    expect(caughtOnDocument).toBe(true);
    document.removeEventListener('editor:file-selected', handler);
  });
});

// ── Following an EMBED ──────────────────────────────────────────────────────
// Embeds were unfollowable by any gesture: extractLinkTargets skips
// WikilinkEmbed (correctly — a lint underline belongs on link text,
// not a card), and nothing else looked. These cover the one rule that now
// serves links and embeds alike, plus the keyboard path that used to not exist
// for either.
/** Run `fn` with `navigator.platform` reporting macOS, then put it back.
 *  `platform` lives on the prototype, so restoring means DELETING the own
 *  property when there was none — a plain re-define leaks 'MacIntel' into
 *  every later test in the file. */
function asMacPlatform(fn: () => void): void {
  const prior = Object.getOwnPropertyDescriptor(navigator, 'platform');
  Object.defineProperty(navigator, 'platform', { value: 'MacIntel', configurable: true });
  try {
    fn();
  } finally {
    if (prior) Object.defineProperty(navigator, 'platform', prior);
    else delete (navigator as unknown as Record<string, unknown>).platform;
  }
}

describe('following an embed', () => {
  let container: HTMLElement;
  let view: EditorView;
  let store: Map<string, EditorReferenceResolution>;
  let resolvedCache: ReferenceCacheRead;
  let dispatched: CustomEvent[];
  let docHandler: EventListener;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
    store = new Map();
    resolvedCache = { get: (text, isEmbed) => store.get(`${text} ${isEmbed}`) };
    dispatched = [];
    docHandler = ((e: CustomEvent) => { dispatched.push(e); }) as EventListener;
    document.addEventListener('editor:file-selected', docHandler);
  });

  afterEach(() => {
    if (view) view.destroy();
    document.body.innerHTML = '';
    document.removeEventListener('editor:file-selected', docHandler);
  });

  function createView(doc: string, selectionAt?: number): EditorView {
    const state = EditorState.create({
      doc,
      selection: selectionAt === undefined ? undefined : { anchor: selectionAt },
      extensions: [
        markdown({ extensions: [wikilinkConfig] }),
        linkNavExtension(resolvedCache),
      ],
    });
    view = new EditorView({ state, parent: container });
    return view;
  }

  /** Press Mod-Enter through the real keymap. */
  function pressModEnter(): void {
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'Enter', ctrlKey: true, bubbles: true, cancelable: true,
    }));
  }

  test('Mod-Enter on a folder embed opens its index source', () => {
    const doc = 'x\n\n![[/awards/|style:grid]]\n';
    // The folder publishes at /awards/ but its source is the CJK file — the
    // slug-override case the backend resolves through FolderFacts.
    store.set('/awards/ true', embedEnvelope('/awards/', {
      resolvedPath: '/project/獎項/獎項.md',
    }));
    createView(doc, doc.indexOf('![[') + 4);

    pressModEnter();

    expect(dispatched).toHaveLength(1);
    expect(dispatched[0].detail.filePath).toBe('/project/獎項/獎項.md');
  });

  test('Mod-Enter on an asset embed opens the asset file', () => {
    const doc = 'see ![[docs/paper.pdf]]\n';
    store.set('docs/paper.pdf true', embedEnvelope('docs/paper.pdf', {
      kind: 'pdf', assetPath: '/project/docs/paper.pdf',
    }));
    createView(doc, doc.indexOf('![[') + 4);

    pressModEnter();

    expect(dispatched).toHaveLength(1);
    expect(dispatched[0].detail.filePath).toBe('/project/docs/paper.pdf');
  });

  test('Mod-Enter on a folder with a synthesized index does nothing', () => {
    const doc = '![[/notes/]]\n';
    // Known folder, but no index source: resolved_path is null, so there is
    // no file to open and Enter must stay Enter.
    store.set('/notes/ true', embedEnvelope('/notes/', { resolvedPath: null }));
    createView(doc, 4);

    pressModEnter();

    expect(dispatched).toHaveLength(0);
  });

  test('on macOS only Command paints the followable cursor', () => {
    // The cursor and the click must name the same key: Ctrl+click is the
    // secondary click on macOS, so a pointer under Ctrl would promise a follow
    // the click then refuses.
    asMacPlatform(() => {
      createView('plain text\n', 0);
      view.contentDOM.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Control', bubbles: true }),
      );
      expect(view.dom.classList.contains('cm-meta-held')).toBe(false);

      view.contentDOM.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Meta', bubbles: true }),
      );
      expect(view.dom.classList.contains('cm-meta-held')).toBe(true);

      view.contentDOM.dispatchEvent(
        new KeyboardEvent('keyup', { key: 'Meta', bubbles: true }),
      );
      expect(view.dom.classList.contains('cm-meta-held')).toBe(false);
    });
  });

  test('off macOS, both Ctrl and Meta arm the cursor they follow with', () => {
    // `isFollowChord` accepts either there, so the cursor must too — a key that
    // navigates without a pointer is the same defect as a pointer that refuses
    // to navigate.
    createView('plain text\n', 0);
    for (const key of ['Control', 'Meta']) {
      view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
      expect(view.dom.classList.contains('cm-meta-held')).toBe(true);
      view.contentDOM.dispatchEvent(new KeyboardEvent('keyup', { key, bubbles: true }));
      expect(view.dom.classList.contains('cm-meta-held')).toBe(false);
    }
  });

  test('a linked embed follows the LINK, not the image inside it', () => {
    // `[![cover](cover.png)](/about/)` is both at once. The rendered widget
    // follows the href when clicked, so the source must answer the same way —
    // otherwise the target depends on whether the source happened to be
    // revealed under the caret.
    const doc = '[![cover](cover.png)](/about/)\n';
    store.set('cover.png true', embedEnvelope('cover.png', {
      kind: 'image', assetPath: '/project/cover.png',
    }));
    store.set('/about/ false', linkEnvelope('/about/', '/project/about.md'));
    createView(doc, doc.indexOf('cover.png') + 2);

    pressModEnter();

    expect(dispatched).toHaveLength(1);
    expect(dispatched[0].detail.filePath).toBe('/project/about.md');
  });

  test('the same embed unwrapped follows the image', () => {
    const doc = '![cover](cover.png)\n';
    store.set('cover.png true', embedEnvelope('cover.png', {
      kind: 'image', assetPath: '/project/cover.png',
    }));
    createView(doc, doc.indexOf('cover.png') + 2);

    pressModEnter();

    expect(dispatched).toHaveLength(1);
    expect(dispatched[0].detail.filePath).toBe('/project/cover.png');
  });

  test('Mod-Enter at a line-start before a link leaves Enter alone', () => {
    // defaultKeymap (via minimalSetup) binds Mod-Enter to insertBlankLine, and
    // this keymap outranks it. A caret BESIDE a reference is not on it, so the
    // command must fall through rather than swallowing a blank-line insert.
    const doc = '[[My Page]] opens something\n';
    store.set('My Page false', linkEnvelope('My Page', '/project/my-page.md'));
    createView(doc, 0);

    pressModEnter();

    expect(dispatched).toHaveLength(0);
  });

  test('Mod-Enter just past a closing ]] leaves Enter alone', () => {
    const doc = 'see [[My Page]]\n';
    store.set('My Page false', linkEnvelope('My Page', '/project/my-page.md'));
    createView(doc, doc.indexOf(']]') + 2);

    pressModEnter();

    expect(dispatched).toHaveLength(0);
  });

  test('Mod-Enter away from any reference does nothing', () => {
    const doc = 'just prose\n';
    createView(doc, 5);

    pressModEnter();

    expect(dispatched).toHaveLength(0);
  });

  test('Cmd+Click on the raw source of an embed follows it', () => {
    const doc = '![[/awards/|style:grid]]\n';
    store.set('/awards/ true', embedEnvelope('/awards/', {
      resolvedPath: '/project/獎項/獎項.md',
    }));
    createView(doc);

    view.posAtCoords = () => 5;
    view.contentDOM.dispatchEvent(new MouseEvent('mousedown', { metaKey: true, bubbles: true }));

    expect(dispatched).toHaveLength(1);
    expect(dispatched[0].detail.filePath).toBe('/project/獎項/獎項.md');
  });

  test('the same text as embed and as link follow different files', () => {
    const doc = '![[/notes/]] and [[/notes/]]\n';
    store.set('/notes/ true', embedEnvelope('/notes/', { resolvedPath: '/project/notes/index.md' }));
    store.set('/notes/ false', linkEnvelope('/notes/', '/project/elsewhere.md'));
    createView(doc);

    // Inside the embed.
    view.posAtCoords = () => 4;
    view.contentDOM.dispatchEvent(new MouseEvent('mousedown', { metaKey: true, bubbles: true }));
    expect(dispatched.at(-1)?.detail.filePath).toBe('/project/notes/index.md');

    // Inside the link that follows it.
    view.posAtCoords = () => doc.indexOf('and') + 6;
    view.contentDOM.dispatchEvent(new MouseEvent('mousedown', { metaKey: true, bubbles: true }));
    expect(dispatched.at(-1)?.detail.filePath).toBe('/project/elsewhere.md');
    expect(dispatched).toHaveLength(2);
  });
});

// ---------------------------------------------------------------------------
// Footnotes answer the same follow chord, but navigate WITHIN the document —
// they move the caret instead of dispatching `editor:file-selected`. These
// tests assert on the caret, which is the whole observable outcome.
// ---------------------------------------------------------------------------
describe('following a footnote', () => {
  let container: HTMLElement;
  let view: EditorView;
  let dispatched: CustomEvent[];
  let docHandler: EventListener;

  const DOC = 'a[^1] and b[^1]\n\n[^1]: the note\n';

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
    dispatched = [];
    docHandler = ((e: CustomEvent) => { dispatched.push(e); }) as EventListener;
    document.addEventListener('editor:file-selected', docHandler);
  });

  afterEach(() => {
    if (view) view.destroy();
    document.body.innerHTML = '';
    document.removeEventListener('editor:file-selected', docHandler);
  });

  function createView(doc: string, selectionAt: number): EditorView {
    const state = EditorState.create({
      doc,
      selection: { anchor: selectionAt },
      extensions: [
        markdown({ extensions: [wikilinkConfig, footnoteConfig] }),
        linkNavExtension({ get: () => undefined }),
      ],
    });
    view = new EditorView({ state, parent: container });
    return view;
  }

  function pressModEnter(): void {
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'Enter', ctrlKey: true, bubbles: true, cancelable: true,
    }));
  }

  test('Mod-Enter on a marker moves the caret to its definition', () => {
    createView(DOC, DOC.indexOf('[^1]') + 2);
    pressModEnter();
    expect(view.state.selection.main.head).toBe(DOC.indexOf('[^1]:') + 2);
    // In-document navigation: nothing is asked to open a file.
    expect(dispatched).toHaveLength(0);
  });

  test('Mod-Enter on the definition goes back to the FIRST marker', () => {
    createView(DOC, DOC.indexOf('[^1]:') + 2);
    pressModEnter();
    expect(view.state.selection.main.head).toBe(DOC.indexOf('[^1]') + 2);
  });

  test('the second marker with the same label reaches the same definition', () => {
    createView(DOC, DOC.indexOf('b[^1]') + 3);
    pressModEnter();
    expect(view.state.selection.main.head).toBe(DOC.indexOf('[^1]:') + 2);
  });

  test('Mod-Enter inside the note body leaves Enter alone', () => {
    // The body is prose. Swallowing Enter there would break writing the note.
    const at = DOC.indexOf('the note') + 3;
    createView(DOC, at);
    pressModEnter();
    expect(view.state.selection.main.head).not.toBe(DOC.indexOf('[^1]') + 2);
  });

  test('an undefined marker does not jump to a different footnote', () => {
    const doc = 'a[^zz]\n\n[^1]: the note\n';
    createView(doc, doc.indexOf('[^zz]') + 2);
    pressModEnter();
    expect(view.state.selection.main.head).toBe(doc.indexOf('[^zz]') + 2);
  });
});

// ---------------------------------------------------------------------------
// `followAtCaret` — the shared "footnote first, then the generic follow rule"
// body, extracted so the Mod-Enter keymap and the insert bar's reference page
// call ONE implementation instead of each repeating it.
// ---------------------------------------------------------------------------
describe('followAtCaret', () => {
  let container: HTMLElement;
  let view: EditorView;

  afterEach(() => {
    if (view) view.destroy();
    document.body.innerHTML = '';
  });

  test('followAtCaret follows a footnote before falling through to the generic rule', () => {
    container = document.createElement('div');
    document.body.appendChild(container);
    const doc = 'See[^1] it.\n\n[^1]: Note.';
    const state = EditorState.create({
      doc,
      extensions: [markdown({ extensions: [wikilinkConfig, footnoteConfig] })],
    });
    view = new EditorView({ state, parent: container });
    const cache: ReferenceCacheRead = { get: () => undefined };

    expect(followAtCaret(view, doc.indexOf('[^1]') + 1, cache)).toBe(true);
    expect(view.state.selection.main.head).toBe(doc.indexOf('[^1]:') + 2);
  });

  test('followAtCaret falls through to the generic follow rule when there is no footnote', () => {
    container = document.createElement('div');
    document.body.appendChild(container);
    const doc = 'see ![[docs/paper.pdf]]\n';
    const store = new Map<string, EditorReferenceResolution>();
    store.set('docs/paper.pdf true', embedEnvelope('docs/paper.pdf', {
      kind: 'pdf', assetPath: '/project/docs/paper.pdf',
    }));
    const cache: ReferenceCacheRead = { get: (text, isEmbed) => store.get(`${text} ${isEmbed}`) };
    const dispatched: CustomEvent[] = [];
    const handler = ((e: CustomEvent) => { dispatched.push(e); }) as EventListener;
    document.addEventListener('editor:file-selected', handler);
    const state = EditorState.create({
      doc,
      extensions: [markdown({ extensions: [wikilinkConfig] })],
    });
    view = new EditorView({ state, parent: container });

    expect(followAtCaret(view, doc.indexOf('![[') + 4, cache)).toBe(true);
    expect(dispatched).toHaveLength(1);
    expect(dispatched[0].detail.filePath).toBe('/project/docs/paper.pdf');
    document.removeEventListener('editor:file-selected', handler);
  });

  test('followAtCaret returns false away from any reference', () => {
    container = document.createElement('div');
    document.body.appendChild(container);
    const doc = 'just prose\n';
    const state = EditorState.create({ doc, extensions: [markdown({ extensions: [wikilinkConfig, footnoteConfig] })] });
    view = new EditorView({ state, parent: container });
    const cache: ReferenceCacheRead = { get: () => undefined };

    expect(followAtCaret(view, 5, cache)).toBe(false);
  });
});

// The caret (Mod-Enter) follows from any character of a link: it is an explicit
// command. The pointer follows only from the link's destination (below).
describe('followTargetAt over a link\'s own characters', () => {
  const doc = 'see [text](other-page.md) here';
  const state = EditorState.create({ doc, extensions: [markdown({ extensions: [wikilinkConfig] })] });
  test.each([
    ['the label', doc.indexOf('text') + 1],
    ['the closing bracket', doc.indexOf('](') + 1],
    ['the URL', doc.indexOf('other') + 3],
    ['the closing parenthesis', doc.indexOf(')') - 1],
  ])('from the caret, %s follows the link', (_where, pos) => {
    expect(followTargetAt(state, pos)).toMatchObject({ text: 'other-page.md', isEmbed: false });
  });
  test('prose beside the link follows nothing', () => {
    expect(followTargetAt(state, 1)).toBeNull();
    expect(followTargetAt(state, doc.length - 2)).toBeNull();
  });
});

const siteState = (doc: string) => EditorState.create({ doc, extensions: [markdown({ extensions: [wikilinkConfig] })] });

describe('referenceSiteAt: a link is found by position over any of its characters', () => {
  const doc = 'see [text](https://x) here';
  const site = { from: 4, to: 21, target: 'https://x', targetFrom: 11, targetTo: 20, isEmbed: false, wiki: false };

  test.each([
    ['the opening bracket', 5], ['the label', 6], ['the closing bracket', 9], ['the URL', 14], ['the closing parenthesis', 20],
    ['either edge (inclusive)', 4], ['either edge (inclusive)', 21],
  ])('%s', (_where, pos) => {
    expect(referenceSiteAt(siteState(doc), pos)).toEqual(site);
  });

  test('prose, a reference-style link, an autolink and a destination-less link are not links', () => {
    expect(referenceSiteAt(siteState(doc), 1)).toBeNull();
    expect(referenceSiteAt(siteState('[text][ref]\n\n[ref]: https://x'), 3)).toBeNull();
    expect(referenceSiteAt(siteState('<https://example.com>'), 5)).toBeNull();
    expect(referenceSiteAt(siteState('[text]()'), 3)).toBeNull();
    expect(referenceSiteAt(siteState('arr[0] and arr[1]'), 4)).toBeNull();
  });

  test('strict excludes both ends of the construct, and only the ends', () => {
    expect(referenceSiteAt(siteState(doc), 4, { strict: true })).toBeNull();
    expect(referenceSiteAt(siteState(doc), 21, { strict: true })).toBeNull();
    expect(referenceSiteAt(siteState(doc), 5, { strict: true })).toEqual(site);
    expect(referenceSiteAt(siteState(doc), 20, { strict: true })).toEqual(site);
  });

  test('a wikilink: the destination is the page (trimmed), the text the alias, else the page name', () => {
    expect(referenceSiteAt(siteState('a [[Page]] b'), 5)).toMatchObject({ wiki: true, target: 'Page', targetFrom: 4, targetTo: 8, from: 2, to: 10 });
    expect(referenceSiteAt(siteState('a [[Page|Shown]] b'), 5)).toMatchObject({ wiki: true, target: 'Page', targetFrom: 4, targetTo: 8 });
    expect(referenceSiteAt(siteState('a [[ Page |Shown]] b'), 6)).toMatchObject({ target: 'Page', targetFrom: 5, targetTo: 9 });
  });
});

// What the POINTER is on: the DESTINATION of a reference and nothing else. The
// link is where it goes; the words around it are just words, so pointing at
// them does nothing. Each row of the author's table (hover and follow agree
// except for a linked picture).
describe('hitSiteAt: the pointer is on a reference only over its destination', () => {
  const follow = (doc: string, pos: number) => followTargetAt(siteState(doc), pos, { pointer: true });
  const hover = (doc: string, pos: number) => hitSiteAt(siteState(doc), pos);

  test.each([
    ['[text](https://x)', 'https://x', ['[', 'text', ']', ')']],
    ['[[Page]]', 'Page', ['[', ']']],
    ['[[Page|Shown]]', 'Page', ['Shown', '|', '[', ']']],
    ['![alt](images/x.png)', 'images/x.png', ['alt', '![', ']', ')']],
    ['![[images/x.png]]', 'images/x.png', ['[[', ']]']],
    ['![[images/x.png|50%]]', 'images/x.png', ['|50', ']]']],
    ['[t](https://x "the title")', 'https://x', ['the title', 't']],
  ])('%s: hover and follow answer over the destination (%s), not over the rest', (doc, active, inert) => {
    const wrapped = `see ${doc} here`;
    const k = wrapped.indexOf(active);
    for (const pos of [k, k + 1, k + active.length - 1]) {
      expect(hover(wrapped, pos), `hover at ${pos}`).not.toBeNull();
      expect(follow(wrapped, pos), `follow at ${pos}`).not.toBeNull();
    }
    // The characters either side of the destination are inert too (a character's edge, not half of one beyond it).
    const inertAt = [k - 1, k + active.length, ...inert.map((needle) => wrapped.indexOf(needle) + (needle.length > 1 ? 1 : 0))];
    for (const pos of inertAt) {
      expect(hover(wrapped, pos), `hover at ${pos}`).toBeNull();
      expect(follow(wrapped, pos), `follow at ${pos}`).toBeNull();
    }
  });

  test('a markdown link follows its URL; a wikilink follows its page, label or not', () => {
    expect(follow('[text](a.md)', 8)).toMatchObject({ text: 'a.md', isEmbed: false });
    expect(follow('[[Page|Shown]]', 3)).toMatchObject({ text: 'Page', isEmbed: false });
    expect(follow('[[Page]]', 3)).toMatchObject({ text: 'Page', isEmbed: false });
  });

  test('a link title is not the destination; a reference-style link has none here and answers nothing', () => {
    const doc = '[t](a.md "title")';
    expect(follow(doc, doc.indexOf('title') + 1)).toBeNull();
    const ref = '[text][ref]\n\n[ref]: https://x';
    for (const pos of [1, 7, ref.indexOf('https') + 2]) expect(hover(ref, pos), `at ${pos}`).toBeNull();
  });

  test('an image follows its path as an embed', () => {
    expect(follow('![alt](images/x.png)', 8)).toMatchObject({ text: 'images/x.png', isEmbed: true });
    expect(follow('![[images/x.png|50%]]', 6)).toMatchObject({ text: 'images/x.png', isEmbed: true });
  });

  // `[![alt](img.png)](/about/)`: the picture's path hovers the picture, the outer URL is the link.
  describe('a linked picture', () => {
    const doc = '[![alt](img.png)](/about/)';
    test('hovering its path shows the picture', () => {
      expect(hover(doc, doc.indexOf('img') + 1)).toMatchObject({ isEmbed: true, target: 'img.png' });
    });
    test('following its path goes where the link goes', () => {
      expect(follow(doc, doc.indexOf('img') + 1)).toMatchObject({ text: '/about/', isEmbed: false });
    });
    test('the outer URL hovers and follows the link', () => {
      expect(hover(doc, doc.indexOf('about'))).toMatchObject({ isEmbed: false, target: '/about/' });
      expect(follow(doc, doc.indexOf('about'))).toMatchObject({ text: '/about/', isEmbed: false });
    });
    test('its alt text and brackets are inert', () => {
      for (const pos of [doc.indexOf('alt') + 1, 0, doc.indexOf('](/') + 1, doc.length - 1]) {
        expect(hover(doc, pos), `hover at ${pos}`).toBeNull();
        expect(follow(doc, pos), `follow at ${pos}`).toBeNull();
      }
    });
    test('link text beside the picture is inert', () => {
      const mixed = '[see ![i](p.png) here](/about/)';
      expect(follow(mixed, 2)).toBeNull();
      expect(follow(mixed, mixed.indexOf('here') + 1)).toBeNull();
    });
  });

  test('prose beside a link is not on it', () => {
    expect(hover('see [text](u) here', 1)).toBeNull();
  });
});

describe('the link colour and the pointer\'s hit range are one range', () => {
  test.each([
    'x [text](u.md) y',
    'x [see **bold** here](https://example.com/a(b)) y',
  ])('%s', (doc) => {
    const view = new EditorView({ state: EditorState.create({ doc, extensions: [markdown({ extensions: [wikilinkConfig] }), markdownStyleExtension()] }), parent: document.body });
    const site = referenceSiteAt(view.state, doc.indexOf('[') + 1)!;
    const url = [...view.dom.querySelectorAll('.cm-lp-link-dest')].map((e) => e.textContent).join('');
    view.destroy();
    expect(site).not.toBeNull();
    expect(url).toBe(doc.slice(site.targetFrom, site.targetTo));
  });
});

describe('the hand is withheld from a reference\'s inert parts', () => {
  const inertText = (doc: string) => {
    const view = new EditorView({ state: EditorState.create({ doc, extensions: [markdown({ extensions: [wikilinkConfig] }), linkNavExtension({ get: () => undefined })] }), parent: document.body });
    const out = [...view.dom.querySelectorAll('.cm-ref-inert')].map((e) => e.textContent);
    view.destroy();
    return out;
  };
  test.each([
    ['[text](u.md)', ['[text](', ')']],
    ['[[Page]]', ['[[', ']]']],
    ['[[Page|Shown]]', ['[[', '|Shown]]']],
    ['![alt](p.png)', ['![alt](', ')']],
    ['![[p.png|50%]]', ['![[', '|50%]]']],
    ['[![alt](i.png)](/a/)', ['[', '![alt](', ')', '](', ')']],
  ])('%s', (doc, expected) => {
    expect(inertText(`x ${doc} y`)).toEqual(expected);
  });
});

describe('referenceSiteAt with embeds (what Mod-Enter follows)', () => {
  test('an embed is found inside, not at its ends', () => {
    const state = siteState('x ![[pic.png]] y');
    expect(referenceSiteAt(state, 6, { strict: true, embeds: true })).toMatchObject({ isEmbed: true, target: 'pic.png', from: 2, to: 14, targetFrom: 5, targetTo: 12 });
    expect(referenceSiteAt(state, 2, { strict: true, embeds: true })).toBeNull();
    expect(referenceSiteAt(state, 6)).toBeNull(); // embeds are opt-in
  });

  test('a link that encloses an embed wins', () => {
    const state = siteState('[![cover](cover.png)](/about/)');
    expect(referenceSiteAt(state, 5, { strict: true, embeds: true })).toMatchObject({ isEmbed: false, target: '/about/' });
  });

  test('followTargetAt is this resolver, strict, mapped to its own shape', () => {
    const state = siteState('x ![[pic.png]] y');
    expect(followTargetAt(state, 6)).toEqual({ text: 'pic.png', isEmbed: true, from: 2, to: 14 });
    expect(followTargetAt(state, 14)).toBeNull();
  });
});

describe('hero media destinations share hover and navigation hit ranges', () => {
  test.each([
    ':::hero {image="assets/封面 photo.jpg|cover top"}',
    ':::hero assets/cover.jpg {wide=page}',
    ':::hero {image=assets/cover.jpg}',
  ])('%s', (opening) => {
    const doc = `${opening}\n# Title\n:::`;
    const state = EditorState.create({ doc, extensions: [markdown({ extensions: [wikilinkConfig, shortcodeBlockConfig] })] });
    const path = opening.includes('封面') ? 'assets/封面 photo.jpg' : 'assets/cover.jpg';
    const from = opening.indexOf(path);
    for (let pos = from; pos < from + path.length; pos++) {
      expect(hitSiteAt(state, pos)).toMatchObject({ target: path, targetFrom: from, targetTo: from + path.length, isEmbed: true });
      expect(hitSiteAt(state, pos, { follow: true })).toEqual(hitSiteAt(state, pos));
    }
    for (const pos of [0, from - 1, from + path.length, doc.indexOf('Title')]) {
      expect(hitSiteAt(state, pos)).toBeNull();
    }
    expect(followTargetAt(state, from + 1)).toMatchObject({ text: path, isEmbed: true });
  });

  test.each([':::hero {image=missing.jpg broken}', ':::grid {image=ignored.jpg}', '```\n:::hero {image=code.jpg}\n```'])('does not activate an unrecognized path: %s', (doc) => {
    const state = EditorState.create({ doc, extensions: [markdown({ extensions: [shortcodeBlockConfig] })] });
    expect(hitSiteAt(state, doc.indexOf('.jpg') - 1)).toBeNull();
  });
});

test('current-article follow reveals metadata without reloading or moving the caret', () => {
  const doc = 'Prose.\n\n![[#|style:map|align-right 50%]]';
  const selected = vi.fn();
  document.addEventListener('editor:file-selected', selected);
  const reveal = vi.fn((target) => target.text === '#' && target.isEmbed);
  const view = new EditorView({ state: EditorState.create({ doc, selection: { anchor: 2 }, extensions: [markdown({ extensions: [wikilinkConfig] })] }) });
  expect(followAtCaret(view, doc.indexOf('#'), { get: () => undefined }, reveal)).toBe(true);
  expect(reveal).toHaveBeenCalledOnce();
  expect(selected).not.toHaveBeenCalled();
  expect(view.state.selection.main.head).toBe(2);
  view.destroy();
  document.removeEventListener('editor:file-selected', selected);
});
