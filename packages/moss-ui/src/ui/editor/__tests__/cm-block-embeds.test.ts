import { describe, test, expect, vi } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { GFM } from '@lezer/markdown';
import { ensureSyntaxTree } from '@codemirror/language';
import { wikilinkConfig, footnoteConfig, mathConfig, shortcodeBlockConfig } from '@symbiosis-lab/moss-syntax';
import { criticmarkupExtension } from '@symbiosis-lab/moss-syntax/cm6';
import { commentExtension } from '../cm-comment';
import {
  blockEmbedsExtension,
  blockEmbedsIn, blockEmbedOnLine, embedAt,
  type ResolveEmbedFn,
} from '../cm-block-embeds';
import { MediaEmbedBlockWidget, type ResolveImageFn } from '../cm-block-embed-widgets';
import { markdownStyleExtension } from '../cm-markdown-style';
import { refsResolvedEffect } from '../cm-reference-resolver';
import type { FolderCardDetail } from '../cm-media-embed';

describe('a link-wrapped embed alone on its line is one block, its source untouched', () => {
  // `[![[x.jpg]]](url)` renders as ONE clickable image block — the
  // enclosing `[` … `](url)` must not count as "something else on the line"
  // and demote the embed to an inline picture. Its source line stays as text and
  // the rendered block sits below it.
  test('[![[x]]](url) is a block embed carrying its link unit', () => {
    const state = EditorState.create({
      doc: '[![[report.pdf]]](/reports/)',
      extensions: [markdown({ extensions: [wikilinkConfig] })],
    });
    const b = blockEmbedOnLine(state, 3)!;
    expect(b.from).toBe(1);
    expect(b.to).toBe(16);
    expect(b.unit?.href).toBe('/reports/');
    expect(b.line).toEqual({ from: 0, to: state.doc.length });
  });

  test('[![alt](x.jpg)](url) markdown-link form too', () => {
    const state = EditorState.create({
      doc: '[![cover](cover.png)](/about/)',
      extensions: [markdown()],
    });
    expect(blockEmbedOnLine(state, 3)?.unit?.href).toBe('/about/');
  });
});

describe('embedAt', () => {
  const wiki = [markdown({ extensions: [wikilinkConfig] })];
  test('any position on a block embed line answers, text elsewhere does not', () => {
    const state = EditorState.create({ doc: '![[a.png]]\n\nplain', extensions: wiki });
    expect(embedAt(state, 0)).toMatchObject({ from: 0, to: 10, inline: false });
    expect(embedAt(state, 10)).toMatchObject({ from: 0, to: 10, inline: false });
    expect(embedAt(state, 14)).toBeNull();
  });
  test('an inline embed answers over every one of its own characters, edges included, and only there', () => {
    const state = EditorState.create({ doc: 'see ![[a.png]] here', extensions: wiki });
    for (const pos of [4, 5, 7, 10, 13, 14]) expect(embedAt(state, pos)).toMatchObject({ from: 4, to: 14, inline: true, probe: 4 }); // a wikilink embed's target span is the whole node
    expect(embedAt(state, 1)).toBeNull();
    expect(embedAt(state, 16)).toBeNull();
  });
  test('![alt](x.png) inline: the probe is its path', () => {
    const state = EditorState.create({ doc: 'see ![a](x.png) here', extensions: [markdown()] });
    expect(embedAt(state, 5)).toMatchObject({ from: 4, to: 15, inline: true, probe: 9 });
  });
});

describe('an embed in a line of prose', () => {
  test('draws no widget: its source text is all there is', () => {
    const state = EditorState.create({
      doc: 'see ![[a.png]] here',
      extensions: [markdown({ extensions: [wikilinkConfig] }), blockEmbedsExtension({ resolveImage: () => ({ url: 'moss-source://a.png' }) })],
    });
    const view = new EditorView({ state });
    expect(view.dom.querySelector('.cm-lp-image-row, img')).toBeNull();
    expect(view.dom.textContent).toContain('![[a.png]]');
    view.destroy();
  });
  test('an embed alone on its line is a block embed', () => {
    const state = EditorState.create({ doc: '![[a.png]]', extensions: [markdown({ extensions: [wikilinkConfig] })] });
    expect(blockEmbedsIn(state, 0, 10)).toHaveLength(1);
  });
});

describe('composition', () => {
  // `blockEmbedsExtension` hosts its decorations in a StateField (block decorations
  // forbid a ViewPlugin), which never sees a ViewUpdate, so its composition guard
  // reads the transaction tag CM6 sets on a composition-driven edit.
  test('no block widget appears on a composing transaction', () => {
    const state = EditorState.create({ doc: 'plain', extensions: [markdown({ extensions: [wikilinkConfig] }), blockEmbedsExtension({ resolveImage: () => ({ url: 'x' }) })] });
    const view = new EditorView({ state });
    // would normally draw a picture below its line — tagged as a composition edit.
    view.dispatch({ changes: { from: 0, to: 5, insert: '![[a.png]]' }, userEvent: 'input.type.compose' });
    expect(view.dom.querySelector('.cm-lp-image-row')).toBeNull(); // deferred — old (empty) set just mapped through
    // A later, ordinary (non-composing) doc change lets the field rebuild and catch up.
    view.dispatch({ changes: { from: view.state.doc.length, to: view.state.doc.length, insert: ' ' } });
    expect(view.dom.querySelector('.cm-lp-image-row')).not.toBeNull();
    view.destroy();
  });
});

describe('RULE 1: no decoration hides source text in the markdown editor', () => {
  // Every construct the editor knows, in one document, through the extensions the
  // app composes for them. A replace decoration is a point decoration over a
  // non-empty range; a block widget below a line (a point at one position) is the
  // only rendered addition allowed.
  const corpus = [
    '# Heading', '',
    'A [link](https://x.example/very/long/url) and [[Page]], ![[a.png]] inline, a footnote.[^1]',
    'Also `code`, $x$, **bold**, ~~gone~~ and {==marked==}{>>a note<<} then {>>lone<<} {++add++} {--del--}.', '',
    '- [ ] open', '- [x] done', '', '> quote', '', '![[b.png]]', '', '---', '',
    ':::grid', '![[c.png]]', '+++', '![[d.png]]', ':::', '',
    '```js', 'const a = 1;', '```', '', '$$', 'E = mc^2', '$$', '', '| a | b |', '|---|---|', '| 1 | 2 |', '',
    '[^1]: note',
  ].join('\n');

  test('every decoration the composed extensions produce is a mark, a line class or a block widget below a line', () => {
    const view = new EditorView({
      state: EditorState.create({
        doc: corpus,
        extensions: [
          markdown({ extensions: [GFM, wikilinkConfig, mathConfig, footnoteConfig, shortcodeBlockConfig] }),
          markdownStyleExtension(),
          blockEmbedsExtension({ resolveImage: () => ({ url: 'x' }) }),
          commentExtension(),
          criticmarkupExtension(),
        ],
      }),
    });
    ensureSyntaxTree(view.state, view.state.doc.length, 1e9);
    view.dispatch({ changes: { from: view.state.doc.length, insert: '\n' } }); // let every field rebuild on the full tree
    const replaces: string[] = [];
    let blocks = 0;
    for (const provider of view.state.facet(EditorView.decorations)) {
      const set = typeof provider === 'function' ? provider(view) : provider;
      for (const cur = set.iter(); cur.value; cur.next()) {
        if (cur.value.point && cur.to > cur.from) replaces.push(view.state.sliceDoc(cur.from, cur.to));
        if (cur.value.point && cur.to === cur.from) blocks++;
      }
    }
    expect(replaces).toEqual([]);
    expect(blocks, 'the pictures below their source lines are still drawn').toBeGreaterThan(0);
    view.destroy();
  });
});

/** Every decoration of a live view's FULL set (each extension's contribution): its range, class and whether it carries a widget. */
function flattenViewDecorations(
  view: EditorView,
): { from: number; to: number; cls?: string; widget: boolean }[] {
  const out: { from: number; to: number; cls?: string; widget: boolean }[] = [];
  for (const provider of view.state.facet(EditorView.decorations)) {
    const set = typeof provider === 'function' ? provider(view) : provider;
    const cur = set.iter();
    while (cur.value) {
      const spec = cur.value.spec as { class?: string; widget?: unknown };
      out.push({ from: cur.from, to: cur.to, cls: spec.class, widget: spec.widget != null });
      cur.next();
    }
  }
  return out;
}

describe('block embed widget: below the source line, replacing nothing', () => {
  const imageView = (doc: string) => new EditorView({
    state: EditorState.create({
      doc,
      extensions: [markdown({ extensions: [wikilinkConfig] }), blockEmbedsExtension({ resolveImage: () => ({ url: 'x' }) })],
    }),
  });

  test('an image alone on its line keeps its source line as text and draws the picture after it', () => {
    const doc = '![[a.png]]\nnext';
    const view = imageView(doc);
    const flat = flattenViewDecorations(view);
    const block = flat.find((d) => d.widget);
    expect(block).toMatchObject({ from: 10, to: 10 }); // zero-width at the line end
    expect(flat.some((d) => d.widget && d.to > d.from)).toBe(false); // no replace over any source character
    expect(view.dom.querySelector('.cm-lp-image-block')).not.toBeNull();
    // The source line is on screen, characters intact.
    expect(view.dom.querySelector('.cm-line')!.textContent).toBe('![[a.png]]');
    // Order in the DOM: the line, then its picture, then the next line.
    const kids = [...view.contentDOM.children].map((c) => c.className.includes('cm-lp-image-row') ? 'row' : c.textContent);
    expect(kids).toEqual(['![[a.png]]', 'row', 'next']);
    view.destroy();
  });
  test('an unresolved image draws nothing below; its source line is all there is', () => {
    const view = new EditorView({
      state: EditorState.create({ doc: '![[a.png]]', extensions: [markdown({ extensions: [wikilinkConfig] }), blockEmbedsExtension({})] }),
    });
    expect(view.dom.querySelector('.cm-lp-image-row')).toBeNull();
    expect(view.dom.querySelector('.cm-line')!.textContent).toBe('![[a.png]]');
    view.destroy();
  });
  test('a rule --- draws no widget at all', () => {
    const state = EditorState.create({ doc: '---', extensions: [markdown(), blockEmbedsExtension({})] });
    const view = new EditorView({ state });
    expect(view.dom.querySelector('hr')).toBeNull();
    expect(flattenViewDecorations(view).filter((d) => d.widget)).toEqual([]);
    view.destroy();
  });
});

// ── MediaEmbedBlockWidget ──────
// Mirrors the harness in cm-image-block.test.ts: construct the widget
// directly, call `.toDOM()`, assert on the produced DOM (jsdom).

function mediaWidget(over: Partial<{
  kind: string;
  name: string;
  target: string;
  intraLineOffset: number;
  onSetWidth: (from: number, to: number, w: string | null) => void;
  resizable: boolean;
  width: string;
  folder: FolderCardDetail;
}> = {}) {
  return new MediaEmbedBlockWidget({
    kind: over.kind ?? 'folder',
    name: over.name ?? 'Resources/app',
    target: over.target ?? over.name ?? 'Resources/app',
    intraLineOffset: over.intraLineOffset ?? 0,
    onSetWidth: over.onSetWidth ?? (() => {}),
    resizable: over.resizable ?? true,
    width: over.width,
    folder: over.folder,
  });
}

describe('MediaEmbedBlockWidget structure', () => {
  test('outer element is the .cm-lp-image-row inset wrapper', () => {
    const dom = mediaWidget().toDOM();
    expect(dom.classList.contains('cm-lp-image-row')).toBe(true);
    // the card is the direct child of the inset row
    expect(dom.querySelector(':scope > .cm-media-embed-card')).toBeTruthy();
  });

  test('renders the kind label and the target name', () => {
    const dom = mediaWidget({ kind: 'pdf', name: 'report.pdf' }).toDOM();
    const label = dom.querySelector('.cm-media-embed-label');
    const name = dom.querySelector('.cm-media-embed-name');
    expect(label?.textContent).toBe('pdf');
    expect(name?.textContent).toBe('report.pdf');
  });

  test('unknown kind falls back to a generic embed label/icon', () => {
    const dom = mediaWidget({ kind: 'wat', name: 'x' }).toDOM();
    expect(dom.querySelector('.cm-media-embed-label')?.textContent).toBe('embed');
    expect(dom.querySelector('.cm-media-embed-icon')?.textContent).toBeTruthy();
  });
});

describe('MediaEmbedBlockWidget — inert target name (no navigation)', () => {
  test.each([
    ['data:text/html,<script>alert(1)</script>'],
    ['https://evil.example/x'],
  ])('%s is rendered as inert text, never an anchor/src', (evil) => {
    const dom = mediaWidget({ kind: 'external', name: evil }).toDOM();
    const name = dom.querySelector('.cm-media-embed-name') as HTMLElement;
    // exact escaped text, no markup injected
    expect(name.textContent).toBe(evil);
    expect(name.querySelector('script')).toBeNull();
    // no navigable element anywhere in the card
    expect(dom.querySelector('a')).toBeNull();
    expect(dom.querySelector('[href]')).toBeNull();
    expect(dom.querySelector('[src]')).toBeNull();
  });
});

describe('MediaEmbedBlockWidget — resize handle', () => {
  test('resizable:true appends a .cm-lp-image-handle (reused image CSS)', () => {
    const dom = mediaWidget({ resizable: true }).toDOM();
    const handle = dom.querySelector('.cm-lp-image-handle');
    expect(handle).toBeTruthy();
    expect(handle!.querySelector('svg')).toBeTruthy();
    expect(handle!.getAttribute('aria-label')).toBe('Resize embed');
  });

  test('resizable:false omits the handle', () => {
    const dom = mediaWidget({ resizable: false }).toDOM();
    expect(dom.querySelector('.cm-lp-image-handle')).toBeNull();
  });

  test('drag commits a percent width with coords resolved at gesture time', () => {
    // Commit coords come from the live syntax tree at gesture time (see
    // cm-image-resize-gesture-pos.test.ts for the stale-position regression).
    const embed = '![[demo.mp4]]';
    const state = EditorState.create({
      doc: embed,
      extensions: [markdown({ extensions: [wikilinkConfig] })],
    });
    const fakeView = {
      state,
      posAtDOM: () => state.doc.line(1).to,
      requestMeasure: () => {},
    } as any;
    const onSetWidth = vi.fn();
    const dom = mediaWidget({ resizable: true, onSetWidth }).toDOM(fakeView);
    const card = dom.querySelector('.cm-media-embed-card') as HTMLElement;
    // content basis 600px; drag the handle right by 60px from a 300px start.
    (dom as any).getBoundingClientRect = () => mediaRect(600);
    card.getBoundingClientRect = () => mediaRect(300);
    const handle = dom.querySelector('.cm-lp-image-handle') as HTMLElement;
    handle.dispatchEvent(new MouseEvent('pointerdown', { clientX: 0, bubbles: true, cancelable: true }));
    window.dispatchEvent(new MouseEvent('pointermove', { clientX: 60 }));
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: 60 }));
    // (300 + 60) / 600 = 60%
    expect(onSetWidth).toHaveBeenCalledWith(0, embed.length, '60%');
  });
});

describe('MediaEmbedBlockWidget — width', () => {
  test('explicit width applies to the card', () => {
    const card = mediaWidget({ width: '55%' }).toDOM()
      .querySelector('.cm-media-embed-card') as HTMLElement;
    expect(card.style.width).toBe('55%');
  });

  test('no explicit width leaves inline width unset (CSS width:100% wins)', () => {
    const card = mediaWidget().toDOM().querySelector('.cm-media-embed-card') as HTMLElement;
    expect(card.style.width).toBe('');
  });
});

describe('MediaEmbedBlockWidget — eq()', () => {
  test('width difference makes widgets unequal (resize repaints)', () => {
    const a = mediaWidget({ width: '50%' });
    const b = mediaWidget({ width: '60%' });
    expect(a.eq(b)).toBe(false);
  });

  test('kind / name / resizable differences flip eq()', () => {
    const base = mediaWidget();
    expect(base.eq(mediaWidget({ kind: 'pdf' }))).toBe(false);
    expect(base.eq(mediaWidget({ name: 'other' }))).toBe(false);
    expect(base.eq(mediaWidget({ resizable: false }))).toBe(false);
  });

  test('a folder detail change flips eq() — a stale count must repaint', () => {
    const base = mediaWidget({ folder: mediaFolderDetail() });
    expect(base.eq(mediaWidget({ folder: mediaFolderDetail() }))).toBe(true);
    expect(base.eq(mediaWidget({ folder: mediaFolderDetail({ title: 'Other' }) }))).toBe(false);
    expect(base.eq(mediaWidget({ folder: mediaFolderDetail({ childCount: 13 }) }))).toBe(false);
    expect(base.eq(mediaWidget({ folder: mediaFolderDetail({ chips: ['sort:date'] }) }))).toBe(false);
    expect(base.eq(mediaWidget())).toBe(false);
  });

  test('identical config is equal', () => {
    expect(mediaWidget().eq(mediaWidget())).toBe(true);
  });
});

function mediaFolderDetail(over: Partial<FolderCardDetail> = {}): FolderCardDetail {
  return {
    title: over.title !== undefined ? over.title : 'Awards',
    childCount: over.childCount !== undefined ? over.childCount : 12,
    chips: over.chips ?? ['style:grid', 'sort:weight'],
  };
}

describe('MediaEmbedBlockWidget — folder card', () => {
  const meta = (d: Partial<FolderCardDetail>) =>
    mediaWidget({ kind: 'folder', folder: mediaFolderDetail(d) }).toDOM();

  test('the index title replaces the target name', () => {
    const dom = meta({ title: '獎項' });
    expect(dom.querySelector('.cm-media-embed-name')?.textContent).toBe('獎項');
  });

  test('an unknown title falls back to the target name', () => {
    const dom = mediaWidget({ kind: 'folder', name: 'awards', folder: mediaFolderDetail({ title: null }) }).toDOM();
    expect(dom.querySelector('.cm-media-embed-name')?.textContent).toBe('awards');
  });

  test.each([
    [12, '12 pages', false],
    [1, '1 page', false],
    [0, '0 pages', true],
  ] as const)('childCount %i renders %j', (childCount, text, muted) => {
    const count = meta({ childCount }).querySelector('.cm-media-embed-count') as HTMLElement;
    expect(count.textContent).toBe(text);
    expect(count.classList.contains('cm-media-embed-count--empty')).toBe(muted);
  });

  test('an unknown folder renders NO count element, but keeps its chips', () => {
    // "found and zero" and "unknown" must be different states on screen.
    const dom = meta({ childCount: null });
    expect(dom.querySelector('.cm-media-embed-count')).toBeNull();
    expect(dom.querySelectorAll('.cm-media-embed-chip')).toHaveLength(2);
  });

  test('chips render in the order given, as inert text', () => {
    const chips = [...meta({}).querySelectorAll('.cm-media-embed-chip')];
    expect(chips.map((c) => c.textContent)).toEqual(['style:grid', 'sort:weight']);
    expect(chips[0].querySelector('*')).toBeNull();
  });

  test('a non-folder card grows no meta group', () => {
    expect(mediaWidget({ kind: 'pdf' }).toDOM().querySelector('.cm-media-embed-meta')).toBeNull();
  });
});

function mediaRect(width: number): DOMRect {
  return { width, height: 40, left: 0, top: 0, right: width, bottom: 40, x: 0, y: 0, toJSON() {} } as DOMRect;
}

function followableMedia(over: Record<string, unknown> = {}) {
  return new MediaEmbedBlockWidget({
    kind: 'folder',
    name: 'notes',
    target: 'notes',
    intraLineOffset: 0,
    onSetWidth: () => {},
    resizable: false,
    ...over,
  } as never);
}

/** Run `fn` with `navigator.platform` reporting macOS, then put it back.
 *  `platform` lives on the prototype, so restoring means DELETING the own
 *  property when there was none — a plain re-define leaks 'MacIntel' into
 *  every later test in the file. */
function asMacPlatformMedia(fn: () => void): void {
  const prior = Object.getOwnPropertyDescriptor(navigator, 'platform');
  Object.defineProperty(navigator, 'platform', { value: 'MacIntel', configurable: true });
  try {
    fn();
  } finally {
    if (prior) Object.defineProperty(navigator, 'platform', prior);
    else delete (navigator as unknown as Record<string, unknown>).platform;
  }
}

describe('MediaEmbedBlockWidget — following the card', () => {
  test('two embeds sharing a basename are NOT the same widget', () => {
    // The card renders the basename, so `name` cannot tell these apart — and
    // `onFollow` closes over the target, so a wrong DOM reuse makes the card
    // open the previous file.
    const a = followableMedia({ kind: 'pdf', name: 'paper.pdf', target: 'docs/paper.pdf' });
    const b = followableMedia({ kind: 'pdf', name: 'paper.pdf', target: 'archive/paper.pdf' });
    expect(a.eq(b as never)).toBe(false);
  });

  test('followability is part of identity', () => {
    const inert = followableMedia({ kind: 'folder', name: 'notes' });
    const live = followableMedia({ kind: 'folder', name: 'notes', onFollow: () => {} });
    expect(inert.eq(live as never)).toBe(false);
  });

  test('a followable card is clickable only with the modifier held', () => {
    const follow = vi.fn();
    const dom = followableMedia({ kind: 'folder', name: 'notes', onFollow: follow }).toDOM();
    const card = dom.querySelector('.cm-media-embed-card') as HTMLElement;

    expect(card.classList.contains('cm-link-clickable')).toBe(true);

    card.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(follow).not.toHaveBeenCalled();

    card.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    expect(follow).toHaveBeenCalledTimes(1);

    card.dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true }));
    expect(follow).toHaveBeenCalledTimes(2);
  });

  test('on macOS, Ctrl+click opens the menu instead of following', () => {
    // Ctrl+click IS the secondary click there: WebKit fires contextmenu AND
    // click, so honouring Ctrl would navigate out from under the menu.
    asMacPlatformMedia(() => {
      const follow = vi.fn();
      const dom = followableMedia({ onFollow: follow }).toDOM();
      const card = dom.querySelector('.cm-media-embed-card') as HTMLElement;

      card.dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true }));
      expect(follow).not.toHaveBeenCalled();

      card.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
      expect(follow).toHaveBeenCalledTimes(1);
    });
  });

  test('a card with nothing to open advertises nothing', () => {
    const dom = followableMedia({ kind: 'folder', name: 'notes' }).toDOM();
    const card = dom.querySelector('.cm-media-embed-card') as HTMLElement;
    expect(card.classList.contains('cm-link-clickable')).toBe(false);
    // No listener either: a modifier click must not throw or navigate.
    card.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
  });

  test('a chord-held handle drag is swallowed, not merely declined', () => {
    // Declining is not enough: the click bubbles to the editor-level handler,
    // which maps a widget click to the end of the source line — inside the link
    // for a link-wrapped card — and navigates, losing the width just set.
    const outer = document.createElement('div');
    document.body.appendChild(outer);
    const seen: MouseEvent[] = [];
    outer.addEventListener('click', (e) => seen.push(e as MouseEvent));
    try {
      const dom = followableMedia({ resizable: true, onFollow: () => {} }).toDOM();
      outer.appendChild(dom);
      const handle = dom.querySelector('.cm-lp-image-handle') as HTMLElement;
      handle.dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true }));
      expect(seen).toHaveLength(0);
    } finally {
      outer.remove();
    }
  });

  test('a card owns its right-click even with nothing to open', () => {
    // The native webview menu never leaks onto moss chrome; with no action to
    // offer, owning it means suppressing that menu and showing none.
    const dom = followableMedia({ kind: 'folder', name: 'notes' }).toDOM();
    const card = dom.querySelector('.cm-media-embed-card') as HTMLElement;
    const e = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    card.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(true);
    expect(document.querySelector('.ctx-menu')).toBeNull();
  });

  test('a chord-held drag of the resize handle does not navigate', () => {
    // The handle is a child of the card, and preventDefault on its pointerdown
    // does not suppress the click that arrives on release.
    const follow = vi.fn();
    const dom = followableMedia({ resizable: true, onFollow: follow }).toDOM();
    const card = dom.querySelector('.cm-media-embed-card') as HTMLElement;
    const handle = card.querySelector('.cm-lp-image-handle') as HTMLElement;
    expect(handle).not.toBeNull();

    handle.dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true }));
    expect(follow).not.toHaveBeenCalled();
  });

  test('a player carries no follow affordance, deliberately', () => {
    // A <video> owns its own gestures and its native menu is the only door to
    // playback speed / PiP / save. Following stays available via Mod-Enter on
    // the source line.
    const dom = followableMedia({
      kind: 'video', name: 'clip.mp4', target: 'clip.mp4',
      src: 'moss-source://localhost/clip.mp4', onFollow: () => {},
    }).toDOM();
    expect(dom.querySelector('.cm-media-embed-player')).not.toBeNull();
    expect(dom.querySelector('.cm-link-clickable')).toBeNull();
  });
});

describe('the picture below an embed line is wired to the editor callbacks', () => {
  // The widget's own behaviour is tested in cm-image-block.test.ts against a
  // hand-built widget; these go through the decoration builder, so a dropped
  // option in `blockEmbedDecoration` would show.
  function mount(doc: string, opts: Parameters<typeof blockEmbedsExtension>[0]) {
    const state = EditorState.create({
      doc,
      extensions: [markdown({ extensions: [wikilinkConfig] }), blockEmbedsExtension({ resolveImage: () => ({ url: 'x.png', width: 40, height: 30 }), ...opts })],
    });
    return new EditorView({ state, parent: document.body });
  }

  test('the resize handle commits through onSetWidth, with the embed\'s own range', () => {
    const onSetWidth = vi.fn();
    const doc = '![a](p.png)';
    const view = mount(doc, { onSetWidth });
    const handle = view.dom.querySelector('.cm-lp-image-block .cm-lp-image-handle') as HTMLElement;
    expect(handle).not.toBeNull();
    const cycle = () => {
      handle.dispatchEvent(new MouseEvent('pointerdown', { clientX: 5, bubbles: true, cancelable: true }));
      window.dispatchEvent(new MouseEvent('pointerup', { clientX: 5 }));
    };
    cycle(); cycle(); // a double-click resets the width
    expect(onSetWidth).toHaveBeenCalledWith(0, doc.length, null);
    view.destroy();
  });

  test('no onSetWidth, no handle', () => {
    const view = mount('![a](p.png)', {});
    expect(view.dom.querySelector('.cm-lp-image-block')).not.toBeNull();
    expect(view.dom.querySelector('.cm-lp-image-handle')).toBeNull();
    view.destroy();
  });

  test('Cmd+click on a linked picture follows its link; an unlinked picture follows nothing', () => {
    const navigateLink = vi.fn();
    const linked = mount('[![a](p.png)](/u/)', { navigateLink, canFollowLink: () => true });
    const block = linked.dom.querySelector('.cm-lp-image-block') as HTMLElement;
    block.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    expect(navigateLink).toHaveBeenCalledWith('/u/');
    linked.destroy();

    const plain = mount('![a](p.png)', { navigateLink, canFollowLink: () => true });
    expect(plain.dom.querySelector('.cm-link-clickable')).toBeNull();
    navigateLink.mockClear();
    plain.dom.querySelector('.cm-lp-image-block')!.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    expect(navigateLink).not.toHaveBeenCalled();
    plain.destroy();
  });

  test('a link the app cannot follow leaves the picture a plain one', () => {
    const view = mount('[![a](p.png)](/gone/)', { navigateLink: vi.fn(), canFollowLink: () => false });
    expect(view.dom.querySelector('.cm-link-clickable')).toBeNull();
    view.destroy();
  });
});
