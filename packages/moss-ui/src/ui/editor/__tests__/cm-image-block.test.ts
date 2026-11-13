/**
 * Tests for the inline image block widget (`buildBlockEmbedDecorations` +
 * `ImageBlockWidget`).
 *
 * Placement: the source line stays on screen as text and the picture is a
 * block widget BELOW it — one zero-width `Decoration.widget({ block: true,
 * side: 1 })` anchored at the line END, unconditionally (there is no reveal to
 * gate on, and nothing is replaced).
 */

import { describe, test, expect, vi } from 'vitest';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { wikilinkConfig, shortcodeBlockConfig } from '@symbiosis-lab/moss-syntax';
import { ensureSyntaxTree, syntaxTree } from '@codemirror/language';
import { buildBlockEmbedDecorations } from '../cm-block-embeds';
import { ImageBlockWidget, type ResolveImageFn } from '../cm-block-embed-widgets';
import { clampPercent } from '../cm-resize-controller';
import { IMAGE_HEIGHT_CAP, ROW_PADDING } from '../block-row-height';

const resolveImage = () => ({ url: 'http://x/p.jpg', width: 400, height: 300 });

function makeState(doc: string, selection?: { anchor: number }) {
  return EditorState.create({
    doc,
    selection,
    extensions: [markdown({ extensions: [wikilinkConfig] })],
  });
}

/** buildBlockEmbedDecorations, called the way this file's old buildImageDecorations was. */
function buildImageDecorations(state: EditorState, resolveImg: ResolveImageFn) {
  return buildBlockEmbedDecorations(state, [{ from: 0, to: state.doc.length }], { resolveImage: resolveImg });
}

function decoPositions(doc: string, selection?: { anchor: number }) {
  const state = makeState(doc, selection);
  const set = buildImageDecorations(state, resolveImage);
  const out: Array<{ from: number; to: number; block: boolean }> = [];
  const it = set.iter();
  while (it.value) {
    out.push({ from: it.from, to: it.to, block: (it.value.spec as any).block === true });
    it.next();
  }
  return { state, out };
}

describe('image block placement', () => {
  test('the picture is a block widget after the line end; the source line is not replaced', () => {
    const { state, out } = decoPositions('![alt](p.jpg)', { anchor: 0 });
    const line = state.doc.line(1);
    const blockWidget = out.find((d) => d.block);
    expect(blockWidget).toBeTruthy();
    expect(blockWidget!.from).toBe(line.to);
    expect(blockWidget!.to).toBe(line.to); // zero-width: it covers none of the line's characters
    expect(out.filter((d) => d.to > d.from)).toEqual([]); // nothing replaces any source character
  });

  test('the block widget sits after the line (side 1), so the caret can reach the source line end first', () => {
    const state = makeState('![alt](p.jpg)\ntext');
    const set = buildImageDecorations(state, resolveImage);
    const it = set.iter();
    expect((it.value!.spec as any).side).toBeGreaterThan(0);
    expect(it.value!.startSide).toBeGreaterThan(0);
  });

  test('the widget does not depend on the caret — no such mode exists', () => {
    const { out: caretElsewhere } = decoPositions('![alt](p.jpg)\ntext', { anchor: 16 });
    const { out: caretOnImage } = decoPositions('![alt](p.jpg)\ntext', { anchor: 2 });
    const blockOf = (out: typeof caretElsewhere) => out.find((d) => d.block);
    expect(blockOf(caretElsewhere)).toBeTruthy();
    expect(blockOf(caretOnImage)).toEqual(blockOf(caretElsewhere));
  });
});

describe('image block sizing', () => {
  test('toDOM wraps the block in an inset row (keeps it within the text column)', () => {
    const w = new ImageBlockWidget({ url: 'http://x/p.jpg', alt: 'a', intrinsicWidth: 400, intrinsicHeight: 300, intraLineOffset: 0 });
    const dom = w.toDOM();
    expect(dom.classList.contains('cm-lp-image-row')).toBe(true);
    expect(dom.querySelector(':scope > .cm-lp-image-block')).toBeTruthy();
  });

  test('widget applies percent width to the inner block (of the text column)', () => {
    const w = new ImageBlockWidget({ url: 'http://x/p.jpg', alt: 'a', intrinsicWidth: 400, intrinsicHeight: 300, intraLineOffset: 0, width: '55%' });
    const block = w.toDOM().querySelector('.cm-lp-image-block') as HTMLElement;
    expect(block.style.width).toBe('55%');
  });

  test('eq() distinguishes width (so resize repaints)', () => {
    const a = new ImageBlockWidget({ url: 'u', alt: 'a', intrinsicWidth: 4, intrinsicHeight: 3, intraLineOffset: 0, width: '50%' });
    const b = new ImageBlockWidget({ url: 'u', alt: 'a', intrinsicWidth: 4, intrinsicHeight: 3, intraLineOffset: 0, width: '60%' });
    expect(a.eq(b)).toBe(false);
  });

  test('no explicit width caps at intrinsic px (small images do not upscale)', () => {
    const w = new ImageBlockWidget({ url: 'http://x/p.jpg', alt: 'a', intrinsicWidth: 120, intrinsicHeight: 80, intraLineOffset: 0 });
    const block = w.toDOM().querySelector('.cm-lp-image-block') as HTMLElement;
    expect(block.style.width).toBe('');
    expect(block.style.maxWidth).toBe('120px');
  });

  test('a tall image is narrowed until it fits the height cap', () => {
    // 400x1200 portrait. Uncapped it renders 400 wide and 1200 tall, which
    // fills the pane and pushes the next paragraph off screen. The cap is
    // applied to the WIDTH — 400 tall at 1:3 is 133 wide — so the whole
    // picture is still shown, at the right shape, just smaller. Capping the
    // height of a fixed-width box would crop it (`overflow: hidden`) or sit it
    // in empty bars.
    const block = new ImageBlockWidget({ url: 'http://x/p.jpg', alt: 'a', intrinsicWidth: 400, intrinsicHeight: 1200, intraLineOffset: 0 })
      .toDOM().querySelector('.cm-lp-image-block') as HTMLElement;
    expect(block.style.maxWidth).toBe('133px');
    expect(block.style.maxHeight).toBe('');
  });

  test('the cap clamps an explicit width token too, and stays responsive', () => {
    // A `|100%` portrait asked for the whole column. The percentage survives —
    // so the box still follows a divider drag — and the px cap clamps it.
    const block = new ImageBlockWidget({ url: 'http://x/p.jpg', alt: 'a', intrinsicWidth: 400, intrinsicHeight: 1200, intraLineOffset: 0, width: '100%' })
      .toDOM().querySelector('.cm-lp-image-block') as HTMLElement;
    expect(block.style.width).toBe('100%');
    expect(block.style.maxWidth).toBe('133px');
  });

  test('an image that already fits the cap is left alone', () => {
    // The cap is for pictures that take the page over, not a resize of every
    // image in the document.
    const block = new ImageBlockWidget({ url: 'http://x/p.jpg', alt: 'a', intrinsicWidth: 300, intrinsicHeight: 200, intraLineOffset: 0 })
      .toDOM().querySelector('.cm-lp-image-block') as HTMLElement;
    expect(block.style.maxWidth).toBe('300px');
  });

  test('estimatedHeight tracks the cap, so the estimate matches the measure', () => {
    // Every pixel between estimate and measurement is a scroll jump: CM6 lays
    // the document out from the estimate and swaps in the measurement when the
    // block scrolls in.
    const w = new ImageBlockWidget({ url: 'http://x/p.jpg', alt: 'a', intrinsicWidth: 400, intrinsicHeight: 1200, intraLineOffset: 0 });
    expect(w.estimatedHeight).toBe(ROW_PADDING + IMAGE_HEIGHT_CAP);
  });

  test('width reaches the widget from a |55% source token', () => {
    const state = makeState('![alt|55%](p.jpg)');
    const set = buildImageDecorations(state, resolveImage);
    const it = set.iter();
    let widget: ImageBlockWidget | undefined;
    while (it.value) {
      const w = (it.value.spec as any).widget;
      if (w instanceof ImageBlockWidget) widget = w;
      it.next();
    }
    expect(widget).toBeTruthy();
    expect((widget!.toDOM().querySelector('.cm-lp-image-block') as HTMLElement).style.width).toBe('55%');
  });
});

// ── A link wrapping an embed renders as one clickable image ──

describe('linked image block', () => {
  function linked(href = '/u', onNavigate?: (h: string) => void): HTMLElement {
    const w = new ImageBlockWidget({
      url: 'http://x/p.jpg', alt: 'a', intrinsicWidth: 4, intrinsicHeight: 3,
      intraLineOffset: 0, link: { href }, onNavigate,
    });
    return w.toDOM().querySelector('.cm-lp-image-block') as HTMLElement;
  }

  test('carries the link affordance classes, and no tooltip repeating the address on its source line', () => {
    const block = linked('/awards/');
    expect(block.classList.contains('cm-lp-image-block--linked')).toBe(true);
    expect(block.classList.contains('cm-link-clickable')).toBe(true);
    expect(block.getAttribute('data-tooltip')).toBeNull();
    expect(block.getAttribute('title')).toBeNull();
  });

  test('Cmd+click navigates; a plain click does not', () => {
    const onNavigate = vi.fn();
    const block = linked('/awards/', onNavigate);
    block.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(onNavigate).not.toHaveBeenCalled();
    block.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    expect(onNavigate).toHaveBeenCalledWith('/awards/');
  });

  test('eq() distinguishes the href (so editing the url repaints)', () => {
    const mk = (href?: string) => new ImageBlockWidget({
      url: 'u', alt: 'a', intrinsicWidth: 4, intrinsicHeight: 3, intraLineOffset: 0,
      link: href ? { href } : undefined,
    });
    expect(mk('/a').eq(mk('/b'))).toBe(false);
    expect(mk('/a').eq(mk(undefined))).toBe(false);
    expect(mk('/a').eq(mk('/a'))).toBe(true);
  });
});

// ── drag-to-resize handle + double-click reset ───────────────

describe('clampPercent (drag math)', () => {
  test('rounds (startW + dx) / contentW to an integer percent', () => {
    // 300 / 600 = 50%
    expect(clampPercent(300, 0, 600)).toBe(50);
    // (300 + 60) / 600 = 60%
    expect(clampPercent(300, 60, 600)).toBe(60);
    // rounding: 301/600 = 50.16% → 50
    expect(clampPercent(301, 0, 600)).toBe(50);
    // rounding: 303/600 = 50.5% → 51 (Math.round half-up)
    expect(clampPercent(303, 0, 600)).toBe(51);
  });

  test('clamps to the [10, 100] range', () => {
    // way over → 100
    expect(clampPercent(600, 400, 600)).toBe(100);
    // exactly the column → 100
    expect(clampPercent(600, 0, 600)).toBe(100);
    // way under → 10 floor
    expect(clampPercent(10, -100, 600)).toBe(10);
    // small but above floor stays exact
    expect(clampPercent(90, 0, 600)).toBe(15);
  });
});

describe('resize handle', () => {
  function widgetWith(onSetWidth?: (from: number, to: number, w: string | null) => void) {
    return new ImageBlockWidget({
      url: 'http://x/p.jpg', alt: 'a', intrinsicWidth: 400, intrinsicHeight: 300,
      intraLineOffset: 0, width: '55%', onSetWidth,
    });
  }

  test('renders a corner-bracket .cm-lp-image-handle when onSetWidth is provided', () => {
    const dom = widgetWith(() => {}).toDOM();
    const handle = dom.querySelector('.cm-lp-image-handle');
    expect(handle).toBeTruthy();
    expect(handle!.querySelector('svg')).toBeTruthy();
    expect(handle!.getAttribute('aria-label')).toBe('Resize image');
  });

  test('omits the handle when no onSetWidth callback (read-only)', () => {
    const dom = widgetWith(undefined).toDOM();
    expect(dom.querySelector('.cm-lp-image-handle')).toBeNull();
  });

  test('double-click (two no-move pointer cycles) resets via onSetWidth(null)', () => {
    // Commit coords resolve at gesture time from the live syntax tree (see
    // cm-image-resize-gesture-pos.test.ts for the stale-position regression).
    const doc = '![a|55%](p.jpg)';
    const state = makeState(doc);
    const fakeView = {
      state,
      posAtDOM: () => state.doc.line(1).to,
      requestMeasure: () => {},
    } as any;
    const onSetWidth = vi.fn();
    const dom = widgetWith(onSetWidth).toDOM(fakeView);
    (dom as any).getBoundingClientRect = () => ({ width: 300, height: 200, left: 0, top: 0, right: 300, bottom: 200, x: 0, y: 0, toJSON() {} });
    const handle = dom.querySelector('.cm-lp-image-handle') as HTMLElement;
    const cycle = () => {
      handle.dispatchEvent(new MouseEvent('pointerdown', { clientX: 50, bubbles: true, cancelable: true }));
      window.dispatchEvent(new MouseEvent('pointerup', { clientX: 50 }));
    };
    cycle(); cycle();
    expect(onSetWidth).toHaveBeenCalledTimes(1);
    expect(onSetWidth).toHaveBeenCalledWith(0, doc.length, null);
  });
});

/**
 * Inside a `:::` fence the body is STRUCTURE, not reading matter: an embed's
 * source line stays as text and nothing is drawn for it — no token, no picture
 * below — because a picture under each cell would turn a six-cell grid into six
 * full-column photographs. The preview pane shows the grid in its real layout.
 * Browser measurements of media density verify the resulting height.
 */
describe('an embed inside a `:::` fence is its source line only', () => {
  function fenceDecos(doc: string, selection?: { anchor: number }) {
    let state = EditorState.create({
      doc, selection,
      extensions: [markdown({ extensions: [wikilinkConfig, shortcodeBlockConfig] })],
    });
    // The 20ms/3000-char initial parse budget can hand back a partial tree with
    // no view attached; without the full tree there is no fence to be inside.
    for (let i = 0; i < 20 && syntaxTree(state).length < state.doc.length; i++) {
      ensureSyntaxTree(state, state.doc.length, 1e9);
      state = state.update({}).state;
    }
    const out: Array<{ from: number; to: number; block: boolean; widget: unknown }> = [];
    const it = buildImageDecorations(state, resolveImage).iter();
    while (it.value) {
      const spec = it.value.spec as { block?: boolean; widget?: unknown };
      out.push({ from: it.from, to: it.to, block: spec.block === true, widget: spec.widget });
      it.next();
    }
    return out;
  }

  test('draws nothing: no token over the source, no picture below it', () => {
    expect(fenceDecos(':::grid\n![alt](p.jpg)\n:::\n', { anchor: 0 })).toEqual([]);
  });

  test('every cell of a grid is source only', () => {
    expect(fenceDecos(':::grid\n![[a.jpg]]\n+++\n![[b.jpg]]\n+++\n![[c.jpg]]\n:::\n')).toEqual([]);
  });

  test('a nested block is inside its parent, so its images are source only too', () => {
    expect(fenceDecos(':::grid\n::::buttons\n![alt](p.jpg)\n::::\n:::\n', { anchor: 0 })).toEqual([]);
  });

  test('the same image outside a fence still renders as a block below its line', () => {
    const out = fenceDecos('![alt](p.jpg)\ntext\n', { anchor: 15 });
    expect(out.filter((d) => d.block)).toHaveLength(1);
  });
});

describe('image block rungs', () => {
  const opts = { url: 'http://x/p.jpg', alt: '', intrinsicWidth: 4000, intrinsicHeight: 3000, intraLineOffset: 0 };
  const srcset = 'http://x/p.jpg?w=800 800w, http://x/p.jpg?w=1600 1600w';

  test('the browser chooses among the built rungs for the width the picture is drawn at', () => {
    const img = new ImageBlockWidget({ ...opts, srcset, width: '50%' }).toDOM().querySelector('img')!;
    expect(img.getAttribute('srcset')).toBe(srcset);
    // Half the column, capped so the picture stays within the height cap.
    expect(img.getAttribute('sizes')).toBe(`${Math.round(Math.min(544 / 2, IMAGE_HEIGHT_CAP * 4 / 3))}px`);
    expect(img.getAttribute('src')).toBe('http://x/p.jpg');
  });

  test('no srcset without both dimensions, since the drawn width is then unknown', () => {
    const img = new ImageBlockWidget({ ...opts, srcset, intrinsicHeight: undefined }).toDOM().querySelector('img')!;
    expect(img.hasAttribute('srcset')).toBe(false);
    expect(img.hasAttribute('sizes')).toBe(false);
  });

  test('a rung landing changes the widget identity, so the <img> is rebuilt', () => {
    expect(new ImageBlockWidget(opts).eq(new ImageBlockWidget({ ...opts, srcset }))).toBe(false);
    expect(new ImageBlockWidget({ ...opts, srcset }).eq(new ImageBlockWidget({ ...opts, srcset }))).toBe(true);
  });

  test('the resolved srcset reaches the widget', () => {
    const state = makeState('![a](p.jpg)');
    const set = buildImageDecorations(state, () => ({ url: 'http://x/p.jpg', srcset, width: 4000, height: 3000 }));
    const widget = set.iter().value!.spec.widget as ImageBlockWidget;
    expect(widget.toDOM().querySelector('img')!.getAttribute('srcset')).toBe(srcset);
  });
});
