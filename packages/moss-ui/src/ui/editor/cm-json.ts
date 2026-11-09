// cm-json.ts — CM6 extension bundle for JSON files.
//
// Provides:
//   1. JSON parse-error diagnostics via `jsonParseLinter()` + `linter()` +
//      `lintGutter()` (@codemirror/lint + @codemirror/lang-json).
//   2. Fold gutter via `foldGutter()` + `codeFolding()` (@codemirror/language).
//   3. "Delete JSON item" command (Mod-Shift-Backspace) — removes the array
//      element or object property containing the cursor, including the correct
//      comma and surrounding whitespace, leaving valid JSON.
//
// CM6 patterns adopted:
// - jsonParseLinter(): official JSON linter factory from @codemirror/lang-json.
//   Wrapped with linter(src, { delay:200 }) for fast inline diagnostics.
//   Ref: https://codemirror.net/docs/ref/#lang-json.jsonParseLinter
// - foldGutter() + codeFolding(): the two-extension pair required for CM6
//   folding. @codemirror/lang-json registers foldNodeProp on Object/Array.
//   Ref: https://codemirror.net/docs/ref/#language.foldGutter
// - syntaxTree(state): walk the lezer JSON grammar tree to locate the enclosing
//   Property or array-element node for structural deletion.
//   Lezer JSON node names: JsonText, Object, Property, PropertyName,
//   Array, String, Number, True, False, Null, ⚠.
//   Ref: https://codemirror.net/docs/ref/#language.syntaxTree

import { keymap } from '@codemirror/view';
import type { EditorView, Command } from '@codemirror/view';
import type { EditorState, Extension } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';
import { foldGutter, codeFolding } from '@codemirror/language';
import { linter, lintGutter } from '@codemirror/lint';
import { jsonParseLinter } from '@codemirror/lang-json';

// Avoid importing from @lezer/common (not a direct dep in the pnpm manifest).
// Use the type the tree's resolveInner returns instead — see cm-editor.ts comments.
type JsonNode = ReturnType<ReturnType<typeof syntaxTree>['resolveInner']>;

// ── Deletion range computation (pure, testable) ──────────────────────────────

/**
 * Compute the range [from, to) to delete in order to remove the array element
 * or object property that contains `pos`, including the correct comma and
 * inter-element whitespace, so the resulting JSON is valid.
 *
 * Comma strategy:
 *   - Non-last element: delete from `itemNode.from` to `nextSibling.from`.
 *     This removes the item, its trailing comma, and any whitespace/newline
 *     between the comma and the next sibling — the next sibling slides up
 *     cleanly with its own indentation preserved.
 *   - Last element (not only): delete from the preceding comma's start to
 *     `itemNode.to`, then eat any trailing whitespace+newline after the item
 *     unless the very next non-whitespace char is the closing `}` or `]`
 *     (in which case just remove the item itself plus any trailing line-end so
 *     the closing delimiter stays on its own clean line).
 *   - Only element: delete from `itemNode.from` to `itemNode.to` — no comma
 *     exists; the container becomes `[]` or `{}`.
 *
 * Returns null if the cursor is not inside a recognisable JSON item (e.g.
 * cursor on the container's own `{`, `}`, `[`, `]` chars, or in invalid JSON).
 */
export function jsonItemDeleteRange(
  state: EditorState,
  pos: number,
): { from: number; to: number } | null {
  const tree = syntaxTree(state);

  // Walk up from the cursor to find a node whose direct parent is Array or Object.
  let node: JsonNode | null = tree.resolveInner(pos, 1);
  while (node) {
    const p: JsonNode | null = node.parent;
    if (!p) return null;
    if (p.name === 'Array' || p.name === 'Object') break;
    node = p;
  }
  if (!node?.parent) return null;

  const parent: JsonNode = node.parent;
  if (parent.name !== 'Array' && parent.name !== 'Object') return null;

  // In an Object, resolve to the enclosing Property node (cursor may be on a
  // PropertyName or value child; climb if needed).
  let itemNode: JsonNode = node;
  if (parent.name === 'Object' && node.name !== 'Property') {
    let n: JsonNode | null = node;
    while (n && n.name !== 'Property') n = n.parent;
    if (!n) return null;
    itemNode = n;
  }

  // Collect content siblings (skip punctuation tokens: { } [ ] , :).
  const PUNCT = new Set(['{', '}', '[', ']', ',', ':']);
  const siblings: JsonNode[] = [];
  let child = parent.firstChild;
  while (child) {
    if (!PUNCT.has(child.name)) siblings.push(child);
    child = child.nextSibling;
  }

  const idx = siblings.findIndex(
    s => s.from === itemNode.from && s.to === itemNode.to,
  );
  if (idx === -1) return null;

  const itemFrom = itemNode.from;
  const itemTo   = itemNode.to;

  // ── Only element ──────────────────────────────────────────────────────────
  if (siblings.length === 1) {
    return { from: itemFrom, to: itemTo };
  }

  // ── Non-last element ──────────────────────────────────────────────────────
  if (idx < siblings.length - 1) {
    const nextSibling = siblings[idx + 1];
    // `to` = start of next sibling: removes item + comma + inter-element
    // whitespace/newlines between the comma and the next sibling.
    return { from: itemFrom, to: nextSibling.from };
  }

  // ── Last element ──────────────────────────────────────────────────────────
  // Find the preceding comma node (between prevSibling and itemNode).
  const prevSibling = siblings[idx - 1];
  let commaFrom = prevSibling.to;          // fallback: no comma found
  let cursor = prevSibling.nextSibling;
  while (cursor) {
    if (cursor.name === ',') { commaFrom = cursor.from; break; }
    cursor = cursor.nextSibling;
  }

  // `to` = end of item. We deliberately do NOT eat the trailing newline after
  // the item: in multiline JSON that newline is the line-ending of the item's
  // own line, and the closing `}` / `]` lives on the NEXT line with its own
  // indentation. Leaving it in place means the closing delimiter stays on its
  // own line, producing clean multiline output.
  //
  // Example:  [\n  "a",\n  "b",\n  "c"\n]
  //   delete "c": commaFrom=14, itemTo=21 → delete [14..21] = ,\n  "c"
  //   result: [\n  "a",\n  "b"\n]  ✓
  //
  // Single-line case also works naturally: ["a", "b"] delete "b"
  //   commaFrom=4, itemTo=9 → delete [4..9] = , "b" → ["a"]  ✓
  return { from: commaFrom, to: itemTo };
}

// ── CM6 command ──────────────────────────────────────────────────────────────

/**
 * CM6 Command: delete the JSON array element or object property under the
 * cursor (bound to Mod-Shift-Backspace). Leaves valid JSON; handles
 * trailing/preceding commas and surrounding whitespace correctly.
 * Returns false (passes through) when the cursor is not inside a JSON item.
 */
export const deleteJsonItemCommand: Command = (view: EditorView): boolean => {
  const pos = view.state.selection.main.head;
  const range = jsonItemDeleteRange(view.state, pos);
  if (!range) return false;
  view.dispatch({
    changes: { from: range.from, to: range.to, insert: '' },
    selection: { anchor: range.from },
  });
  return true;
};

// ── Extension factory ─────────────────────────────────────────────────────────

/**
 * CM6 extension bundle for JSON files. Includes:
 * - `linter(jsonParseLinter(), { delay:200 })` — parse-error diagnostics.
 * - `lintGutter()` — red-dot left-gutter marker on error lines.
 * - `codeFolding()` + `foldGutter()` — fold triangles for Object/Array nodes.
 * - `Mod-Shift-Backspace` keymap — structural JSON item deletion.
 */
export function jsonEditingExtension(): Extension {
  return [
    linter(jsonParseLinter(), { delay: 200 }),
    lintGutter(),
    codeFolding(),
    foldGutter(),
    keymap.of([
      { key: 'Mod-Shift-Backspace', run: deleteJsonItemCommand },
    ]),
  ];
}
