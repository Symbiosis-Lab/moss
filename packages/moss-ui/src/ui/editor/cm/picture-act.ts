/**
 * The preview menu's picture rows, done on the picture reference the row
 * names: the `picture`th on its line. The row has already put the caret on
 * that line; a line that no longer holds that reference gets nothing more.
 */

import { EditorSelection, type EditorState, type Extension } from '@codemirror/state';
import { ViewPlugin, type EditorView } from '@codemirror/view';
import { extractImageTargets, type ImageTarget } from '@symbiosis-lab/moss-syntax/cm6';
import type { Bus } from '../../host';
import { EDITOR_PICTURE_ACT, type PictureAct, type PictureActRequest } from '../session/editor-position';
import { focusDotEffect } from './focus-point';
import { isCaptionSegment } from './image-position';

export interface PictureActOptions {
  host: Bus;
  /** Replace…, as the reference card does it. */
  replace(view: EditorView, picture: ImageTarget): Promise<void>;
  /** Show the file a reference resolves to in the file list. */
  reveal(reference: string): void;
}

/** The `picture`th image reference on `line`, in document order. */
export function pictureOnLine(state: EditorState, line: number, picture: number): ImageTarget | null {
  if (line < 1 || line > state.doc.lines) return null;
  const { from, to } = state.doc.line(line);
  const onLine = extractImageTargets(state).filter((t) => t.nodeFrom >= from && t.nodeFrom <= to);
  return onLine.sort((a, b) => a.nodeFrom - b.nodeFrom)[picture] ?? null;
}

/**
 * Where a picture's description goes, found as moss reads the pipe text: its
 * first caption segment, which is never a width, placement or frame word. With
 * none, `pipe` is where a `|` is written first (selection offsets are in the
 * document after it) and the caret sits where the words begin, so the width and frame words stay as they were.
 */
export function descriptionSpan(state: EditorState, picture: ImageTarget): { from: number; to: number; pipe?: number } {
  const wiki = state.sliceDoc(picture.nodeFrom, picture.nodeFrom + 3) === '![[';
  const end = picture.nodeTo - (wiki ? 2 : 0);
  const pipe = wiki ? state.sliceDoc(picture.to, end).indexOf('|') : -1;
  if (wiki && pipe < 0) return { from: picture.to + 1, to: picture.to + 1, pipe: picture.to };
  // A markdown image's text is everything between `![` and `](`; its own first
  // segment is the caption's place even when empty.
  const start = wiki ? picture.to + pipe + 1 : picture.nodeFrom + 2;
  const text = state.sliceDoc(start, wiki ? end : picture.from - 2);
  let at = start;
  for (const segment of text.split('|')) {
    if (isCaptionSegment(segment)) {
      const lead = segment.length - segment.trimStart().length;
      return { from: at + lead, to: at + lead + segment.trim().length };
    }
    at += segment.length + 1;
  }
  return { from: start, to: start, pipe: start };
}

export function pictureActExtension(opts: PictureActOptions): Extension {
  const run = (view: EditorView, line: number, act: PictureAct, index: number) => {
    const picture = pictureOnLine(view.state, line, index);
    if (!picture) return;
    switch (act) {
      case 'replace':
        void opts.replace(view, picture);
        return;
      case 'focus':
        // The focus instrument works on a line's first picture (a hero's, on its fence).
        if (index === 0) view.dispatch({ effects: focusDotEffect.of(line) });
        return;
      case 'description': {
        if (picture.attr) return;
        const span = descriptionSpan(view.state, picture);
        view.dispatch({
          changes: span.pipe === undefined ? undefined : { from: span.pipe, insert: '|' },
          selection: EditorSelection.range(span.from, span.to),
          scrollIntoView: true,
        });
        view.focus();
        return;
      }
      case 'reveal':
        opts.reveal(picture.target);
        return;
    }
  };
  return ViewPlugin.define((view) => {
    const off = opts.host.on<{ line: number } & PictureActRequest>(EDITOR_PICTURE_ACT, (p) => run(view, p.line, p.act, p.picture));
    return { destroy: off };
  });
}
