/**
 * Tests for the playable video/audio widget in `MediaEmbedBlockWidget`
 * (cm-media-embed.ts) — verifies that video/audio kinds with a resolved src
 * render a native <video>/<audio> element instead of the placeholder card, and
 * that the resize handle, eq(), and perf-critical no-rebuild path all work.
 *
 * Also tests that the `ResolveEmbedFn` type extension (carrying `src`) reaches
 * the MediaEmbedBlockWidget via buildImageDecorations.
 */

import { describe, test, expect, vi } from 'vitest';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { wikilinkConfig } from '@symbiosis-lab/moss-syntax';
import {
  buildBlockEmbedDecorations,
  type ResolveEmbedFn, type NavigateLinkFn, type CanFollowLinkFn,
} from '../cm-block-embeds';
import { MediaEmbedBlockWidget, type ResolveImageFn, type SetImageWidthFn } from '../cm-block-embed-widgets';
import { mediaRowHeight, editorColumnWidth } from '../block-row-height';

/** buildBlockEmbedDecorations, called with the same positional signature as
 *  this file's buildImageDecorations. */
function buildImageDecorations(
  state: EditorState,
  resolveImage?: ResolveImageFn,
  onSetWidth?: SetImageWidthFn,
  resolveEmbed?: ResolveEmbedFn,
  navigateLink?: NavigateLinkFn,
  canFollowLink?: CanFollowLinkFn,
) {
  return buildBlockEmbedDecorations(state, [{ from: 0, to: state.doc.length }], {
    resolveImage, onSetWidth, resolveEmbed, navigateLink, canFollowLink,
  });
}

// ── Helper ───────────────────────────────────────────────────────────────────

function videoWidget(over: {
  kind?: string;
  src?: string;
  width?: string;
  resizable?: boolean;
} = {}) {
  return new MediaEmbedBlockWidget({
    kind: over.kind ?? 'video',
    name: 'test.mp4',
    target: 'test.mp4',
    intraLineOffset: 0,
    onSetWidth: () => {},
    resizable: over.resizable ?? true,
    src: over.src,
    width: over.width,
  });
}

function audioWidget(over: { src?: string } = {}) {
  return new MediaEmbedBlockWidget({
    kind: 'audio',
    name: 'test.mp3',
    target: 'test.mp3',
    intraLineOffset: 0,
    onSetWidth: () => {},
    resizable: true,
    src: over.src,
  });
}

function rect(width: number, height = 200): DOMRect {
  return {
    width, height, left: 0, top: 0, right: width, bottom: height, x: 0, y: 0,
    toJSON() {},
  } as DOMRect;
}

// ── video kind with src → native <video controls> ────────────────────────────

describe('MediaEmbedBlockWidget — native video player', () => {
  test('video + src renders a <video controls> element, not the placeholder card', () => {
    const dom = videoWidget({ src: 'moss-source://localhost/test.mp4' }).toDOM();
    const video = dom.querySelector('video');
    expect(video).toBeTruthy();
    expect(video!.controls).toBe(true);
    expect(video!.src).toContain('test.mp4');
    // No placeholder card
    expect(dom.querySelector('.cm-media-embed-card')).toBeNull();
  });

  test('video + src preload is "metadata" (lazy load — no eager fetch)', () => {
    const dom = videoWidget({ src: 'moss-source://localhost/clip.mp4' }).toDOM();
    const video = dom.querySelector('video') as HTMLVideoElement;
    expect(video.preload).toBe('metadata');
  });

  test('video + src wraps in .cm-media-embed-player (not .cm-media-embed-card)', () => {
    const dom = videoWidget({ src: 'moss-source://localhost/v.mp4' }).toDOM();
    expect(dom.querySelector('.cm-media-embed-player')).toBeTruthy();
    expect(dom.querySelector('.cm-media-embed-card')).toBeNull();
  });

  test('video WITHOUT src falls back to the placeholder card', () => {
    const dom = videoWidget({ src: undefined }).toDOM();
    expect(dom.querySelector('.cm-media-embed-card')).toBeTruthy();
    expect(dom.querySelector('video')).toBeNull();
  });

  test('outer wrapper is .cm-lp-image-row (text column inset)', () => {
    const dom = videoWidget({ src: 'moss-source://localhost/v.mp4' }).toDOM();
    expect(dom.classList.contains('cm-lp-image-row')).toBe(true);
  });

  test('explicit |NN% width applies to the player wrapper', () => {
    const dom = videoWidget({ src: 'moss-source://localhost/v.mp4', width: '60%' }).toDOM();
    const wrapper = dom.querySelector('.cm-media-embed-player') as HTMLElement;
    expect(wrapper.style.width).toBe('60%');
  });
});

// ── audio kind with src → native <audio controls> ───────────────────────────

describe('MediaEmbedBlockWidget — native audio player', () => {
  test('audio + src renders a <audio controls> element', () => {
    const dom = new MediaEmbedBlockWidget({
      kind: 'audio',
      name: 'track.mp3',
      target: 'track.mp3',
      intraLineOffset: 0,
      onSetWidth: () => {},
      resizable: true,
      src: 'moss-source://localhost/track.mp3',
    }).toDOM();
    const audio = dom.querySelector('audio');
    expect(audio).toBeTruthy();
    expect(audio!.controls).toBe(true);
    expect(dom.querySelector('.cm-media-embed-card')).toBeNull();
  });

  test('audio WITHOUT src falls back to the placeholder card', () => {
    const dom = new MediaEmbedBlockWidget({
      kind: 'audio',
      name: 'track.mp3',
      target: 'track.mp3',
      intraLineOffset: 0,
      onSetWidth: () => {},
      resizable: true,
      src: undefined,
    }).toDOM();
    expect(dom.querySelector('.cm-media-embed-card')).toBeTruthy();
    expect(dom.querySelector('audio')).toBeNull();
  });
});

// ── Resize handle on native player ───────────────────────────────────────────

describe('MediaEmbedBlockWidget — resize handle on native player', () => {
  test('video player with resizable:true has a .cm-lp-image-handle', () => {
    const dom = videoWidget({ src: 'moss-source://localhost/v.mp4', resizable: true }).toDOM();
    const handle = dom.querySelector('.cm-lp-image-handle');
    expect(handle).toBeTruthy();
    expect(handle!.querySelector('svg')).toBeTruthy();
  });

  test('video player with resizable:false omits the handle', () => {
    const dom = videoWidget({ src: 'moss-source://localhost/v.mp4', resizable: false }).toDOM();
    expect(dom.querySelector('.cm-lp-image-handle')).toBeNull();
  });

  test('drag on video handle commits a percent width', () => {
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
    const w = new MediaEmbedBlockWidget({
      kind: 'video',
      name: 'demo.mp4',
      target: 'demo.mp4',
      intraLineOffset: 0,
      onSetWidth,
      resizable: true,
      src: 'moss-source://localhost/demo.mp4',
    });
    const dom = w.toDOM(fakeView);
    // content basis 600px; drag from 300px start by 60px right
    (dom as any).getBoundingClientRect = () => rect(600);
    const wrapper = dom.querySelector('.cm-media-embed-player') as HTMLElement;
    wrapper.getBoundingClientRect = () => rect(300);
    const handle = dom.querySelector('.cm-lp-image-handle') as HTMLElement;
    handle.dispatchEvent(new MouseEvent('pointerdown', { clientX: 0, bubbles: true, cancelable: true }));
    window.dispatchEvent(new MouseEvent('pointermove', { clientX: 60 }));
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: 60 }));
    // (300 + 60) / 600 = 60%
    expect(onSetWidth).toHaveBeenCalledWith(0, embed.length, '60%');
  });
});

// ── eq() identity — CRITICAL for no-rebuild during playback ──────────────────

describe('MediaEmbedBlockWidget — eq() with src', () => {
  test('same src/kind/width → eq (no teardown mid-playback)', () => {
    const a = videoWidget({ src: 'moss-source://localhost/v.mp4' });
    const b = videoWidget({ src: 'moss-source://localhost/v.mp4' });
    expect(a.eq(b)).toBe(true);
  });

  test('different src → not eq (player rebuilds when URL changes)', () => {
    const a = videoWidget({ src: 'moss-source://localhost/v1.mp4' });
    const b = videoWidget({ src: 'moss-source://localhost/v2.mp4' });
    expect(a.eq(b)).toBe(false);
  });

  test('src present vs absent → not eq', () => {
    const a = videoWidget({ src: 'moss-source://localhost/v.mp4' });
    const b = videoWidget({ src: undefined });
    expect(a.eq(b)).toBe(false);
  });

  test('width difference → not eq (resize must repaint)', () => {
    const a = videoWidget({ src: 'moss-source://localhost/v.mp4', width: '50%' });
    const b = videoWidget({ src: 'moss-source://localhost/v.mp4', width: '60%' });
    expect(a.eq(b)).toBe(false);
  });

  test('kind difference → not eq', () => {
    const a = videoWidget({ kind: 'video', src: 'moss-source://localhost/v.mp4' });
    const b = videoWidget({ kind: 'audio', src: 'moss-source://localhost/v.mp4' });
    expect(a.eq(b)).toBe(false);
  });
});

// ── buildImageDecorations passes src through to widget ───────────────────────

describe('buildImageDecorations — src forwarded to video widget', () => {
  test('resolveEmbed with src produces a widget carrying that src', () => {
    const state = EditorState.create({
      doc: '![[clip.mp4]]',
      extensions: [markdown({ extensions: [wikilinkConfig] })],
    });
    const resolveEmbed = () => ({
      kind: 'video' as const,
      src: 'moss-source://localhost/clip.mp4',
    });
    const set = buildImageDecorations(state, undefined, undefined, resolveEmbed);
    let found: MediaEmbedBlockWidget | undefined;
    const it = set.iter();
    while (it.value) {
      const w = (it.value.spec as any).widget;
      if (w instanceof MediaEmbedBlockWidget) found = w;
      it.next();
    }
    expect(found).toBeTruthy();
    // The widget must produce a <video> element (src was forwarded)
    const dom = found!.toDOM();
    expect(dom.querySelector('video')).toBeTruthy();
    expect(dom.querySelector('.cm-media-embed-card')).toBeNull();
  });

  test('resolveEmbed without src falls back to placeholder', () => {
    const state = EditorState.create({
      doc: '![[clip.mp4]]',
      extensions: [markdown({ extensions: [wikilinkConfig] })],
    });
    // kind=video but no src (e.g. asset not yet in registry)
    const resolveEmbed = () => ({ kind: 'video' as const });
    const set = buildImageDecorations(state, undefined, undefined, resolveEmbed);
    let found: MediaEmbedBlockWidget | undefined;
    const it = set.iter();
    while (it.value) {
      const w = (it.value.spec as any).widget;
      if (w instanceof MediaEmbedBlockWidget) found = w;
      it.next();
    }
    expect(found).toBeTruthy();
    const dom = found!.toDOM();
    expect(dom.querySelector('.cm-media-embed-card')).toBeTruthy();
    expect(dom.querySelector('video')).toBeNull();
  });
});

// ── estimatedHeight ──────────────────────────────────────────────────────────

describe('MediaEmbedBlockWidget — estimatedHeight', () => {
  test('video with src has a larger estimatedHeight than the placeholder card', () => {
    const player = videoWidget({ src: 'moss-source://localhost/v.mp4' });
    const card = videoWidget({ src: undefined });
    expect(player.estimatedHeight).toBeGreaterThan(card.estimatedHeight);
  });

  // CM6 asks for the height of the widget ROOT — `.cm-lp-image-row`, which
  // carries the block's vertical spacing as padding. block-row-height.ts owns
  // that arithmetic for both widgets; the widget's only job is picking the
  // right shape, so that is what this asserts.
  test('the widget maps each render branch to its own row shape', () => {
    expect(videoWidget({ src: 'moss-source://localhost/v.mp4' }).estimatedHeight)
      .toBe(mediaRowHeight('video', { src: 'moss-source://localhost/v.mp4' }));
    expect(videoWidget({ src: undefined }).estimatedHeight).toBe(mediaRowHeight('card'));
    // A src'd audio embed renders a bare control bar, NOT the 150px video box —
    // conflating the two cost ~126px of phantom height per audio embed.
    expect(audioWidget({ src: 'moss-source://localhost/a.mp3' }).estimatedHeight)
      .toBe(mediaRowHeight('audio'));
    // No src → the placeholder card, whatever the kind claims to be.
    expect(audioWidget({ src: undefined }).estimatedHeight).toBe(mediaRowHeight('card'));
  });

  // A <video preload="metadata"> has no intrinsic size, so it renders the CSS
  // default 300x150 box; when metadata lands it takes its real ratio and the
  // row can more than triple (9:16 portrait at a 300px player → 541px). The
  // widget must feed that ratio back so every later layout estimates correctly
  // — otherwise a flat 150 is off by up to -383px per embed.
  test('a video records its real aspect ratio once metadata arrives', () => {
    const src = 'moss-source://localhost/portrait.mp4';
    const before = videoWidget({ src }).estimatedHeight;

    const dom = videoWidget({ src }).toDOM();
    const video = dom.querySelector('video') as HTMLVideoElement;
    Object.defineProperty(video, 'videoWidth', { value: 360, configurable: true });
    Object.defineProperty(video, 'videoHeight', { value: 640, configurable: true });
    video.dispatchEvent(new Event('loadedmetadata'));

    const after = videoWidget({ src }).estimatedHeight;
    expect(before).toBe(mediaRowHeight('video', { src: 'moss-source://never-seen.mp4' }));
    expect(after).toBeGreaterThan(before);
    expect(after).toBe(mediaRowHeight('video', { src, columnWidth: editorColumnWidth() }));
  });
});

// ── A linked card follows the LINK, not the embed inside it ─────────────────

describe('buildImageDecorations — a linked card follows its link', () => {
  function cardIn(
    doc: string,
    navigateLink?: (href: string) => boolean,
    embedFollow = () => {},
    canFollowLink: (href: string) => boolean = () => true,
  ) {
    const state = EditorState.create({
      doc,
      extensions: [markdown({ extensions: [wikilinkConfig] })],
    });
    const resolveEmbed = () => ({
      kind: 'pdf' as const,
      follow: embedFollow,
    });
    const set = buildImageDecorations(
      state, undefined, undefined, resolveEmbed, navigateLink, canFollowLink,
    );
    let found: MediaEmbedBlockWidget | undefined;
    const it = set.iter();
    while (it.value) {
      const w = (it.value.spec as { widget?: unknown }).widget;
      if (w instanceof MediaEmbedBlockWidget) found = w;
      it.next();
    }
    return found;
  }

  test('the card wrapped in a link opens the href', () => {
    // `[![[report.pdf]]](/reports/)` renders as ONE card. Cmd+click on it must
    // agree with Mod-Enter on the source line (cm-link-nav's followTargetAt),
    // which resolves the enclosing link.
    const hrefs: string[] = [];
    const card = cardIn('[![[report.pdf]]](/reports/)\n', (h) => { hrefs.push(h); return true; });
    expect(card).toBeTruthy();
    const dom = card!.toDOM();
    const el = dom.querySelector('.cm-media-embed-card') as HTMLElement;
    el.dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true }));
    expect(hrefs).toEqual(['/reports/']);
  });

  test('the link wins over the embed’s own follow', () => {
    const embedFollow = vi.fn();
    const hrefs: string[] = [];
    const card = cardIn('[![[report.pdf]]](/reports/)\n', (h) => { hrefs.push(h); return true; }, embedFollow);
    const el = card!.toDOM().querySelector('.cm-media-embed-card') as HTMLElement;
    el.dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true }));
    expect(embedFollow).not.toHaveBeenCalled();
    expect(hrefs).toEqual(['/reports/']);
  });

  test('an unwrapped card still opens the embed itself', () => {
    const embedFollow = vi.fn();
    const card = cardIn('![[report.pdf]]\n', () => true, embedFollow);
    const el = card!.toDOM().querySelector('.cm-media-embed-card') as HTMLElement;
    el.dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true }));
    expect(embedFollow).toHaveBeenCalledTimes(1);
  });

  test('a card wrapped in a link that goes nowhere advertises nothing', () => {
    // External URL, bare anchor, page not yet written — the href resolves to no
    // source file, so there is nothing to open and the card must say so by
    // showing no hand and offering no menu entry.
    const card = cardIn('[![[report.pdf]]](https://example.com/)\n', () => false, () => {}, () => false);
    const el = card!.toDOM().querySelector('.cm-media-embed-card') as HTMLElement;
    expect(el.classList.contains('cm-link-clickable')).toBe(false);
  });

  test('editing only the URL repaints the card', () => {
    // Every other compared field is identical across this edit; without the
    // href in eq() CM6 reuses the DOM and the card keeps opening /reports/.
    const a = cardIn('[![[report.pdf]]](/reports/)\n', () => true);
    const b = cardIn('[![[report.pdf]]](/archive/)\n', () => true);
    expect(a!.eq(b as never)).toBe(false);
  });
});

// ── The image block obeys the same two rules as the card ───────────────────

describe('buildImageDecorations — a linked IMAGE follows the same rules', () => {
  function imageIn(doc: string, canFollowLink: (href: string) => boolean, nav: string[]) {
    const state = EditorState.create({
      doc,
      extensions: [markdown({ extensions: [wikilinkConfig] })],
    });
    const resolveImage = () => ({ url: 'moss-source://localhost/cover.png' });
    const set = buildImageDecorations(
      state, resolveImage, undefined, undefined,
      (h) => { nav.push(h); return true; }, canFollowLink,
    );
    let dom: HTMLElement | undefined;
    const it = set.iter();
    while (it.value) {
      const w = (it.value.spec as { widget?: { toDOM?: () => HTMLElement } }).widget;
      if (w?.toDOM) dom = w.toDOM();
      it.next();
    }
    return dom;
  }

  test('a link with nowhere to go paints no hand on the image', () => {
    const dom = imageIn('[![cover](cover.png)](https://example.com/)\n', () => false, []);
    expect(dom?.querySelector('.cm-link-clickable')).toBeNull();
  });

  test('a resolvable link keeps the affordance and follows it', () => {
    const nav: string[] = [];
    const dom = imageIn('[![cover](cover.png)](/about/)\n', () => true, nav);
    const el = dom?.querySelector('.cm-link-clickable') as HTMLElement;
    expect(el).toBeTruthy();
    el.dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true }));
    expect(nav).toEqual(['/about/']);
  });

  test('on macOS the image ignores Ctrl+click, which is the secondary click', () => {
    const prior = Object.getOwnPropertyDescriptor(navigator, 'platform');
    Object.defineProperty(navigator, 'platform', { value: 'MacIntel', configurable: true });
    try {
      const nav: string[] = [];
      const dom = imageIn('[![cover](cover.png)](/about/)\n', () => true, nav);
      const el = dom?.querySelector('.cm-link-clickable') as HTMLElement;
      el.dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true }));
      expect(nav).toEqual([]);
      el.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
      expect(nav).toEqual(['/about/']);
    } finally {
      if (prior) Object.defineProperty(navigator, 'platform', prior);
      else delete (navigator as unknown as Record<string, unknown>).platform;
    }
  });
});
