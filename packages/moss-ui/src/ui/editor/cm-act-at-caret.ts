// packages/moss-ui/src/ui/editor/cm-act-at-caret.ts
//
// Acting on the thing at the caret or under the pointer: a press on a task's
// `[ ]` toggles it, a press on a rendered block (picture, card, player) selects
// its source line, ⌘⏎ toggles a task or opens a comment, ⌘K opens the card of the
// link or picture at the caret. Every construct's characters are on screen, so
// each of these finds its construct by position in the syntax tree; there is no
// widget to step over or select first.

import { EditorView, keymap } from '@codemirror/view';
import { Prec, type EditorState, type Extension } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';
import { blockEmbedOnLine, embedAt } from './cm-block-embeds';
import { commentParsedField, openCommentPopover } from './cm-comment';
import { openCardAt } from './cm-reference-card';
import { referenceSiteAt } from './cm-link-nav';

// A press on a task's `[ ]` toggles it; a click on a rendered block (picture,
// card, player) selects the embed's own source line, which is on screen just
// above it. The block widgets' `ignoreEvent()` returns false, so their resize
// handle (class `cm-lp-image-handle`, `createResizeController`'s default
// `classPrefix`) is excluded here explicitly rather than by the widget
// swallowing the event first.
const mousePress = EditorView.domEventHandlers({
  mousedown(event, view) {
    const target = event.target as HTMLElement;
    // A press on a task's `[ ]` toggles it and leaves the caret where it was:
    // returning true stops the browser placing one there first.
    if (target.closest('.cm-task-toggle')) {
      if (event.button !== 0) return false;
      const task = taskAt(view.state, view.posAtDOM(target));
      return !!task && toggleTask(view, task);
    }
    if (target.closest('.cm-lp-image-handle')) return false;
    const row = target.closest('.cm-lp-image-row');
    if (row) {
      const block = blockEmbedOnLine(view.state, view.posAtDOM(row));
      if (!block) return false;
      view.dispatch({ selection: { anchor: block.line.from, head: block.line.to } });
      event.preventDefault();
      return true;
    }
    return false;
  },
});

/** A GFM task `- [ ] text` at `pos` (its marker or any of its text; either edge counts): where its `[` is and whether it is done. */
export function taskAt(state: EditorState, pos: number): { markerFrom: number; checked: boolean } | null {
  for (const side of [1, -1] as const) {
    for (let node: { name: string; parent: unknown; cursor(): { firstChild(): boolean; name: string; from: number; to: number } } | null = syntaxTree(state).resolveInner(pos, side); node; node = node.parent as typeof node) {
      if (node.name !== 'Task') continue;
      const cur = node.cursor();
      if (!cur.firstChild() || cur.name !== 'TaskMarker') return null;
      const marker = state.doc.sliceString(cur.from, cur.to);
      return { markerFrom: cur.from, checked: marker === '[x]' || marker === '[X]' };
    }
  }
  return null;
}

/** Flip the one character between the brackets, in one transaction; false when the editor is read-only. The document keeps its length, so a caret anywhere stays exactly where it was. */
function toggleTask(view: EditorView, task: { markerFrom: number; checked: boolean }): boolean {
  if (view.state.readOnly) return false; // a sealed editor is not edited by a click
  const mid = task.markerFrom + 1;
  view.dispatch({ changes: { from: mid, to: mid + 1, insert: task.checked ? ' ' : 'x' } });
  return true;
}

/** Toggle a task, or open a comment, at the caret. Follow (links/embeds)
 *  stays owned by `cm-link-nav.ts`'s existing `followKeymap` (registered
 *  separately in `cm-editor.ts`) — this only adds the two cases the spec
 *  names beyond following, and declines (false) for anything else so that
 *  keymap gets its turn. */
export function actAtCaret(view: EditorView): boolean {
  const pos = view.state.selection.main.head;
  const task = taskAt(view.state, pos);
  if (task) return toggleTask(view, task);
  // `commentParsedField` isn't registered when the host disables comment
  // authoring (`options.comments === false` in cm-editor.ts — the email
  // composer's write buffer does this), so read it optionally rather than
  // assuming every caller wires cm-comment.ts's extension.
  const comment = view.state.field(commentParsedField, false)?.find((c) => pos >= c.from && pos <= c.to);
  if (comment) { view.dispatch({ effects: openCommentPopover.of({ from: comment.from }) }); return true; }
  return false;
}

/** Opens the editable card for the link or picture at the caret (a link's URL, a picture's path and width). With a non-empty selection, or
 *  an empty caret not inside either, this module declines (false) and
 *  `cm-format-keys.ts`'s own Mod-k (registered at `Prec.default` in
 *  `cm-editor.ts`, below this module's `Prec.high`) runs next — the same
 *  "selection present → formatMark('link')" wrap/insert path as today. */
export function modK(view: EditorView): boolean {
  const { from, to } = view.state.selection.main;
  if (from === to) {
    const link = referenceSiteAt(view.state, from);
    if (link && !link.wiki) return openCardAt(view, link.from); // the link's own start, not the caret's
    const embed = embedAt(view.state, from);
    if (embed) return openCardAt(view, embed.probe); // inside the embed's target, where the card providers look
  }
  return false;
}

export function actAtCaretExtension(): Extension {
  return [
    Prec.high(keymap.of([
      { key: 'Mod-Enter', run: actAtCaret },
      { key: 'Mod-k', run: modK },
    ])),
    mousePress,
  ];
}
