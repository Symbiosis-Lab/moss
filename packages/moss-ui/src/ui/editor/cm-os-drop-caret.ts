// CM6 StateField that paints a HORIZONTAL block-boundary drop line during OS
// drags (Tauri onDragDropEvent 'over') and during internal drags (HTML5
// dragover, which dropCursor() would normally handle).
//
// The built-in dropCursor() cannot fire for OS drags because Tauri intercepts
// them before they reach the CM DOM (dragDropEnabled:true). We drive the
// indicator manually from Tauri 'over' coords delivered by editor-main.ts.
//
// For internal drags (tree→editor) we ALSO replace dropCursor() with this
// horizontal-line indicator so both paths show an identical UI. Register
// blockDropCursor() in cm-editor.ts INSTEAD of the built-in dropCursor()
// inside the isMarkdown block.
//
// Design (option A, LOCKED):
//   - A 2px horizontal line spanning the CONTENT width, drawn BETWEEN blocks.
//   - Color: var(--moss-color-accent), no glow/box-shadow.
//   - Implementation: Decoration.widget with block:true at the snapped
//     line-start position, rendered as a zero-height <div> with border-top.

import { StateField, StateEffect } from '@codemirror/state';
import { EditorView, Decoration, type DecorationSet, WidgetType, ViewPlugin, type ViewUpdate } from '@codemirror/view';

// ---------------------------------------------------------------------------
// Shared helper — snap a raw doc position to the nearest block boundary
// ---------------------------------------------------------------------------

/**
 * Snap a raw doc position (from posAtCoords) to a block boundary so that
 * embeds always land on their own line between paragraphs.
 *
 * Strategy: if clientY is in the bottom half of the line, snap to the start
 * of the NEXT line (insert after the current line). Otherwise snap to the
 * start of the current line (insert before it).
 *
 * Falls back to line.from when layout data is unavailable (e.g. jsdom).
 */
export function snapToBlockBoundary(
  view: EditorView,
  rawPos: number,
  clientY?: number,
): number {
  const doc = view.state.doc;

  // Guard: real CM6 Text objects have lineAt(); mocked views in tests may not.
  if (typeof doc.lineAt !== 'function') {
    return rawPos;
  }

  const line = doc.lineAt(rawPos);

  if (clientY !== undefined) {
    try {
      // coordsAtPos returns client-space rect for the start of the line.
      const lineCoords = view.coordsAtPos(line.from);
      if (lineCoords) {
        const lineMidY = (lineCoords.top + lineCoords.bottom) / 2;
        if (clientY > lineMidY) {
          // Bottom half of line: snap to start of next line (insert after).
          const nextLineStart = line.to + 1;
          if (nextLineStart <= doc.length) {
            return doc.lineAt(nextLineStart).from;
          }
          // Already on the last line — snap to end of document.
          return doc.length;
        }
      }
    } catch {
      // coordsAtPos can throw in non-rendered views (e.g. jsdom tests).
    }
  }

  // Default: snap to start of current line (insert before it).
  return line.from;
}

// ---------------------------------------------------------------------------
// Shared widget — a zero-height <div> with a 2px accent border-top
// ---------------------------------------------------------------------------

class HorizontalDropLineWidget extends WidgetType {
  toDOM(): HTMLElement {
    const div = document.createElement('div');
    div.className = 'cm-drop-line-indicator';
    return div;
  }
  eq(): boolean { return true; }
  ignoreEvent(): boolean { return true; }
  // Zero estimated height — the widget is purely visual and should not
  // affect the editor's scroll position calculations.
  get estimatedHeight(): number { return 0; }
}

const _dropLineWidget = new HorizontalDropLineWidget();

// ---------------------------------------------------------------------------
// OS-drop path — StateField driven from editor-main.ts Tauri 'over' events
// ---------------------------------------------------------------------------

/** Dispatch this effect to move the OS-drop indicator to a doc position, or
 *  clear it with null. The position MUST already be snapped to a block
 *  boundary (call snapToBlockBoundary before dispatching). */
export const setOsDropCaret = StateEffect.define<number | null>();

/**
 * StateField<number | null> tracking the current OS-drop indicator position.
 * Null = no indicator shown. Provides a block-level Decoration.widget that
 * renders a horizontal 2px accent line between blocks.
 */
export const osDropCaretField = StateField.define<number | null>({
  create: () => null,
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setOsDropCaret)) value = e.value;
    }
    return value;
  },
  provide: (f) =>
    EditorView.decorations.from(f, (pos): DecorationSet => {
      if (pos == null) return Decoration.none;
      // block:true + side:-1 renders the widget BEFORE the line at `pos`,
      // i.e. between the previous block and the block starting at `pos`.
      return Decoration.set([
        Decoration.widget({ widget: _dropLineWidget, block: true, side: -1 }).range(pos),
      ]);
    }),
});

/**
 * Factory that returns the extension array to register in cm-editor.ts.
 * Used inside the `isMarkdown` block so the indicator is only active in
 * markdown files (the only context where OS file drops can embed into the
 * editor).
 */
export function osDropCaret(): typeof osDropCaretField[] {
  return [osDropCaretField];
}

// ---------------------------------------------------------------------------
// Internal-drag path — ViewPlugin that replaces dropCursor() for tree→editor
// ---------------------------------------------------------------------------

/** Effect to set/clear the internal-drag block-snapped indicator position. */
const setInternalDropPos = StateEffect.define<number | null>();

/** StateField for the internal-drag block-snapped indicator position. */
const internalDropPosField = StateField.define<number | null>({
  create: () => null,
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setInternalDropPos)) value = e.value;
    }
    return value;
  },
  provide: (f) =>
    EditorView.decorations.from(f, (pos): DecorationSet => {
      if (pos == null) return Decoration.none;
      return Decoration.set([
        Decoration.widget({ widget: _dropLineWidget, block: true, side: -1 }).range(pos),
      ]);
    }),
});

class BlockDropCursorPlugin {
  constructor(readonly view: EditorView) {}

  setPos(pos: number | null): void {
    if (this.view.state.field(internalDropPosField) !== pos) {
      this.view.dispatch({ effects: setInternalDropPos.of(pos) });
    }
  }

  clearPos(): void {
    this.setPos(null);
  }

  update(_u: ViewUpdate): void {}

  destroy(): void {
    this.clearPos();
  }
}

const _blockDropCursorPlugin = ViewPlugin.fromClass(
  BlockDropCursorPlugin,
  {
    eventObservers: {
      dragover(
        this: BlockDropCursorPlugin,
        event: DragEvent,
      ): void {
        if (!event.dataTransfer?.types.includes('application/x-moss-entries')) {
          // Not our payload — clear any lingering indicator.
          this.clearPos();
          return;
        }
        const rawPos =
          this.view.posAtCoords({ x: event.clientX, y: event.clientY }) ?? null;
        const snapped =
          rawPos != null
            ? snapToBlockBoundary(this.view, rawPos, event.clientY)
            : null;
        this.setPos(snapped);
      },

      dragleave(
        this: BlockDropCursorPlugin,
        event: DragEvent,
      ): void {
        if (
          event.target === this.view.contentDOM ||
          !this.view.contentDOM.contains(event.relatedTarget as Node | null)
        ) {
          this.clearPos();
        }
      },

      dragend(this: BlockDropCursorPlugin): void {
        this.clearPos();
      },

      drop(this: BlockDropCursorPlugin): void {
        this.clearPos();
      },
    },
  },
);

/**
 * Returns the extension array that replaces `dropCursor()` for internal
 * (tree→editor) drags with a horizontal block-boundary line matching the
 * OS-drag indicator.
 *
 * Register this in cm-editor.ts INSTEAD of `dropCursor()` inside the
 * `isMarkdown` block. Non-markdown files retain the built-in `dropCursor()`
 * since they do not support tree→editor embed insertion.
 */
export function blockDropCursor(): [typeof internalDropPosField, typeof _blockDropCursorPlugin] {
  return [internalDropPosField, _blockDropCursorPlugin];
}
