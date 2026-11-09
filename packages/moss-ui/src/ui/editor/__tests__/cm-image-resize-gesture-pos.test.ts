/**
 * Resize commits must target the image at its CURRENT document position,
 * resolved at GESTURE time — not the position captured when the widget DOM
 * was built.
 *
 * The bug class: `eq()` deliberately excludes positions (so an edit above the
 * image doesn't recreate the DOM and flash the <img>), but the old design
 * baked `nodeFrom` into the resize controller's commit closure at toDOM time.
 * After any edit above, CM6 reused the old DOM whose handle committed the OLD
 * offset — `applyEmbedWidth`'s re-resolve then silently no-oped, or, if
 * another Image had shifted under the stale offset, edited the WRONG embed.
 *
 * New contract: widgets capture only the intra-line offset of their Image
 * node (stable under edits above the line); at gesture time they ask the view
 * where the widget is (`view.posAtDOM`) and resolve the Image node on that
 * line, using the intra-line offset to disambiguate multiple images per line.
 *
 * The fake view models the live editor: `state` is read at gesture time (a
 * getter over a mutable binding), `posAtDOM` returns the widget's anchor
 * (line end of the image's source line) as CM6 would.
 */

import { describe, test, expect, vi } from 'vitest';
import { EditorState } from '@codemirror/state';
import type { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { wikilinkConfig } from '@symbiosis-lab/moss-syntax';
import { ImageBlockWidget, MediaEmbedBlockWidget } from '../cm-block-embed-widgets';

/** Minimal EditorView stand-in: live state + widget-anchor posAtDOM. */
function fakeView(getState: () => EditorState, imageLineNumber: number): EditorView {
  return {
    get state() { return getState(); },
    posAtDOM: () => getState().doc.line(imageLineNumber).to,
    requestMeasure: () => {},
  } as unknown as EditorView;
}

function mdState(doc: string): EditorState {
  return EditorState.create({
    doc,
    extensions: [markdown({ extensions: [wikilinkConfig] })],
  });
}

/** Two no-move pointer cycles on the handle = double-click reset. */
function doubleClickReset(dom: HTMLElement) {
  (dom as any).getBoundingClientRect = () => ({
    width: 300, height: 200, left: 0, top: 0, right: 300, bottom: 200, x: 0, y: 0, toJSON() {},
  });
  const handle = dom.querySelector('.cm-lp-image-handle') as HTMLElement;
  const cycle = () => {
    handle.dispatchEvent(new MouseEvent('pointerdown', { clientX: 50, bubbles: true, cancelable: true }));
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: 50 }));
  };
  cycle(); cycle();
}

describe('resize commit resolves position at gesture time', () => {
  test('image widget commits CURRENT coords after an edit above the line', () => {
    // Image on line 2, at offset 6 when the widget is built.
    let state = mdState('intro\n![alt](p.jpg)');
    const image = '![alt](p.jpg)';
    const onSetWidth = vi.fn();
    // intraLineOffset 0: the image starts at its line's start.
    const w = new ImageBlockWidget({ url: 'http://x/p.jpg', alt: 'a', intrinsicWidth: 400, intrinsicHeight: 300, intraLineOffset: 0, width: '55%', onSetWidth });
    const dom = w.toDOM(fakeView(() => state, 2));

    // The doc shifts AFTER the widget DOM exists (edit above the image).
    state = state.update({ changes: { from: 0, insert: 'PREFIX ' } }).state;
    const freshFrom = state.doc.toString().indexOf(image);

    doubleClickReset(dom);

    expect(onSetWidth).toHaveBeenCalledTimes(1);
    expect(onSetWidth).toHaveBeenCalledWith(freshFrom, freshFrom + image.length, null);
  });

  test('media embed widget commits CURRENT coords after an edit above', () => {
    let state = mdState('intro\n![[demo.mp4]]');
    const embed = '![[demo.mp4]]';
    const onSetWidth = vi.fn();
    const w = new MediaEmbedBlockWidget({
      kind: 'video',
      name: 'demo.mp4',
      target: 'demo.mp4',
      intraLineOffset: 0,
      onSetWidth,
      resizable: true,
    });
    const dom = w.toDOM(fakeView(() => state, 2));

    state = state.update({ changes: { from: 0, insert: 'LONG PREFIX ' } }).state;
    const freshFrom = state.doc.toString().indexOf(embed);

    // Drag: content basis 600px, card 300px, +60px → 60%.
    (dom as any).getBoundingClientRect = () => ({
      width: 600, height: 56, left: 0, top: 0, right: 600, bottom: 56, x: 0, y: 0, toJSON() {},
    });
    const card = dom.querySelector('.cm-media-embed-card') as HTMLElement;
    card.getBoundingClientRect = () => ({
      width: 300, height: 56, left: 0, top: 0, right: 300, bottom: 56, x: 0, y: 0, toJSON() {},
    } as DOMRect);
    const handle = dom.querySelector('.cm-lp-image-handle') as HTMLElement;
    handle.dispatchEvent(new MouseEvent('pointerdown', { clientX: 0, bubbles: true, cancelable: true }));
    window.dispatchEvent(new MouseEvent('pointermove', { clientX: 60 }));
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: 60 }));

    expect(onSetWidth).toHaveBeenCalledWith(freshFrom, freshFrom + embed.length, '60%');
  });

  test('two images on one line: intra-line offset picks the right one', () => {
    const first = '![a](1.jpg)';
    const doc = `${first} ![b](2.jpg)`;
    const state = mdState(doc);
    const secondFrom = first.length + 1;
    const onSetWidth = vi.fn();
    // Widget for the SECOND image: intraLineOffset = its offset within the line.
    const w = new ImageBlockWidget({ url: 'http://x/2.jpg', alt: 'b', intrinsicWidth: 400, intrinsicHeight: 300, intraLineOffset: secondFrom, width: '55%', onSetWidth });
    const dom = w.toDOM(fakeView(() => state, 1));

    doubleClickReset(dom);

    expect(onSetWidth).toHaveBeenCalledWith(secondFrom, doc.length, null);
  });
});
